# 发布形态：nsh package（安装与更新同源，issue #2134 / ADR-0145 修订注记 2026-11-15）

切轨时「release 二进制 + .desktop 直跑」的简化形态随自动更新翻案退役：更新器
（nsh-updater）只消费 **nsh package** 打包产物（包内 `nsh/app.json` 与
`nsh/assets/nsh-updater.json` 是换装定位与更新源的硬前提），发版产物迁回
nsh package——目录包既是安装形态也是更新形态（词汇表「更新工件」词条）。

`nsh.json`（`src-app/nsh.json`）承载打包元数据：`id`、`version`（semver，随
版本发布增长，`scripts/check-versions.ts` 与 Cargo/package.json 三处一致守门）、
`icon`（本目录 `ledger.png`）、`updater`（更新源 + 公钥）。

## 更新签名密钥（一次性人工步骤清单）

更新清单强制 Ed25519 分离签名（`nsh keygen` 密钥对，清单 `.sig` 与
`latest.json` 成对上传）；公钥烧入 `src-app/nsh.json` 的 `updater.publicKey`
——已发行客户端无法换锁，私钥保管是长期承诺，泄漏即需重建信任链。

当前 `nsh.json` 里的 publicKey 是**占位值**（打包校验需要合法 Ed25519 公钥
形状；它不对应任何在保管的私钥）。**首次带更新器的发布前必须换成真钥匙**：

1. 离线生成密钥对（`.secret` 不进仓、不入 CI，离线保存一份备份）：

   ```bash
   nsh keygen keys/update    # 产 keys/update.secret 与 keys/update.pub
   ```

2. 把 `keys/update.pub` 的文件内容（一行 base64）替换进
   `src-app/nsh.json` 的 `updater.publicKey`。此改动随首个带更新器的版本
   一起发布——烧入后即不可换（ADR-0124 决策 3 语义沿袭）。
3. `keys/update.secret` 离线保管：签名只在发布机上发生，用完归档；泄漏
   = 重建信任链（新密钥对 + 新公钥随下一个大版本烧入）。

## 发布流程（Linux，打包机 = 构建机原生）

仓库提供脚本接缝 `scripts/publish-nsh-update.sh`（封装下述两步并打印上传清
单）；手工等价命令链：

```bash
# 1. 打包（release 构建自带 runner；产物 .nsh/out/Ledger/ + 平台命名 archive）
nsh package --target linux          # → Ledger-<version>-linux-x86_64.tar.gz

# 2. 清单生成（读 nsh.json 的 id/version；archive 的 sha256/size 自动计算）
nsh update-manifest .nsh/out/Ledger-<version>-linux-x86_64.tar.gz \
  --notes <CHANGES 片段文件> --key keys/update.secret
```

3. 上传三件套到 GitHub Release（`asiazhang/ledger`，updater.github 源）：
   `<name>-<version>-linux-x86_64.tar.gz` + `latest.json` + `latest.json.sig`。
   GitHub `releases/latest/download/latest.json` 端点只认最新 release，且
   draft / prerelease 自动跳过——单 stable 通道由此成立（ADR-0124 决策 9）。

注意事项：

- `nsh.json` 的 `version` 每次发布必须增长（更新器版本只升不降）；`build`
  字段本仓未用（无 OTA 包需求），缺省 1 即可。
- 打包须在 Linux 上进行（runner 为构建机原生）；运行依赖 GTK 4.14+。
- 版本号三处一致（`package.json` / `src-app/Cargo.toml` / `src-app/nsh.json`）
  由 `scripts/check-versions.ts` 守门，漏改一处即红。
- 旧 tauri 清单生成器（tauri-plugin-updater 静态清单形态，minisign 签名，
  scripts/generate-update-manifest.ts）与 NSH 清单格式不兼容，已随本票删除
  （ADR-0145 修订注记 5「旧 tauri 清单格式与生成器直接弃用、无迁移负担」）。

## 端到端人工验收（更新链路）

更新链路的真实端到端（清单 → 下载 → 换装 → 重启）无法在 headless 替身回路
里覆盖（换装要求真包布局 + 真重启），首个带更新器的版本发布时按票面清单人工
执行一遍并回填票面。
