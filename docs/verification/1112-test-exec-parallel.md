# 测试执行按包并发（issue #1112 / 父 spec #1086）

> workspace 拆成多 crate 后，`cargo test --workspace` 顺序启动每个测试二进制，
> 每个二进制各自按 CPU 数开满 libtest 线程。本票把「单测 + 集成测试」的**执行**
> 改为「构建一次 + 统一执行器调度全二进制并发」，e2e 与 doc-test 留在 cargo 自有
> 入口不动；两条入口的覆盖范围由守门脚本锁死。本文件记录负向验收、同机改前/改后
> 耗时与「是否继续投入含 e2e 同跑」的结论。
>
> **数字单点**：目标清单的数量与种类以 `bun scripts/test-exec.ts plan` 的输出为唯一
> 权威（下文只在「执行面与并行度口径」一处复述并标注复述时点）；本文件其余处的
> 「23 个二进制」等字样是**该时点的实测快照**（耗时表/对拍表的行标签），清单漂移时
> 无需逐处改数字，以 `plan` 为准。

## 交付物

- `scripts/test-exec.ts`（新）：并发执行器 + 两入口覆盖守门。`run` 命令先跑守门，
  再以一条 `cargo test --workspace --no-run --message-format=json-render-diagnostics`
  构建一次，解析 cargo 报告的测试二进制清单，最后由执行器统一调度：全局并行度
  = min(CPU 数, 待跑二进制数)，每个二进制固定 `RUST_TEST_THREADS=1`（只约束
  libtest 线程）——**libtest 线程数 = 并行度**，不存在「各二进制各自开满 libtest
  线程」的 CPU 超订；测试自起的 tokio 等运行时不在控制面（属经验证据，非守门）。
  失败聚合报告，跑完全部才退出。
- `scripts/test.sh`（改）：两条入口一次跑完 workspace 全部成员的测试面——并发
  入口（执行器）+ 非并发入口（`cargo test --workspace --test e2e`、
  `cargo test --workspace --doc`）。e2e 仍由 cargo 驱动，形态与拆 workspace 前
  一致；新增 doc-test 入口是**补齐**（拆分前 `cargo test --workspace` 本就跑
  doc-test，本仓现有 `compile_fail` / `ignore` 两个文档块）。
- `scripts/check.sh` + `.github/workflows/build.yml`（改）：覆盖守门挂入本地门槛
  与 CI frontend job（纯静态发现，不需 Rust 工具链）。
- `scripts/test-exec.test.ts`（新）：夹具负向用例（守门规则逐条「制造变红」）+
  接线用例（删任一入口调用/门禁接线即红）。断言观察面统一是 `check` 的**退出码与
  输出**，不钉命令字符串形态（ADR-0087 断言强度）——`scripts/check.sh` 与 CI 的
  门禁接线在单元测试里不会被管线执行，按 AGENTS.md 以源码扫描守门替代（先例
  #959/#961），落在 `test-exec.ts` 的守门规则④内。

## 执行面与并行度口径

- 目标清单口径 = `cargo test` 默认执行面：lib 单测 + bin 单测 + 集成测试 +
  doc-test。example / bench 不在默认执行面（cargo 只构建 example、不跑 bench），
  不入清单但按下述守门规则显式提示。
- 并发入口承接：lib 单测 + bin 单测 + `harness = true` 的集成测试。
  非并发入口承接：`[[test]] harness = false` 的自定义 runner（当前 = e2e）+
  doc-test（测试二进制由 rustdoc 生成，不进 cargo 的 test artifact 报告）。
- **当前清单（唯一权威 = `bun scripts/test-exec.ts plan` 输出）**：41 个目标 =
  并发入口 23（lib 单测 17 + bin 单测 2 + 集成测试 4）⊎ 非并发入口 18（e2e 1 +
  doc-test 17）。上面这行是本文件里唯一复述清单数字的地方（#1112 审查发现：同一批
  数字曾散住脚本头/文档/提交信息多处并实际漂移）。
- 并行度上限默认取 CPU 数（本机 12）；`--jobs N` 是显式覆盖，N 超过 CPU 数时
  执行器会打印「显式超订」提示（默认路径不会超订）。

## 负向验收（删除即变红）

守门五条（`bun scripts/test-exec.ts check`，挂在 `scripts/check.sh` 与 CI
frontend job；夹具用例见 `scripts/test-exec.test.ts`）：

1. **覆盖全集（规则①）**：工作区全部测试目标 = 并发入口承接 ⊎ 非并发入口承接，
   互斥且无遗漏；空集一律拒绝假绿。目标清单自 manifest + 目录自动发现派生，新增
   target / 新成员 crate 自动入列。
2. **自定义 harness 登记（规则②）**：`[[test]] harness = false` 的声明集合必须与
   `scripts/test.sh` 非并发入口的 `--test <name>` 名单**双向全等**——新增目标未
   登记、登记失效或拼写漂移都红。夹具用例
   `新增 harness=false 测试目标未登记非并发入口 → 红` 断言的输出含新目标名与
   「静默漏跑」；本仓实测见下。
3. **doc 入口与并发入口接线（规则③）**：存在 doc-test 目标 ⇔ `scripts/test.sh` 有
   `cargo test --workspace --doc`（缺 `--doc`、无目标却留 `--doc` 都红）；
   `--doc` 存在但缺 workspace 范围（`--workspace` 或词尾 `--all`）→ 红——非虚拟
   workspace 下 `cargo test --doc` 只跑根包 doctest，成员 crate 的 doc-test 静默
   漏跑而守门仍绿（#1112 审查补强）；`scripts/test.sh` 未调用 `scripts/test-exec.ts`
   → 红。
4. **门禁自身接线（规则④，#1112 审查补强）**：`scripts/check.sh` 与 CI frontend
   job 必须调用 `check`，否则 → 红（接线在管线里、不由单元测试构建，按先例
   #959/#961 以源码扫描守门）。
5. **执行器等价性（规则⑤，#1112 审查补强）**：测试运行期读 cargo 注入环境变量
   （`env::var("CARGO_*"/"OUT_DIR")`；编译期 `env!` 与 `build.rs` 的构建期读取不受
   影响）→ 红——执行器只对齐 cwd 与 `RUST_TEST_THREADS`，不复制 cargo 运行期环境。

**本仓实测（制造变红 → 恢复 → 变绿；均在真实仓库文件上做，`bun scripts/test-exec.ts
check` 的退出码为观察面）**：

| 步骤 | 操作 | 结果 |
| --- | --- | --- |
| 制造变红 | 新增 `src-tauri/tests/parallel_probe.rs` 并在 `src-tauri/Cargo.toml` 声明 `[[test]] name = "parallel_probe" harness = false` | `bun scripts/test-exec.ts check` 退出码 1，输出点名 `tauri-app::parallel_probe` 与「静默漏跑」 |
| 恢复 | 删除该声明与文件 | `bun scripts/test-exec.ts check` 退出码 0 |
| 制造变红 | 删 `scripts/test.sh` 的 `bun scripts/test-exec.ts` 行 | 退出码 1，输出「未调用并发入口（scripts/test-exec.ts）」 |
| 恢复 | 加回该行 | 退出码 0 |
| 制造变红 | 删 `scripts/test.sh` 的 `( cd src-tauri && cargo test --workspace --test e2e )` 行 | 退出码 1，输出点名 `tauri-app::e2e` 与「静默漏跑」 |
| 恢复 | 加回该行 | 退出码 0 |
| 制造变红 | `scripts/test.sh` 的 doc 行退化成 `cargo test --doc` | 退出码 1，输出「`--doc` 缺 workspace 范围」 |
| 恢复 | 改回 `cargo test --workspace --doc` | 退出码 0 |
| 制造变红 | `scripts/check.sh` 的守门行换成 `bun scripts/test-exec.ts plan` | 退出码 1，输出「scripts/check.sh 质量门槛序列（scripts/check.sh）未调用守门命令」 |
| 恢复 | 改回 `check` | 退出码 0 |
| 制造变红 | CI `run: bun scripts/test-exec.ts check` 换成 `run: echo "…"` | 退出码 1，输出「CI frontend job（.github/workflows/build.yml）未调用守门命令」 |
| 恢复 | 改回 `check` | 退出码 0 |

> 自证教训：`check.sh` 的改坏第一次**未被发现**——该脚本的 `echo "▶ 测试执行覆盖守门
> (bun scripts/test-exec.ts check)"` 标签行含同样词序，被源码扫描误当接线（假绿）。
> 接线判定随之收紧为「`bun` 必须落在命令位置（剥掉 YAML / 子 shell 装饰后的首个
> 词）」，并补「只剩 echo 标签行 → 红」「接线带引号/前置参数 → 不假红」两条夹具用例。

夹具侧的规则矩阵住 `scripts/test-exec.test.ts` 的第一个 describe（17 条），另加
接线 describe 2 条（真实仓库通过 + 删 check.sh 接线即红），`pnpm exec vitest run
scripts/test-exec.test.ts` **19/19 全绿**；其中「删 `--test e2e`」「删 `--doc`」
「doc 缺 workspace 范围」「删并发入口调用」「`--test` 漂移」「auto* 开关」
「未支持 members glob」「check.sh / CI 接线缺失或只剩 echo 标签」「运行期读 cargo
注入环境」各自对准一条可观察的失败输出。

全量质量门槛：`./scripts/check.sh` **退出码 0**（前端类型检查 + oxlint + clippy
`--all-targets --all-features -D warnings` + fmt + rustdoc 门禁 + 全部守门脚本，
其中含本次新增的「测试执行覆盖守门」步骤）；测试面证据见下段。

`run` 命令另有第六道交叉核对：`cargo test --workspace --no-run` 实际构建出的测试二进制集合
必须与静态发现的目标清单全等——发现逻辑与 cargo 真实行为漂移即红，不给
「清单看着对、实际漏跑」留口子。

**Review 补强（两轴审查发现，均带实测证据）**：

- 执行面核对键必须含目标**种类**：cargo 允许同一包内 lib 与集成 target 同名
  （`src/lib.rs` 与 `tests/<同名>.rs`），只用「包::名」会让两侧集合同时撞键、
  交叉核对双双假绿。实测（临时把核对键退回「包::名」、并在 `ledger-dashboard`
  下加一个与 lib target 同名的集成 target）：清单 42 项、执行器只调度 **23** 个
  二进制却报 ✅ 全绿（漏跑 1 个）；带 kind 的键下同一场景 24/24 全绿、两个同名
  目标各自执行。夹具侧另有「同包同名目标都入清单」的回归用例。
- 测试进程 cwd 与 cargo 对齐到**各包根**（不是 workspace 根）：cargo 跑测试
  二进制时的 cwd 是包根，成员 crate 的测试若用相对路径，直接以 workspace 根
  为 cwd 会读到不同位置（等价性缺口）。执行器现按目标所属包根 spawn。

**Review 补强（独立审查第二轮，Standards / Spec 两轴）**：

- **验证文档计数失真**：原写「规则矩阵 11 条、12/12 全绿」，实际首个 describe 17
  条、加接线 describe 2 条共 19 条（`pnpm exec vitest run scripts/test-exec.test.ts`
  19/19）——已按实跑改写（本文上节）。
- **接线断言钉命令行形态**：原用逐字正则锁 `test.sh` / `check.sh` / `build.yml` 的
  命令行（加 `--locked`、换引号即假红），违反 ADR-0087「断言对准可观察结果、不钉
  实现形状」。现改为：`test.sh` 三入口由守门读真实文件后的**退出码/输出**断言
  （夹具逐条制造变红），`check.sh` 与 CI 接线并入守门规则 4 / 5 → 断言面统一为
  `check` 的退出码；接线删除即红。接线扫描按 shell 词切分并要求 `bun` 落在命令
  位置——加引号、多空白、前置参数不假红；纯粹的说明文字（含 echo 标签行）不算接线。
- **「总线程数 = 并行度、无 CPU 超订」措辞过强**：`RUST_TEST_THREADS=1` 只约束
  libtest 线程，测试自起的 tokio 运行时不受控（`api_server/router.rs` 的
  `Runtime::new()` 等）。代码注释、脚本头与本文统一改为「**libtest 线程数 =
  并行度**」，并发阶段跑绿属经验证据而非控制面承诺。
- **新 cargo 命令宿主未登记**：`scripts/test-exec.ts` 程序化拼装 `cargo test
  --workspace …`，但不在 `scripts/check-structure.ts` 的 `WORKSPACE_COMMAND_FILES`
  内 → `--workspace` 元门禁静态不覆盖。已登记，并让该门禁认 `.ts` 的 `//` /
  `/** … */` 注释行（注释里的 `cargo test` 是说明文字，不算命令面）；夹具侧补
  「test-exec.ts 缺 `--workspace` → 红」与「注释里的 `cargo test` 不假红」两例。
- **数字重复五处**：41/23/18 与并行度口径原同住脚本头、文档、提交信息多处，且已
  实际漂移（12/12 vs 13/13）。现收敛为「唯一权威 = `bun scripts/test-exec.ts
  plan` 输出」，脚本头不再复述数量，文档只在「执行面与并行度口径」一处复述并标注
  时点（见文首「数字单点」）。
- **执行器绕过 cargo 运行期注入环境**：处置取「显式守门」而非「复制环境」——
  cargo 注入面（`CARGO_MANIFEST_DIR` / `CARGO_PKG_*` / `OUT_DIR` / `CARGO_BIN_EXE_*` /
  动态库搜索路径）无法有界复制，只补其中几个会给「已等价」的假信心。改为：执行器
  只对齐 cwd 与 `RUST_TEST_THREADS`，测试运行期读这些变量即 fail loud（守门规则⑤），
  由引入者显式扩展执行器并更新守门。扫描复用结构守门的 Rust 词法掩码（注释掩去、
  字面量保留），`build.rs` 的构建期读取与编译期 `env!` 不受影响；夹具覆盖「命中即红」
  与「注释里提到不假红」两侧。

## 实测耗时（同机、同 commit 基线、缓存状态）

口径：本机 macOS / arm64 / 12 CPU；cargo 1.98 + kache（热回路增量已命中），
**依赖与 workspace 全热**（`cargo test --no-run` 空跑 ≈ 0.4s）；同一 commit
基线（`origin/main` 8d987284 + 本票改动，e2e 断言面零改）。CI runner 数据未采集
（本票不改 CI 的测试执行面，见「未验证项」）。

**改前**（`cd src-tauri && cargo test --workspace`，即旧 `scripts/test.sh` 入口）：

| 轮次 | 墙钟 | 23 个 libtest 二进制耗时之和 | e2e | doc-test |
| --- | --- | --- | --- | --- |
| 第 1 轮 | 145.48s | 81.23s | ≈63.5s | 0.27s |
| 第 2 轮 | 120.88s | 88.44s | ≈31.7s | 0.27s |

**改后**（`./scripts/test.sh`，两条入口）：墙钟 **56.61s**，分解为

| 阶段 | 耗时 | 说明 |
| --- | --- | --- |
| 构建一次（`cargo test --workspace --no-run`） | 0.46s | 缓存全热；冷构建的编译成本与旧入口相同（同一条 cargo 构建） |
| 并发执行 23 个二进制 | 13.60s | 各二进制耗时之和 73.03s，加速 5.37x；最长 `tauri-app::tauri_app_lib` 12.58s |
| e2e（`cargo test --workspace --test e2e`） | ≈42s | 453 场景 3139 步全绿；单独跑实测 35.01s（负载敏感，见下） |
| doc-test（`cargo test --workspace --doc`） | ≈0.5s | 17 个 doc 目标 |

并发阶段另跑 6 轮复测（只跑入口①）：墙钟 10.87 / 12.82 / 13.21 / 13.52 / 16.44
/ 20.00s，各二进制耗时之和 63.7–95.8s——**23/23 二进制六轮全绿**，无 CPU 超订
导致的失败。结论：执行阶段（不含 e2e）从 81–88s 降到 11–20s；两入口合计
145.5 / 120.9s → 56.6s（2.1–2.6x）。e2e 与并发入口的负载敏感度是本机（12 CPU、
多 worktree 并行开发）观测，波动大，故取区间而非单值。

端到端复测（含 review 补强后的最终形态）另有 44.31s 与 38.92s 两轮：并发阶段
11.3s / 9.8s（各二进制耗时之和 60.1s / 52.8s，加速 5.3x / 5.4x），e2e 落到
~28s——再次印证表内的 e2e 波动幅度。

## 结论：是否值得做「构建一次 + 全二进制并发（含 e2e 同跑）」

**值得继续投入**，但建议另立票，且先做资源冲突实测：

- 数据：改后总时长 56.6s 中 e2e 占 ≈42s（≈74%），已是唯一大头。把 e2e 纳入并发
  池（与其余二进制同跑）后，总时长可望落到 ≈15–25s 量级——即本票拿到的 5x 执行
  加速可以覆盖到全量，而不是只覆盖 23 个小二进制。
- 风险与代价：e2e 是 cucumber 自有 runner（`harness = false`），当前执行器只按
  libtest 二进制调度；同跑需要（a）cucumber 二进制的并发调度接缝，（b）实测它与
  其余二进制的资源冲突（单机 SQLite 临时库 / 文件系统 / 内存 / 端口）——本票的
  6 轮复测显示 23 个 libtest 二进制同跑无冲突，e2e 的负载画像不同，需单独证据。
- 反向证据：e2e 单跑在 31.7–63.5s 间波动（同一机器、同一二进制），说明它本身对
  机器负载敏感；把它塞进满并行度池子可能只是把等待挪个位置，需以「e2e 同跑 vs
  串行」的对照实验定案。

## 既有三层测试结果不变（改前 ↔ 改后逐项对拍）

同一 commit 基线（e2e 断言面与用例数零改）、同一机器，改前 = `cargo test
--workspace`，改后 = `./scripts/test.sh`：

| 层 | 改前 | 改后 |
| --- | --- | --- |
| 域/壳单测 + 集成测试（23 个 libtest 二进制） | 1521 passed / 0 failed / 4 ignored | 1521 passed / 0 failed / 4 ignored |
| e2e BDD（cucumber） | 453 scenarios（453 passed）/ 3139 steps（3139 passed） | 453 scenarios（453 passed）/ 3139 steps（3139 passed） |
| doc-test（17 个目标） | 4 passed / 1 ignored | 4 passed / 1 ignored |

并发入口 6 轮复测均为 23/23 二进制通过、1521 passed / 0 failed / 4 ignored；
`./scripts/test.sh` 端到端退出码 0。

## 范围外修复（单独提交）

> **与父 spec 的显式例外（需约束方确认）**：父 spec #1086 字面规定「既有测试不得因
> 拆分而改动断言（断言与场景文本保持原样）」。下面两处确实改了既有断言，属**例外**，
> 单独成一个提交（`dee66f6e`）并写明根因，理由三条：
> ① 根因正当——断言把「创建序 == id 序」当既定事实，而 UUIDv7 只在毫秒内有序，
> 是既有时序脆弱（并发执行把偶发变高频，不是拆分引入的语义变化）；
> ② 断言未弱化——仍锚定同一域语义（A 流水位点；每批消耗份额与结转成本），只是把
> 两侧按同一键归一后比较，换批次序会改数值；
> ③ 范围外分流纪律——真实缺陷（用户可见的随机假红）直接修、单独提交、留痕，不夹带
> 重构。已在 PR #1265 与 issue #1112 各留一条说明供 spec 维护者确认；**未**改 spec
> 正文、未关闭 issue。

并发执行（每个二进制 `RUST_TEST_THREADS=1`）把两个**既有**时序脆弱用例从偶发
变成约 15–25% 必现，证据与根因如下（修复前后均为独立实测）：

1. `src-tauri/src/sync_engine/tests/checkpoint.rs::positions_pinned_below_parked_op`
   以 `stream_positions(&conn_b)[0]` 取「A 流位点」。B 端本机 op 也会推进自己流
   的位点（`crates/sync-protocol/src/position.rs` 模块文档），故位点清单是**两行**
   多流集合，且按 `ORDER BY device_id ASC` 排序；两个 UUIDv7 同一毫秒生成时顺序
   由随机位决定 → 断言随机红。实测（修复前，单测循环）**22/150 失败**；按
   device_id 定位后 **0/300 失败**（同组 4 个用例合并计数）。

2. `src-tauri/src/sync_engine/tests/convert.rs::convert_create_replay_converges_lots_carried_cost_and_pnl`
   以创建顺序构造 `expected_conversions`，而读侧 `ORDER BY l.buy_transaction_id`
   ——同样是「UUIDv7 同毫秒不定序」的假设。实测修复前 **25/100 失败**，两侧按
   同一键归一后 **0/100**（断言仍比较每批的份额与结转成本，换批次序会改数值，
   断言强度不变）。

同一根因的其余命中已核对：`checkpoint.rs` 其余 `[0]` 用例中，`cp.positions[0]`
所在用例先断言 `len() == 1`、`channel.rs` 的检查点来自单流库——均为单行清单，
无漂移面；`stream_positions` 的其他调用点已按 device_id 或 a/b 对拍比较。

## 未验证项

- **CI runner 上的收益未实测**：本票不改 CI 的测试执行面（`cargo test --workspace
  --lib --test '*'` 原样保留：CI 刻意不含 bin 单测与 doc-test，先例 #988），
  故表内数据全部为**本机口径**。把 CI backend job 切到 `scripts/test.sh` 属另一
  决策（会把 bin 单测与 doc-test 带回 CI、且与 #1110 的缓存/编译调优同文件），
  本票只留数据与结论。
- e2e 纳入并发池的收益/风险（见结论段）未做对照实验。
- Windows / Linux 上的执行器行为未验证（本机 macOS）。
