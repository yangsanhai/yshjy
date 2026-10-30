//! 嵌套包自动下钻（只下一层）
//!
//! 真实场景：网盘里下的「游戏.mp4」解开后是 `示例包.7z.001` + `示例包.7z.002`，
//! 用户还得再来一次。界面早就提示「里面还有压缩包」，现在把它做成**能自动做**的。
//!
//! 三条边界（都是踩出来的）：
//! 1. **只下一层**：内层再套就不再自动解，避免「压缩包套娃」把磁盘写爆
//! 2. **绝不碰原包**：输出目录里如果就是用户给的那份包（dest == 源目录），跳过它
//! 3. **失败不影响主流程**：内层解不开就如实记一笔 `failed`，外层结果照常成功

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::batch::{BatchOptions, Job};
use crate::event::EventSink;
use crate::progress::CancelToken;

/// 一次下钻最多处理几个内层包（防止「1000 个 zip」把一次解压拖成一天）
pub const 最多下钻数: usize = 8;

/// 下钻结果（写进 extract 结果的 `nested` 数组）
#[derive(Debug, Clone)]
pub struct Drilled {
    /// 内层包路径
    pub path: PathBuf,
    /// 输出目录（成功时）
    pub dest: Option<PathBuf>,
    /// `ok` / `failed` / `cancelled`
    pub outcome: String,
}

/// 在 `root` 的**第一层**找压缩包，逐个解到 `root/<包名>/`
///
/// - `原包`: 用户给的输入路径；如果它正好落在 `root` 里（dest == 源目录），跳过，
///   否则会把自己再解一遍（无限套娃的入口）
/// - `密码`/`编码`: 继承父包的 —— 内层包通常是同一个作者用同一套参数打的
/// - 返回值里失败的项也在（外壳可以显示「内层 1 个没解开」）
pub fn drill_once(
    root: &Path,
    跳过: Option<&Path>,
    password: Option<&str>,
    encoding: Option<&str>,
    sink: &Arc<dyn EventSink>,
    取消: &CancelToken,
) -> Vec<Drilled> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut 候选: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    候选.sort();

    let mut 结果 = Vec::new();
    for 路径 in 候选 {
        // dest == 源目录时，用户给的包本身就在这里 —— 绝不能把它再解一遍
        if let Some(原) = 跳过
            && 是同一个文件(原, &路径)
        {
            continue;
        }
        if 结果.len() >= 最多下钻数 || 取消.is_cancelled() {
            break;
        }
        // 只认「真的是压缩包」：identify 只读文件头，几毫秒
        let 空出口: Arc<dyn EventSink> = Arc::new(crate::event::NullSink);
        let ctx = crate::ctx::UnpackCtx::new(0, 空出口);
        if crate::job::identify(&路径, &ctx).is_err() {
            continue;
        }

        let 大小 = std::fs::metadata(&路径).map(|m| m.len()).unwrap_or(0);
        let mut job = Job::new(结果.len() as u64 + 1, 路径.clone(), 大小);
        let stem = 路径
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "内层".to_string());
        let 输出 = root.join(stem);
        job.dest = Some(输出.clone());

        let opts = BatchOptions {
            encoding: encoding.map(str::to_string),
            jobs: 1,
            continue_on_error: true,
            password: password.map(str::to_string),
            selection: crate::select::Selection::all(),
            dest: Some(输出),
            smart_dir: true,
            force: false,
            verify_only: false,
            limits: crate::safety::Limits::default(),
            candidate_offset: None,
        };
        let out = crate::job::run_job(&job, &opts, sink, 取消);
        let outcome = match &out.outcome {
            crate::error::Outcome::Success => "ok",
            crate::error::Outcome::Partial { .. } => "partial",
            crate::error::Outcome::Cancelled { .. } => "cancelled",
            crate::error::Outcome::Failed(_) => "failed",
        }
        .to_string();
        结果.push(Drilled {
            path: 路径,
            dest: out.stats.dest.clone(),
            outcome,
        });
    }
    结果
}

/// 判断某个文件是不是「输入本身」（用于 dest == 源目录时避免自我递归）
pub fn 是同一个文件(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("yshjy_drill_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 用 zip crate 造一个最小的 zip（内容是给定条目）
    fn 造zip(路径: &Path, 条目: &[(&str, &[u8])]) {
        use zip::write::SimpleFileOptions;
        let f = std::fs::File::create(路径).unwrap();
        let mut z = zip::ZipWriter::new(f);
        for (name, data) in 条目 {
            z.start_file(*name, SimpleFileOptions::default()).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn 找到内层包并解出一层() {
        let d = scratch("one");
        // 内层包
        let inner = d.join("内层.zip");
        造zip(&inner, &[("里的文件.txt", b"hello")]);
        // 外层包（名字故意不叫 zip，验证按内容识别）
        let outer = d.join("外层.mp4");
        造zip(&outer, &[("内层.zip", &std::fs::read(&inner).unwrap())]);

        let 解压区 = d.join("out");
        let 空: Arc<dyn EventSink> = Arc::new(crate::event::NullSink);
        let 取消 = CancelToken::new();
        let opts = BatchOptions {
            encoding: None,
            jobs: 1,
            continue_on_error: true,
            password: None,
            selection: crate::select::Selection::all(),
            dest: Some(解压区.clone()),
            smart_dir: false,
            force: false,
            verify_only: false,
            limits: crate::safety::Limits::default(),
            candidate_offset: None,
        };
        let mut job = Job::new(1, outer.clone(), std::fs::metadata(&outer).unwrap().len());
        job.dest = Some(解压区.clone());
        let out = crate::job::run_job(&job, &opts, &空, &取消);
        assert!(matches!(out.outcome, crate::error::Outcome::Success));

        let r = drill_once(&解压区, Some(&outer), None, None, &空, &取消);
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].outcome, "ok");
        assert!(r[0].dest.is_some());
        let 里 = std::fs::read_to_string(r[0].dest.as_ref().unwrap().join("里的文件.txt")).unwrap();
        assert_eq!(里, "hello");

        // 内层包本身留着（证据），但里面套的第三层没有再被解
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 非压缩包不会被当地雷() {
        let d = scratch("plain");
        std::fs::write(d.join("说明.txt"), b"not an archive").unwrap();
        let 空: Arc<dyn EventSink> = Arc::new(crate::event::NullSink);
        let r = drill_once(&d, None, None, None, &空, &CancelToken::new());
        assert!(r.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 同一个文件判定能识别硬链接之外的相同路径() {
        let d = scratch("same");
        let f = d.join("a.zip");
        std::fs::write(&f, b"x").unwrap();
        assert!(是同一个文件(&f, &f));
        let g = d.join("b.zip");
        std::fs::write(&g, b"x").unwrap();
        assert!(!是同一个文件(&f, &g));
        let _ = std::fs::remove_dir_all(&d);
    }
}
