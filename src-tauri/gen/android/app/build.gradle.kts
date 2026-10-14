// Android 发布签名运维 runbook（issue #560 / ADR-0074 决策 3）。release 签名经
// keystore.properties 接线（本文件下方 signingConfigs）；keystore 是「丢失即永远
// 无法给已装用户发升级」的资产，强制顺序：本地生成 keystore → 口令入密码管理器 +
// jks 异地备份（人工暂停点）→ 配置 GitHub Actions secrets → 才启用发版。
//
// 前置：本机无 JDK 时先装一个真实 JDK（`brew install --cask temurin`，或用
// Android Studio 内置 JBR 的 keytool）。
//
// 1. 本地生成 keystore（RSA 2048+、有效期 10000 天）：
//      keytool -genkey -v -keystore ~/openledger-release.jks -alias openledger \
//        -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=OpenLedger"
//    口令在交互提示中输入（不进 shell 历史）；key 口令提示处直接回车（与 store 口令
//    相同，CI 的 keystore.properties 单口令接线按此约定）；口令避免含反斜杠 `\`
//    （Java Properties 转义字符，gradle 会报 Malformed \uxxxx）；jks 本身永不入库
//    （src-tauri/.gitignore 兜底）。
// 2. 人工暂停点（确认前不得继续）：口令 + alias `openledger` + 有效期已录入密码
//    管理器；`openledger-release.jks` 已异地备份（网盘/U 盘皆可，至少一份离开本机）；
//    明确确认「备份完成，可以继续」。
// 3. 配置 secrets（repo Settings → Secrets and variables → Actions，或 gh secret set；
//    口令经 stdin 粘贴）：
//      base64 -i ~/openledger-release.jks -o ~/openledger-release.jks.b64
//      gh secret set ANDROID_KEY_BASE64 < ~/openledger-release.jks.b64
//      gh secret set ANDROID_KEY_PASSWORD    # 回车后按提示粘贴口令
//      gh secret set ANDROID_KEY_ALIAS --body openledger
//      rm ~/openledger-release.jks.b64
//    CI 侧解码与接线见 .github/workflows/build.yml「解码 Android 签名 keystore」步骤
//    （tag 构建缺 secrets 直接失败：未签名 APK 不可安装也无法覆盖升级）；签名验收走
//    workflow_dispatch 试跑，看「解码 Android 签名 keystore」无跳过、「校验 APK 签名
//    （apksigner）」通过、artifact 可下载。
// 4. 发布后人工验收：下载 artifact `openledger-android-arm64`（=
//    `OpenLedger_<版本>_arm64.apk`），传输到真机（下载/adb 均可）安装成功（首次
//    安装放行「未知来源」）并冒烟记账主流程；后续版本必须用同一 keystore 签名才可
//    覆盖升级（versionCode 由 tag 派生，单调递增）。
import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

android {
    compileSdk = 36
    namespace = "com.zhangheng.ledger"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.zhangheng.ledger"
        minSdk = 24
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    // 发布签名（issue #560 / ADR-0074 决策 3）：keystore.properties 由 CI 从 secrets
    // 生成（本地开发通常不存在），存在时 release APK 用它签名；不存在时保持未签名。
    // 配置创建与 signingConfig 赋值都必须条件化——半初始化的 release 签名配置会让
    // 打包直接失败（SigningConfig "release" is missing required property "storeFile"）。
    val keystorePropertiesFile = rootProject.file("keystore.properties")
    signingConfigs {
        if (keystorePropertiesFile.exists()) {
            create("release") {
                val keystoreProperties = Properties().apply {
                    keystorePropertiesFile.inputStream().use { load(it) }
                }
                keyAlias = keystoreProperties["keyAlias"] as String
                keyPassword = keystoreProperties["password"] as String
                storeFile = file(keystoreProperties["storeFile"] as String)
                storePassword = keystoreProperties["password"] as String
            }
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            if (keystorePropertiesFile.exists()) {
                signingConfig = signingConfigs.getByName("release")
            }
            isMinifyEnabled = true
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")