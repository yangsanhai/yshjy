package com.yangsanhai.yshjy

import android.content.ContentValues
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.webkit.MimeTypeMap
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File
import java.io.IOException

/**
 * 安卓入口。两个职责：
 *
 * 1. 把**应用沙箱目录**告诉 Dart 侧（引擎和界面都要一个可写的状态/配置目录；
 *    桌面上靠 XDG / LOCALAPPDATA 算得出，安卓上只有原生知道）。
 * 2. **导出到系统「下载」**：解压落在 App 私有目录（零权限，装上就能用），
 *    用户想拿到共享目录时走这条路 —— 用 MediaStore 一条一条插，
 *    **不需要「所有文件访问」权限**（方案里的 C 路，见 docs/ANDROID-整体方案 §5）。
 */
class MainActivity : FlutterActivity() {
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "yshjy/paths")
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "filesDir" -> result.success(filesDir.absolutePath)
                    "cacheDir" -> result.success(cacheDir.absolutePath)
                    else -> result.notImplemented()
                }
            }

        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "yshjy/export")
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "exportToDownloads" -> {
                        val dir = call.argument<String>("dir")
                        val subDir = call.argument<String>("subDir") ?: "yshjy"
                        if (dir.isNullOrEmpty()) {
                            result.error("BAD_ARG", "缺少目录参数", null)
                            return@setMethodCallHandler
                        }
                        // 几百个大文件拷起来是几十秒，绝不能占着主线程
                        Thread {
                            try {
                                val n = exportToDownloads(File(dir), subDir)
                                runOnUiThread { result.success(n) }
                            } catch (e: Exception) {
                                runOnUiThread {
                                    result.error("EXPORT_FAILED", e.message ?: "导出失败", null)
                                }
                            }
                        }.start()
                    }
                    else -> result.notImplemented()
                }
            }
    }

    /** 把 [root] 目录下的所有文件导出到 `Download/<subDir>/…`，返回导出个数 */
    private fun exportToDownloads(root: File, subDir: String): Int {
        if (!root.exists()) return 0
        val files = root.walkTopDown().filter { it.isFile }.toList()
        if (files.isEmpty()) return 0

        var count = 0
        for (f in files) {
            val rel = f.relativeTo(root).invariantSeparatorsPath
            val parent = rel.substringBeforeLast('/', "")
            val name = f.name
            val mime = MimeTypeMap.getSingleton()
                .getMimeTypeFromExtension(name.substringAfterLast('.', "").lowercase())
                ?: "application/octet-stream"

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                // Android 10+：MediaStore 是现代做法，不需要任何存储权限
                val values = ContentValues().apply {
                    put(MediaStore.MediaColumns.DISPLAY_NAME, name)
                    put(MediaStore.MediaColumns.MIME_TYPE, mime)
                    val path = if (parent.isEmpty()) "$subDir" else "$subDir/$parent"
                    put(MediaStore.MediaColumns.RELATIVE_PATH, "Download/$path")
                }
                val uri = contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
                    ?: throw IOException("系统拒绝写入「$name」（同名文件太多或被限制？）")
                val out = contentResolver.openOutputStream(uri)
                    ?: throw IOException("打不开输出流：$name")
                out.use { o -> f.inputStream().use { i -> i.copyTo(o) } }
            } else {
                @Suppress("DEPRECATION")
                val dl = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
                val target = File(dl, "$subDir/$rel")
                target.parentFile?.mkdirs()
                f.copyTo(target, overwrite = true)
            }
            count++
        }
        return count
    }
}
