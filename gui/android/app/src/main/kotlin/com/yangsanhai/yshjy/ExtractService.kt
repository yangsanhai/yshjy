package com.yangsanhai.yshjy

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder

/**
 * 解压期间的前台服务。
 *
 * 为什么必须有：解一个 2 GB 的包要几分钟，用户切到微信再回来，安卓在后台会把
 * 普通进程直接杀掉 —— 解压解到一半没了。挂上前台服务（`dataSync` 类型）才算
 * 「用户知情的前台工作」。
 *
 * 两条平台硬要求：
 * - **Android 14 起漏写 `foregroundServiceType` 会直接崩**（`MissingForegroundServiceTypeException`），
 *   所以 `startForeground` 要带类型（见下），清单里也要声明。
 * - Android 15 起 `dataSync` 有 6 小时上限，超时系统会停 —— 解压这种分钟级任务够用。
 */
class ExtractService : Service() {
    companion object {
        const val CHANNEL_ID = "yshjy_extract"
        const val NOTIFICATION_ID = 1001
    }

    override fun onCreate() {
        super.onCreate()
        val nm = getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
            nm.getNotificationChannel(CHANNEL_ID) == null
        ) {
            val ch = NotificationChannel(
                CHANNEL_ID,
                "解压进度",
                NotificationManager.IMPORTANCE_LOW, // 低优先级：不出声、不弹横幅
            ).apply { description = "正在解压时显示" }
            nm.createNotificationChannel(ch)
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val title = intent?.getStringExtra("title") ?: "杨三海解压"
        val text = intent?.getStringExtra("text") ?: "正在解压，请稍候…"
        val n: Notification = Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_download)
            .setContentTitle(title)
            .setContentText(text)
            .setOngoing(true)
            .build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIFICATION_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            startForeground(NOTIFICATION_ID, n)
        }
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
            stopForeground(STOP_FOREGROUND_REMOVE)
        } else {
            @Suppress("DEPRECATION")
            stopForeground(true)
        }
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null
}
