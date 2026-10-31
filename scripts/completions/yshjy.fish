# ⚠️ 本文件由 scripts/gen-cli-assets.py 生成，**别手改** —— 改了下次打包就被覆盖。
# 要加/改选项：改 src/cli_spec.rs 的 OPTS 表，然后跑一次生成器。
# fish 补全 —— yshjy（杨三海解压）
#
# 装法（用户级，不用 sudo）：
#   cp scripts/completions/yshjy.fish ~/.config/fish/completions/yshjy.fish

# 不自动补全选项的值（文件名由下面的规则补）
complete -c yshjy -f
complete -c yshys -f

for bin in yshjy yshys
    complete -c $bin -s l -l list
    complete -c $bin -l rescue
    complete -c $bin -l extract-cover
    complete -c $bin -s o -l dest -r -a "(__fish_complete_directories)" -d '解压到哪'
    complete -c $bin -s p -l password -x -d '解压码 / 打包密码'
    complete -c $bin -l password-file -r -F -d '密码本：每行一个（# 注释），只对这次命令有效'
    complete -c $bin -l no-auto-password -d '不自动从「说明.txt」和密码本找解压码'
    complete -c $bin -l no-auto-password-notes -d '只关「从说明.txt 抓解压码」这一档'
    complete -c $bin -l no-auto-password-book -d '只关「试自己的密码本」这一档'
    complete -c $bin -l encoding -x -a 'utf-8 gbk big5 sjis' -d '手动指定文件名编码（auto / gbk / big5 / sjis / utf-8；gb18030、shift-jis 是别名）'
    complete -c $bin -l select -x -d '只解一部分：'
    complete -c $bin -l candidate -r -F -d '一个文件里藏着好几处时，指定解哪一处（偏移＝扫描结果里那个数字）'
    complete -c $bin -l pick -d '全屏勾选要解哪些（↑↓ 选 · 空格勾 · a 全选 · Enter 确认）'
    complete -c $bin -s j -l jobs -x -a '1 2 4 8' -d '同时跑几个'
    complete -c $bin -s f -l force -d '跳过解压前的磁盘空间预检（空间不够时会直接拒绝开始）'
    complete -c $bin -l test -d '只测不写：完整解码 + 校验，一个字节都不落盘（查包完不完整用）'
    complete -c $bin -l no-smart-dir -d '关掉「智能解压」：默认会看包里有没有单一顶层目录来决定'
    complete -c $bin -l keep -d '打包时保留中间 ZIP（排错用）'
    complete -c $bin -l trash-source -d '解完把原包移进回收站（能还原）'
    complete -c $bin -l delete-source -d '解完永久删除原包（不可恢复；会先问一次，非交互直接拒绝）'
    complete -c $bin -l split -x -d '打包时分卷（如 2G / 500M，也认纯字节数）'
    complete -c $bin -l format -x -a 'zip tar.gz tar.zst' -d '打包成普通归档（不加外壳）：zip / tar.gz / tar.zst。给了就是普通压缩，不给还是伪装包'
    complete -c $bin -l name -r -F -d '改产物的文件名（伪装成 电影.2026.1080p.mp4 这种）；只换名字不换位置，位置用 -o'
    complete -c $bin -l tweak-time -d '防炸链：把条目时间戳打乱（每次都不一样），网盘就不会判成同一个文件；默认关'
    complete -c $bin -l level -x
    complete -c $bin -l layout -x -a 'L1 L2'
    complete -c $bin -l decoy -x -d '打包时撒 N 段干扰数据（最多 64 段，默认 0 = 不打）'
    complete -c $bin -l decoy-size -x -d '每段多大（4K–1M，默认 16K）'
    complete -c $bin -l decoy-signatures -d '每段里夹一枚假特征码（骗扫描器；默认关）'
    complete -c $bin -l cover -r -F -d '用自己的图片/视频当封面'
    complete -c $bin -l tail -d '打包用"尾部追加"布局'
    complete -c $bin -l no-checksum -d '不生成 .sha256 校验和'
    complete -c $bin -l no-readme -d '不生成 .说明.txt'
    complete -c $bin -l sfx
    complete -c $bin -l sfx-linux
    complete -c $bin -l json -d '按行输出事件流/结果（给脚本用）'
    complete -c $bin -s q -l quiet -d '只输出结果（脚本用）'
    complete -c $bin -l no-color -d '不上色（管道 / 日志友好）'
    complete -c $bin -l verbose
    complete -c $bin -l log -r -F -d '把日志写进文件（报错时发给开发者）'
    complete -c $bin -l lang -x -a 'zh-CN zh-TW en-US ja-JP ko-KR ru-RU es-ES' -d '切换界面语言（如 en、ja、ko、ru、es）'
    complete -c $bin -l config -r -F -d '用指定的配置文件'
    complete -c $bin -l show-config -d '看当前配置（配置文件在哪、各项现在是什么值）'
    complete -c $bin -l caps -d '看各格式的能力表（能解 / 能打包 / 加密 / 分卷 / 续传）'
    complete -c $bin -l import-passwords -r -F -d '导入密码本（每行一个解压码，# 开头是注释；Tab 分隔取第一列，自动去重）'
    complete -c $bin -l report
    complete -c $bin -l no-paths
    complete -c $bin -s h -l help
    complete -c $bin -s V -l version
    complete -c $bin -f -a 'jy' -d '解压（完整写法）'
    complete -c $bin -f -a 'ys' -d '打包成伪装文件'
    # 位置参数：文件或目录
    complete -c $bin -F
end
