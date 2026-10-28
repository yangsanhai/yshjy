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
    complete -c $bin -s o -l dest -r -F -d '解压到哪'
    complete -c $bin -s p -l password -x -d '解压码 / 打包密码'
    complete -c $bin -l password-file -r -F -d '密码本：每行一个（# 注释），只对这次命令有效'
    complete -c $bin -l no-auto-password -d '不自动从「说明.txt」和密码本找解压码'
    complete -c $bin -l encoding -x -a 'utf-8 gbk big5 sjis' -d '手动指定压缩包里的文件名编码（big5 / sjis / gbk）'
    complete -c $bin -l select -x -d '只解一部分：'
    complete -c $bin -s j -l jobs -x -a '1 2 4 8' -d '同时跑几个'
    complete -c $bin -s f -l force -d '跳过解压前的磁盘空间预检（空间不够时会直接拒绝开始）'
    complete -c $bin -l no-smart-dir -d '关掉「智能解压」：默认会看包里有没有单一顶层目录来决定'
    complete -c $bin -l keep -d '打包时保留中间 ZIP（排错用）'
    complete -c $bin -l split -x -d '打包时分卷（如 2G / 500M）'
    complete -c $bin -l level -x
    complete -c $bin -l layout -x -a 'L1 L2'
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
    complete -c $bin -l import-passwords -r -F -d '导入密码本（每行一个解压码，# 开头是注释）'
    complete -c $bin -l report
    complete -c $bin -l no-paths
    complete -c $bin -s h -l help
    complete -c $bin -s V -l version
    complete -c $bin -f -a 'jy' -d '解压（完整写法）'
    complete -c $bin -f -a 'ys' -d '打包成伪装文件'
    # 位置参数：文件或目录
    complete -c $bin -F
end
