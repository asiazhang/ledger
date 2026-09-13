# CI 编译与缓存调优基线（issue #1110 / 父 spec #1086）

> 本票在 #1109 预构建镜像（apt 归零）之上做 CI 侧编译与缓存调优：CI 降调试信息、
> 实测增量编译收益、刷新 CI 注释里的耗时基线，并把「既定约束下 180s 不可达」的
> 推算写回注释止损。对外口径住 `.github/workflows/build.yml`（文件头 + backend /
> backend-lint job 注释），本文件是实测数据、推导过程、口径与局限。

## 交付物

- `.github/workflows/build.yml`：backend job `CARGO_PROFILE_DEV_DEBUG: "1"` → `"0"`；
  文件头耗时基线刷新；backend job 注释补四段拆解与「250s 目标 / 180s 不可达」推算；
  backend-lint job 注释说明为何刻意不设该变量。
- 本文件：数据与局限。
- 未改动的项：job 结构（仍是四 job + 并行分片，未拆后端测试）、测试范围与 target
  选择（`cargo test --workspace --lib --test '*'` 原样）、链接器与 RUSTFLAGS、
  rust-cache 输入（`prefix-key` / `save-if` 保持）、本地开发默认（本地不设
  `CARGO_PROFILE_DEV_DEBUG`，profile.dev 仍是 debuginfo=2 全量）。

## 一、CI 侧调试信息降级（1 → 0）

### 作用面（scratch workspace 三组对照实测）

用最小 workspace 复现本仓的 profile 结构（成员 `member` + 非成员依赖 `dep` +
`[profile.dev.package."*"] debug = "line-tables-only"`），`cargo build -v` 看 rustc
实际收到的 `-C debuginfo`：

| 包 | `CARGO_PROFILE_DEV_DEBUG` 未设 | `=1` | `=0` |
| --- | --- | --- | --- |
| workspace 成员 | `2`（全量） | `1` | 无该旗标 |
| 依赖（`package."*"` 覆盖） | `line-tables-only` | `line-tables-only` | `line-tables-only` |

即：本变量**只**作用于 workspace 成员（本仓 16 个成员 crate + 根包 tauri-app 的
17 个 lib 目标与 5 个集成测试 target），依赖（含 dev-dependencies）的 debuginfo 由
`src-tauri/Cargo.toml` 的 `[profile.dev.package."*"]` 钉在 line-tables-only 上，与本变量无关。

### 收益（本机实测，macOS arm64）

口径：`cargo test --workspace --lib --test '*' --no-run`，依赖产物已在位（等价 CI
「缓存恢复完成后」的状态），**全量重编 workspace 成员**（用逐 crate 追加/撤销一行
注释的内容改动绕过 kache，使每组都是真实编译；改动在测量后已用 `git checkout --`
还原，未进入提交）。测量机为仓库开发机 macOS arm64（12 核）、kache 作为
rustc-wrapper 命中依赖产物；测量期间其它 worktree 正在并发构建，load average ≈ 20，
故 wall 噪声大，附 CPU 时间（user+sys，负载不敏感）：

| 变体 | wall | user+sys |
| --- | --- | --- |
| debug=1, inc=0（样本 1） | 72.4s | 218.3s |
| debug=1, inc=0（样本 2） | 82.9s | 225.4s |
| debug=0, inc=0（样本 1） | 83.4s | 167.2s |
| debug=0, inc=0（样本 2） | 51.7s | 152.5s |
| debug=0, inc=1（样本 1） | 47.0s | 173.5s |

结论：CPU 时间 −28%（≈ −70s 的 rustc 工作量），wall 差异被并发负载淹没、不可比。
落到 CI 侧，收益只体现在编译段（实测基线 82–116s，Linux x64 4 vCPU），方向与
spec #1086 预估的「省 10–20s」一致，但本机数据无法外推到 CI 的秒数；逐 run 净收益
待本票 merge 后的 main 运行回填（见「未验证项」）。

### 缓存体积与解压时间：本变量**不改变**（推翻 spec #1086 的预估，留痕止损）

spec #1086 预计「缓存体积 2 GB → 约 1 GB、解包时间砍半」。核实结论：该预期不成立，
理由三条，前两条是 rust-cache v2 源码事实，第三条是本仓 CI 直接证据：

1. rust-cache 只缓存**依赖产物**。保存阶段先取「workspace 根之外的包」
   （`src/workspace.ts` 的 `getPackagesOutsideWorkspaceRoot()`，判据是 manifest
   路径不以 workspace 根开头）再 `cleanTargetDir`，把其余 `deps/`、`build/`、
   `.fingerprint/` 条目全部删除；README 明确「the workspace crates themselves are
   not cached」。本仓各 job 均用默认 `cache-workspace-crates: false`。
   （源码引用针对 `Swatinem/rust-cache@v2`；CI 实测解析到 v2.9.2 / SHA `6323deb1`。）
2. 依赖产物的 debuginfo 已被 `[profile.dev.package."*"]` 钉在 line-tables-only，
   与 `CARGO_PROFILE_DEV_DEBUG` 无关（见上一节对照表）。
3. CI 直接证据：main run `34749230834`（成员 16 个 crate 全量冷编，debug=1）里
   rust-cache Post 保存 `1236735941 B`，与同一 job 恢复的 `1236768034 B` 基本等大
   ——若成员产物真进了缓存，保存体积会远大于恢复体积。

因此 CI 缓存体积/解压不受本票影响。spec #1086 里的 2.0 GB（下载 12s + 解压 40s）
是**依赖侧** line-tables-only 生效（`db611ca7`，2026-09-06，
`[profile.dev.package."*"]` 引入）之前的历史值；此后依赖缓存稳定在 1.14–1.18 GB
（本票基线），恢复段（下载+解压）30–42s，这部分收益**已由 db611ca7 取得**，不是
本票新增。

代价一（有意接受）：CI 失败时 backtrace 只有符号名、没有行号（断言位置仍由测试输出
给出）。

代价二（一次性，实测）：`CARGO_`/`RUST_`/`CC` 前缀环境变量进入 rust-cache 的
**restore key** 的 env hash（`src/config.ts` 的 `envPrefixes`；CI 日志的
"Environment considered" 段可见 `CARGO_PROFILE_DEV_DEBUG`），本变量改动使该 job 的
env hash 从 `259c68f2`（debug=1）变为 `7392abf3`（debug=0）——restore key 前缀随之
变化，GitHub 的前缀回退匹配不上旧缓存，于是带本改动的分支下一次运行是**全量冷编**
（依赖也重编），而不是「只重编成员」：

| run | 缓存恢复 | 编译 | job 端到端 | 结论 |
| --- | --- | --- | --- | --- |
| 34751213388（本 PR，过渡态） | 1s（miss） | 5m36s | 8m11s | 一次性代价 |
| 基线均值（同 job，第三节） | 30–42s | 82–116s | 258–306s | 稳态 |

影响面仅限带本改动的分支与 merge 后首个 main 运行（其余分支/PR 的 env hash 不变，
backend-lint 未设本变量），之后新 key 缓存固化即回到稳态。备选处置（本票未采用、
留给维护者取舍）：把本变量降为 cargo 步骤级 `env`，env hash 保持 `259c68f2`，缓存
内容不受影响（依赖产物与本变量无关，见本节 2），代价是该变量不再出现在 rust-cache
的「Environment considered」清单里。

## 二、增量编译：实测后**否决**（不采纳 `CARGO_INCREMENTAL`）

事实前提：

- rust-cache v2（CI 实测 v2.9.2 / SHA `6323deb1`）在 restore 阶段
  `exportVariable("CARGO_INCREMENTAL", 0)`
  （`src/restore.ts`），即 CI 现状就是关闭增量；
- 保存前删除增量产物（README「Before being persisted, the cache is cleaned of: …
  Incremental build artifacts」），故跨运行的增量状态不存在；
- 同一 job 内每个 crate 只编一次，也没有 job 内复用机会。

本机对照（同一份内容、同一 target dir，仅切 `CARGO_INCREMENTAL`，全量重编 workspace；
各 1 次样本，wall 受并发负载影响，判据取 CPU 时间与结构论证）：

| 变体 | wall | user+sys |
| --- | --- | --- |
| inc=0 | 51.7s | 152.5s |
| inc=1（增量状态为空） | 47.0s | 173.5s（+14% CPU） |

增量产物体积 `1.7 GB`（本机 kache 归置到 `target/debug/incremental.kache-preserved`），
约为现有依赖缓存（1.14–1.18 GB）的 1.5 倍——要让它跨运行产生收益就得把它一并纳入
缓存，缓存体积与解压时间都会显著上升，而 CI 侧收益为零。

结论：**否决**。CI 保持 rust-cache 的默认（incremental 关闭），workflow 不设
`CARGO_INCREMENTAL`。本地热回路不需要它：kache 命中时 workspace 全量重编只需 4.6s
（本机实测），且 `.kache.toml` 的 `preserve_incremental` 保本地 rustc 增量。

## 三、耗时基线刷新（2026-09-13）

来源：7 次成功的 PR 运行（#1109 落地之后、本票之前）：后端测试 job 端到端取
run `34747049405` / `34747417399` / `34747480095` / `34747583723` / `34748550902` /
`34748847869` / `34749024124`。runner 画像：`ubuntu-latest`（Linux x64，4 vCPU，
GitHub 托管）+ job 级预构建容器镜像 `ghcr.io/asiazhang/ledger-ci-backend:latest`；
缓存状态：rust-cache 精确 key 命中（恢复 1138–1179 MB 依赖缓存，Post 不保存——
`save-if: main`）。

### 后端测试 job（关键路径）

| run | 端到端 | 容器初始化 | 缓存恢复 | 编译 | 测试执行 | 残差 |
| --- | --- | --- | --- | --- | --- | --- |
| 34747049405 | 267s | 19s | 34s | 91s | 115s | 8s |
| 34747417399 | 280s | 22s | 30s | 101s | 121s | 6s |
| 34747480095 | 266s | 20s | 30s | 93s | 118s | 5s |
| 34747583723 | 271s | 20s | 31s | 98s | 117s | 5s |
| 34748550902 | 306s | 28s | 35s | 116s | 118s | 9s |
| 34748847869 | 258s | 31s | 34s | 82s | 102s | 9s |
| 34749024124 | 291s | 28s | 42s | 95s | 119s | 7s |

- 端到端 258–306s，典型（中位）271s；apt 阶段已归零（#1109），其成本并入容器初始化。
- 残差 = 端到端 − 四段之和，5–9s，内容为 setup/checkout/Post rust-cache/stop
  containers 等固定开销（**不属于**任何可优化段）。
- 编译段口径：cargo 自报 `Finished \`test\` profile … in 1m22s–1m56s`（从 cargo 步骤
  起点到该行）。
- 执行段口径：首个测试二进制启动到 e2e 汇总行（单次拆解 run `34749230834`）：lib 单测
  1229 用例分 17 个二进制（~51s）、api_server 236 用例 11s、commands 19 用例 6s、
  e2e 451 场景 3122 步 ~53s、sync_trigger_poll 1 用例 ~0s。

### 其余 job（同日同批）

| job | 端到端（同批 7 次运行） |
| --- | --- |
| frontend-test（shard 1/2） | 126–189s |
| frontend-test（shard 2/2） | 112–175s |
| frontend（类型检查 + lint + 守门脚本） | 39–87s |
| backend-lint（fmt + clippy + rustdoc 门禁） | 81–96s |

关键路径因此从「前端测试」转移到后端测试 job：7 次运行的 job 跨度 260–308s
（≈ 4.3–5.1 分钟，不含 GitHub 排队；同批另有 1 次因排队使工作流 created→updated
达 372s）。

## 四、250s 目标与「180s 不可达」

乐观下界（四段各取基线最小观测值，再加残差）：19（容器初始化）+ 30（缓存恢复）
+ 82（编译）+ 102（执行）= 233s，加 6–9s 残差 ≈ 239–242s。

- **180s 不可达**：下界 ≈ 240s 已高于 180s 达 60s；四条约束各自锁死一段——②固定执行段
  （不能按路径过滤、e2e 不挪 nightly，时间就是用例本身），④固定固定开销（拆 job 会
  重复付容器与缓存恢复，更慢），①③固定编译段（4 核 runner 的 codegen+链接下界；
  编译侧唯一数量级工具 kache 在 CI 侧已被明令否决）。结论写回 CI 注释止损。
- **250s 只在各段同时接近最小观测值时成立**：7 次运行中仅 1 次达 258s，其余 266–306s，
  故 250s 是乐观下界而非稳态目标；本票后预期落在 250–300s 区间（编译段 −10~20s、
  其余不变）。

## 五、未验证项与局限

- **本票的稳态收益仍未在本票 CI 运行中验证**：本 PR 的运行（run `34751213388`）因
  env hash 变化走了全量冷编（见第一节「代价二」），其余四个 job 全绿、后端测试 job
  通过（三层测试与静态检查结果不变，AC5 的运行证据）。稳态（debug=0 下缓存恢复命中、
  成员重编）要等 merge 后首个 main 运行以新 key 保存缓存。回填方式：读该 main run 与
  其后任一次 PR 运行的编译段耗时、Post 缓存大小，与本文件第三节基线对比。预期：
  编译段小幅下降，缓存体积不变。
- **本机测量 ≠ CI 条件**：macOS arm64 + kache（CI 为 Linux x64 + rust-cache + mold），
  并发负载 load ≈ 20/12 核使 wall 不可比；只把 CPU 时间与体积对照当证据，未外推秒数。
- 镜像体积与容器初始化时间（19–31s）未优化：其归属是 #1109 的镜像构建议题，不在本票
  范围。
- e2e 单场景耗时异常（spec #1086 Further Notes）不在本票范围，未做诊断。
