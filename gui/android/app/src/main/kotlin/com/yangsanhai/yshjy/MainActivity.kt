package com.yangsanhai.yshjy

import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

/**
 * 安卓入口。
 *
 * 唯一职责：把**应用沙箱目录**告诉 Dart 侧。
 *
 * 为什么需要：引擎与界面都要一个可写的状态/配置目录。桌面上靠 XDG / LOCALAPPDATA 算得出，
 * 安卓上却只有原生才知道（`filesDir` = `/data/user/0/com.yangsanhai.yshjy/files`）。
 * 界面里的 `platform_paths.dart` 早就留了口子（`setPlatformStateDir`），就是等这里注入；
 * 不注入的话它会退到自己算的 `~/.config/yshjy`，在安卓上根本写不进去。
 */
class MainActivity : FlutterActivity() {
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "yshjy/paths")
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    // 状态 + 配置都放这儿：卸载 App 会一起清掉，符合「默认私有目录」方案（A）
                    "filesDir" -> result.success(filesDir.absolutePath)
                    "cacheDir" -> result.success(cacheDir.absolutePath)
                    else -> result.notImplemented()
                }
            }
    }
}
