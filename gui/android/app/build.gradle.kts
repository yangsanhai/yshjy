import java.io.FileInputStream
import java.util.Properties

plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

// 发布签名：一次性生成的 keystore（gui\android\app\yshjy-release.jks + key.properties）
// 两个文件都不入库（.gitignore），签名丢了以后就发不了能覆盖安装的升级包 ——
// 备份在 D:\Work\yshjy-win\签名备份\。没有这两个文件时回退到 debug 签名（本地调试用）。
val keystorePropertiesFile = file("key.properties")
val keystoreProperties = Properties().apply {
    if (keystorePropertiesFile.exists()) {
        FileInputStream(keystorePropertiesFile).use { load(it) }
    }
}

android {
    namespace = "com.yangsanhai.yshjy"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "com.yangsanhai.yshjy"
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName

        // 注意：这里**不能**写 ndk.abiFilters —— `flutter build apk --split-per-abi`
        // 自己会设 splits 的 abi 过滤，两边同设会直接报
        // "Conflicting configuration ... cannot be present when splits abi filters are set"。
        // 打进去的 .so 由 jniLibs/ 里放什么决定（scripts/build-android.ps1 只放 arm64/armv7）。
    }

    signingConfigs {
        create("release") {
            if (keystorePropertiesFile.exists()) {
                keyAlias = keystoreProperties.getProperty("keyAlias")
                keyPassword = keystoreProperties.getProperty("keyPassword")
                storeFile = file(keystoreProperties.getProperty("storeFile"))
                storePassword = keystoreProperties.getProperty("storePassword")
            }
        }
    }

    buildTypes {
        release {
            signingConfig = if (keystorePropertiesFile.exists()) {
                signingConfigs.getByName("release")
            } else {
                // 没有 keystore 时不要让构建直接失败：退回 debug 签名，至少能装能测
                signingConfigs.getByName("debug")
            }
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}
