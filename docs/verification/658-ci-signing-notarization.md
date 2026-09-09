# CI 签名 + 公证：secrets 说明与验收清单（issue #658 / 父任务 #645）

> `.github/workflows/build.yml` 的 release job 在 macOS 矩阵上完成 Developer ID
> 签名（`tauri.conf.json` 的 `signingIdentity`，#657 落地）与公证（notarytool
> `--wait` + stapler 附着），使 `v*` tag 发布的 DMG 产物可被 Gatekeeper 放行。
> 证书与公证凭据全部经 GitHub Actions secrets 注入，本文件记录各 secret 的
> 来源、CI 读取方式与发布产物的人工验收清单。

## Actions secrets 清单与来源

配置入口：repo Settings → Secrets and variables → Actions。五个 secret 缺一不可
（签名与公证同时启用；未配置时 CI 跳过签名步骤、产物保持未签名，仅供干跑）。

| Secret | 内容 | 获取方式 |
| --- | --- | --- |
| `APPLE_CERT_P12` | Developer ID Application 签名身份 `.p12`（含私钥）的 base64 编码 | 钥匙串访问导出签名身份（含私钥）为 `.p12`，再 `base64 -i identity.p12 -o identity.p12.b64`；证书申请流程见 issue #660 |
| `APPLE_CERT_PASSWORD` | 导出 `.p12` 时设置的密码 | 导出时自设 |
| `APPLE_ID` | Apple 账号邮箱（公证认证用） | Apple Developer 账号 |
| `APPLE_APP_SPECIFIC_PASSWORD` | 应用专用密码（notarytool 认证用） | appleid.apple.com → 登录与安全 → App 专用密码生成 |
| `APPLE_TEAM_ID` | 团队 ID | Apple Developer 账户页（当前团队：`75N2YA2H9Q`） |

## CI 读取方式（build.yml release job，macOS 矩阵）

1. **导入证书**：`APPLE_CERT_P12` base64 解码 → 创建 runner 临时钥匙串（密码随机
   生成，不落 secrets）→ 导入 `.p12` 并 `set-key-partition-list` 免交互访问。
2. **构建签名**：`tauri build` 按 `tauri.conf.json` 的 `signingIdentity`
   （"Developer ID Application" 前缀匹配）+ 默认 hardened runtime 签名。
   **与 issue #658 步骤 4 的显式偏差：不接 entitlements 文件**——#657 证据矩阵
   （`scripts/biometry-poc/run.sh` 头注释）实证 keychain-access-groups 类受限
   entitlement 在 Developer ID 分发下会被 AMFI SIGKILL（须 provisioning profile
   背书，不可得），故刻意省略，签名仅用 signingIdentity + hardened runtime。
3. **公证**：`xcrun notarytool store-credentials`（`APPLE_ID` +
   `APPLE_APP_SPECIFIC_PASSWORD` + `APPLE_TEAM_ID`，缺失即 fail fast，不等到
   提交时才报错）→ DMG 本体 `notarytool submit --wait` → `xcrun stapler staple`
   附着票据。
   注：tauri 的 dmg 打包流程结束后会清掉 `bundle/macos` 下的 `.app`
   （实测），公证输入只有 DMG。issue 步骤 6 的「及 zip」分发暂缓——zip 内
   `.app` 自带票据需改用 tauri 内建公证或调整打包顺序，另开 ticket 处理。
4. **校验**：`xcrun stapler validate`（DMG）→ 临时挂载 DMG
   （`hdiutil attach -readonly`）验内部 `.app`：`codesign --verify --strict`、
   签名身份必须是 Developer ID Application、hardened runtime 标志必须存在
   （防止 bundler 默认行为变化）——任一失败即发布失败。`spctl --assess` 需 GUI
   会话上下文（headless CI 返回 Insufficient Context 误报，run 34211413678），
   挪至人工验收清单。
5. **清理**：删除临时钥匙串并恢复 login 钥匙串为默认（`always()`，失败路径
   同样清理）。

公证失败排查：`notarytool submit --wait` 报错后查提交详情
（`xcrun notarytool log <submission-id> --keychain-profile <profile>`），常见
原因是受限 entitlement 缺 provisioning 背书（见 `scripts/biometry-poc/run.sh`
头注释的证据矩阵）或二进制内含未签名依赖。

## tag 发布产物人工验收清单

> 自动校验已在 CI 内完成；本清单用于发布后终验（Gatekeeper 放行）。

- [ ] `xcrun stapler validate OpenLedger_<版本>_aarch64.dmg` 通过（CI 已验，此处终验）。
- [ ] `spctl --assess --type open -vv <dmg>`：本机 GUI 会话执行，显示 accepted
  （CI 无 GUI 上下文会误报 Insufficient Context，勿在 CI 断言）。
- [ ] 挂载 DMG 拷出 `.app`：`codesign -dv --verbose=4` 显示 Developer ID
  Application 与 TeamIdentifier；`spctl --assess --type execute -vv` accepted。
- [ ] 在未登录开发者账号的机器上首次启动：Gatekeeper 提示「已验证」放行
  （无「无法验证开发者」拦截）。

## 记录

| 日期 | tag | 结果 | 备注 |
| --- | --- | --- | --- |
|      |     |      |     |
