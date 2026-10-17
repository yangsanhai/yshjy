# fish 补全 —— yshjy（杨三海解压）
#
# 装法（用户级，不用 sudo）：
#   cp scripts/completions/yshjy.fish ~/.config/fish/completions/yshjy.fish

# 不自动补全选项的值（文件名由下面的规则补）
complete -c yshjy -f
complete -c yshys -f

for bin in yshjy yshys
    complete -c $bin -s h -l help -d '显示帮助'
    complete -c $bin -l version -d '显示版本与支持的格式'
    complete -c $bin -s q -l quiet -d '安静模式：只输出结果路径'
    complete -c $bin -s l -l list -r -F -d '只列出包里有什么（不解压）'
    complete -c $bin -l rescue -r -F -d '救援模式：索引坏了也按文件头抢救'
    complete -c $bin -l report -d '生成报障用的诊断包'
    complete -c $bin -l no-paths -d '诊断包里的路径脱敏'
    complete -c $bin -l json -d '事件流按行输出'
    complete -c $bin -l show-config -d '显示当前配置'
    complete -c $bin -l tail -d '打包：尾部追加布局'
    complete -c $bin -l force -d '跳过安全预检'
    complete -c $bin -l no-smart-dir -d '关掉智能解压目录规则'
    complete -c $bin -s o -l dest -r -F -d '输出目录'
    complete -c $bin -s p -l password -x -d '解压码（不回显）'
    complete -c $bin -s j -l jobs -x -a '1 2 4 8' -d '并行任务数'
    complete -c $bin -l select -x -d '只解一部分（序号/通配/required）'
    complete -c $bin -l lang -x -a 'zh-CN zh-TW en-US ja-JP ko-KR ru-RU es-ES' -d '界面语言'
    complete -c $bin -l import-passwords -r -F -d '导入密码本'
    complete -c $bin -f -a 'jy' -d '解压（完整写法）'
    complete -c $bin -f -a 'ys' -d '打包成伪装文件'
    # 位置参数：文件或目录
    complete -c $bin -F
end
