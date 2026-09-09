# Android 发布签名：keystore 备份、secrets 说明与验收清单（issue #560 / 父任务 #557）

> `.github/workflows/build.yml` 的 release job 在 Android 矩阵上完成自签名
> （ADR-0074 决策 3）：`ANDROID_KEY_*` secrets 经 base64 解码与
> `keystore.properties` 注入 gradle release 签名，构建后 `apksigner verify`
> 校验最终 APK。keystore 是「丢失即永远无法给已装用户发升级」的资产，流程
> 强制顺序：**本地生成 keystore → 口令入密码管理器 + jks 异地备份（人工暂停
> 点）→ 配置 GitHub Actions secrets → 才启用发版**。本文件记录各 secret 的
> 来源、设置命令与发布产物的人工验收清单。

## 前置：本地 keytool

本机无 JDK（`/usr/libexec/java_home` 报 "Unable to locate a Java Runtime"，
`/usr/bin/keytool` 为系统空壳）。生成 keystore 前先装一个真实 JDK：

```sh
brew install --cask temurin
```

（或安装 Android Studio，用其内置 `/Applications/Android Studio.app/Contents/jbr/Contents/Home/bin/keytool`。）

## 第 1 步：本地生成 keystore（RSA 2048+、有效期 10000 天）

```sh
keytool -genkey -v \
  -keystore ~/openledger-release.jks \
  -alias openledger \
  -keyalg RSA -keysize 2048 -validity 10000 \
  -dname "CN=OpenLedger"
```

- 口令在 keytool 交互提示中输入（不进 shell 历史）；key 密码提示处直接回车
  （与 keystore 口令相同，CI 的 `keystore.properties` 单口令接线即按此约定）。
- 口令避免包含反斜杠 `\`：`keystore.properties` 是 Java Properties 文件，
  反斜杠是转义字符（会以 `Malformed \uxxxx encoding` 之类的 gradle 报错收场）。
- 产物 `~/openledger-release.jks` 本身永不入库（`src-tauri/.gitignore` 已兜底）。

## 第 2 步：人工暂停点（确认前不得继续）

- [ ] keystore 口令已录入维护者密码管理器（口令 + alias `openledger` + 有效期）。
- [ ] `openledger-release.jks` 已异地备份（网盘/U 盘皆可，至少一份离开本机）。
- [ ] 明确确认「备份完成，可以继续」。

## 第 3 步：配置 GitHub Actions secrets

配置入口：repo Settings → Secrets and variables → Actions，或 `gh secret set`。
口令经 stdin 粘贴（不进 shell 历史）；base64 文件可先落地再重定向：

```sh
base64 -i ~/openledger-release.jks -o ~/openledger-release.jks.b64
gh secret set ANDROID_KEY_BASE64 < ~/openledger-release.jks.b64
gh secret set ANDROID_KEY_PASSWORD    # 回车后按提示粘贴口令
gh secret set ANDROID_KEY_ALIAS --body openledger
rm ~/openledger-release.jks.b64
```

| Secret | 内容 | 来源 |
| --- | --- | --- |
| `ANDROID_KEY_BASE64` | keystore 文件的 base64 编码 | `base64 -i ~/openledger-release.jks` |
| `ANDROID_KEY_PASSWORD` | keystore 口令（store/key 同一口令，第 1 步约定） | keytool 交互时输入 |
| `ANDROID_KEY_ALIAS` | key 别名 | 固定 `openledger` |

## CI 读取方式（build.yml release job，Android 矩阵）

1. **解码**：`ANDROID_KEY_BASE64` base64 解码 → `$RUNNER_TEMP/openledger-release.jks`；
   口令与路径写入 `src-tauri/gen/android/keystore.properties`（gradle 官方接线，
   已 gitignore）。`secrets` 未配置时干跑整步跳过（产物保持未签名，与 macOS 证书
   行为一致）；**tag 构建缺 secrets 直接失败**——「先配 secrets 才启用发版」
   （ADR-0074 决策 3）的机械执行，未签名 APK 不可安装也无法覆盖升级。
2. **构建签名**：gradle release buildType 条件挂 `signingConfig`（仅当
   `keystore.properties` 存在；无条件挂载会因半初始化配置直接打包失败）。
3. **校验**：`apksigner verify --print-certs` 校验归一命名后的最终 APK
   （`OpenLedger_<版本>_arm64.apk`）。不用 `jarsigner`——minSdk 24 时 AGP 默认
   仅 v2+ scheme 签名，jarsigner 只认 v1，会误报未签名。

## workflow_dispatch 试跑

Actions → Build → Run workflow（选分支），release 矩阵四行并行；publish 仅
tag 触发，不会误建 Release。签名验收看 Android 行：

- [ ] 「解码 Android 签名 keystore」步骤无 ⚠️ 跳过。
- [ ] 「校验 APK 签名（apksigner）」通过（--print-certs 显示 `openledger` 证书）。
- [ ] 下载 artifact `openledger-android-arm64`，得到 `OpenLedger_<版本>_arm64.apk`。

## 真机安装人工验收清单

- [ ] APK 传输到真机（下载/adb 均可）安装成功（首次安装需放行「未知来源」）。
- [ ] 应用启动并完成记账主流程冒烟（跨模块旅程归 e2e BDD，此处仅人工冒烟）。
- [ ] 后续版本用**同一 keystore** 签名才可覆盖升级（versionCode 单调递增，
  CI 按 tag 派生）。

## 记录

| 日期 | 触发 | 结果 | 备注 |
| --- | --- | --- | --- |
|     |     |      |     |
