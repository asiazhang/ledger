# ADR-0145: 代码注释禁令与棘轮清零——注释只许删不许加，注记住址改 ADR 本体与守门脚本数据面

- 状态：已接受（spec #2202 定稿，2026-11-21；治理批次随 #2203 同批落地门禁、初始基线与配套治理文档）
- 日期：2026-11-21
- 作者：Ledger 项目
- 关联：ADR-0113 决策 5（「注记归 crate `//!` 与模块文件头」条款随本决策废止，见其修订注记）；ADR-0112 / ADR-0056（守门家族与 crate 政策核）；ADR-0083（守门脚本 Bun 运行时与测试口径）；ADR-0087（删除即变红断言强度）；#2202（spec）；#2203（治理批次实施）；Excuse 跟踪 issue #2213

## 背景

开发模式已转向 agent 主导，注释的两个系统性问题被放大：

- **过时与不一致**：注释是第二事实源，改代码不同步注释时，漂移注释反过来误导后续 agent 与人类评审者；没有守门机制阻止注释净增长。全仓存量注释约 5.2 万行（落地时点基线计数，下同），占代码量约 16%。
- **政策数据错位**：仓库治理此前把守门「注记」安在 crate 注释与模块文件头（ADR-0113 决策 5 的 #1591/#1864 定案），机器不消费、agent 却会当真源引用——注释即政策的错位。

同时，注释里混着两类真实知识没有权威住址：unsafe 块的正确性前提（`// SAFETY:` 形态）、依赖钉住与配置口径的理由（package.json `"//"` 数据键、Cargo.toml 配置注释）。删除注释不能让这些信息蒸发。

## 决策

### 1. 禁令辖域：全部代码注释

Rust 全形态（`///`、`//!`、普通 `//`、块注释）；TS/Vue/JS 行注释与块注释（含 JSDoc）；shell 行首 `#` 注释；配置文件注释——Cargo.toml 与 `.kache.toml` 的 `#` 行、package.json 的 `"//"` 数据键。markdown（docs/、词汇表、ADR、skill）不受禁令辖域。

### 2. 豁免闭集

- **`// SAFETY:` 形态**：行 trim 后以 `//` 起头（可带 `/` 或 `!` 成文档形态）且紧跟 `SAFETY` 加冒号、全角冒号或括注——unsafe 块的不变量载荷（哪个字段为空是契约、哪个生命周期是前提），删除即丢正确性前提，是 Rust 惯例语义而非漂移面。SAFETY 注释块只有首行命中豁免形态，续行照常计数。
- **shebang 行**：语义行非注释（shell 首行 `#!`）；TS/Rust 面的 shebang 与内部属性不构成掩码差异，天然不计数。
- 扩豁免 = 修订本 ADR，不得在守门脚本或评审中临时扩。

### 3. 注释识别：行掩码口径，复用守门家族共享掩码原语

- Rust 经 `maskNonCode`（keepLiterals=true 只掩注释保留字面量）、TS/Vue 经 `maskComments`（掩注释跳过字面量）——两者住守门家族共享原语库（ADR-0111 / #1433 双源登记的 TS 侧载体）；行掩码口径 = 掩码后与原文逐行对比，掩码处原行非空白即注释行，整行注释与行尾注释各计一行，字符串字面量里的注释字样不误报。
- shell / TOML / package.json 无共享掩码原语：单消费方不满足原语 ≥2 门准入判据（#1680），识别口径是本门政策、留门内——shell 与 TOML 计行首注释行（trim 后以 `#` 起头；shell 首行 shebang 豁免；行尾 `#` 不计数，字符串内 `#` 无法廉价区分，确定性优先，靠评审兜底）；package.json 计行形态 `"//":`（值折行形态靠评审兜底）。
- 掩码规则本身改动仍守家族双源同步纪律；门内自持口径改动 = 修订本 ADR。

### 4. 棘轮基线：按目录计数、只降不升、归零即零容忍

- 基线数据面 = JSON 文件（`scripts/fixtures/comment-baseline.json`，守门语料夹具住址约定）：目录键 → 注释行计数，键集与扫描键集双向全等，缺键、冗余键、负数、非整数即红（schema 校验住门与测试）。
- 目录键集：文件键（`.kache.toml`、`package.json`、根散配置 `resume-dev.sh`、`vite.config.ts`、`vitest.config.ts`、`vitest.bench.config.ts`）+ 固定目录键（`src`、`scripts`、`src-tauri`、`src-tauri/src`、`src-tauri/tests`）+ 投影目录键（`src-tauri/crates/<crate>`、`packages/<pkg>`，按目录列举投影）。扫描文件落不进任何键即红——新增 crate / 包 / 根文件须显式登记基线（fail loud，不静默放行，与守门家族清单纪律同构）。
- 基线值 = 本 ADR 落地时点各键现状计数；门提供基线再生成（`bun scripts/check-comment-gate.ts --write-baseline`）。实际计数 > 基线即红；基线只许随清理下调，不许上调（上调 = 修订本 ADR）；基线为 0 的目录零容忍，新增一行注释即红。
- 与 ADR-0113 #1864「校准向前生效」同构：存量注释不批量回改，基线锁定现状，清理分批推进、每批 PR 基线单调下降，任何一批回退不丢其他批次成果。

### 5. 扫描边界

扩展名闭集：`.rs`、`.ts`/`.vue`/`.js`、`.sh`、`.toml`（仅文件名 `Cargo.toml` 与 `.kache.toml`）、`.json`（仅文件名 `package.json`）；其他 JS 方言扩展（`.mjs` 等）与 Vue 模板 HTML 注释不入闭集，靠评审兜底。目录剪枝闭集：`node_modules`、`target`、`dist`、`.git`；`scripts/fixtures/` 整目录豁免（掩码共享语料与门自用数据面，非受守代码）。gate 自测夹具住临时目录，构造上不入仓库扫描。

### 6. 存量处置：四分类法（判定优先级：决策记录 > 豁口辩护 > 失准 > 同义复述）

- **决策记录（Decision Record）**：承载代码查不到的真实决策、理由或口径（依赖钉住、口径裁定）。逐条核对对应 ADR：已含该信息则直接删，未含则先并入 ADR 再删；crate 注释里的独有政策并入对应域 ADR，配置类钉住理由迁依赖升级文档与守门数据面。信息只搬家、不蒸发。
- **豁口辩护（Excuse）**：为 hacky 实现辩护（「这里不能改因为 X」）。登记于 Excuse 跟踪 issue #2213（每条 = 位置 + 豁口 + 修复票）后删注释；豁口修复独立成票，不与清理批次捆绑。
- **失准（Stale）**：与代码行为不一致。以代码为准直接删；若注释描述的「应有行为」实为需求缺口，按范围外纪律报告，不静默修。
- **同义复述（Tautology）**：复述代码已表达的内容。直接删。
- 一条注释命中多类时按优先级归类处置；流程载体是 `/delete-comment` skill（`.agents/skills/delete-comment/`），分类术语收进测试基础设施词汇表。

### 7. 注记住址改址

注记（职责定位、不变量与口径、ADR 与票号指针、陷阱与边界）的新住址 = **ADR 本体 + 守门脚本数据面**（登记条目的理由与出处字段、政策核清单逐条 why）。ADR-0113 决策 5「文件头与 crate 级 `//!` 是注记的唯一作者位」随之废止（见其修订注记）；AGENTS.md 门禁保底段同步改址。删除注释后的信息缺口由 ADR、git history 与类型/结构表达承接——代码自解释性是硬约束，不是口号。

## 理由 / 备选与否决

- 为什么禁而不劝：评审劝退不可规模化，agent 顺手留注释无人拦；守门把「注释净增长」变成显式失败（spec #2202 用户拍板）。
- 为什么棘轮而非一步清零：5 万行级存量逐条分类无法一次完成；按目录基线让治理先行、清理分批，且成果单调累积。
- **被排除路线（防重提）**：
  - 借道 oxlint / clippy 现有 linter——禁令政策（豁免闭集、棘轮、按目录基线）与 linter 规则模型不合，且 TOML / package.json 面不被覆盖；本仓守门家族自持政策是先例（check-infra-dml 等）。
  - clippy `missing_docs` 反向强制文档注释——与禁令方向相反，文档注释同样是会漂移的注释（spec 事实备查：clippy 门禁不含 missing_docs，删文档注释不破编译）。
  - per-file 基线——数千文件键不可维护；目录键与 crate / 包结构对齐（ADR-0112 / ADR-0118），清理批次天然按域推进。
  - 先清后禁——治理被存量清理阻塞数月，注释净增长窗口继续开着。

## 后果 / 代价与边界

- 门禁落地即全绿（基线 = 现状），全仓行为零变化；任何目录净增注释立即红。
- 存量清理是长期批次工作：豁口辩护类每条对应一个待修实现（跟踪 issue #2213 承载），修复优先级由该 issue 管理而非清理批次。
- 清理期存在过渡态：存量脚本头部注释仍是过渡期读物（AGENTS.md 触发条款已注明过渡语义），新脚本一律零注释，规则出处查守门挂载登记与 ADR。
- 基线文件是治理批次唯一新增数据面；除 comment gate 与 skill 外不新增任何模块。

## 后续票

- 守门数据迁移批次：crate 注释独有政策并入 ADR、package.json `"//"` 内容迁依赖升级文档、Cargo.toml 守门说明并入 ADR（spec #2202 批次 2）。
- 分域清理批次：19 个域 crate 与前端逐域推进，每域一个 checklist 勾选项，豁口辩护记入 #2213，基线随 PR 单调下降（spec #2202 批次 3）。
- 归零批次：基线清零，门禁全面零容忍（spec #2202 批次 4）。

## 修订注记（#2205 清零，2026-11-21）：配置面与 shell 面注释清零——独有决策记录在本注记收口

package.json `"//"` 键（根 + 18 包）、Cargo.toml 全部注释、`.kache.toml`、resume-dev.sh 与 scripts/*.sh 注释按四分类清零（shebang 豁免）。多数条目已有住址直接删：同义复述类（clippy 六件套继承声明住 AGENTS.md「门禁保底」；dev-dependency 测试专用环与 compile_fail 击穿机理住 ADR-0084 #1111 修订注记；各 crate 依赖面定位与依赖声明、check-structure.ts 守门及根 Cargo.toml 同文重复；workspace 骨架与协议 crate 定位住 AGENTS.md「后端分层」「协议」）；决策记录类（加密引擎与生物门住 ADR-0075；行情 HTTP 面与异步 DB 门面住 ADR-0125；窗口状态插件桌面化住 ADR-0074 决策 6；测试器具可见性与 HTTP 投影门住 ADR-0111 决策 5；归档 op 载荷住 ADR-0142 决策 5；行菜单事件纪律住 ADR-0077 决策 4；ECB 汇率源住 ADR-0019 修订注记；mock 后端住 ADR-0143；bin 测试面与脚本测试运行口径住 ADR-0083 修订注记与 test-exec.ts / gate-mounts.ts 数据面）。package.json `"//"` 键的住址改迁见依赖升级文档（TypeScript 6.x hold）与 ADR-0118 修订注记（包定位）。以下独有决策记录无既有住址，在本注记收口（信息不蒸发）：

- **备份版本锁步**（backup crate）：`after_commit_hook` 以编译期 `env!("CARGO_PKG_VERSION")` 落备份元数据的 app_version 字段——workspace 成员升版必须同步，否则备份元数据版本失真。
- **默认 bin 指向**（根包）：包内第二个 bin（ledger-perf，ADR-0062）出现后显式 `default-run` 指向主程序，保证 `cargo run` 与 tauri dev 不改行为。
- **bin 单测排除**（根包，#1787 热点 2）：main.rs 薄入口（逻辑住 lib 与各域 crate）的 bin 目标声明 `test = false` 免链免跑；bin 内新增 `#[test]` 即被 scripts/test-exec.ts 的 test = false 源码扫描守门打红；ledger-perf 保留单测（EXEMPT_BINS 登记），归 perf-bench.yml 每日基准承接。
- **updater 刻意 default-features = false**（根包，ADR-0124 伴随裁决）：default 含 system-proxy，会把整个二进制的 reqwest 编译面翻成「读系统代理」——本机实证 hyper-util 的 macOS 实现只读代理地址不读例外表，行情域 fx 同步（ADR-0125 决策 5 直连语义）对 127.0.0.1 的请求被劫持进本机代理，既有测试 fx_sync_classifies_unreachable_source 当场变红。保留 rustls-tls（GitHub 端点 HTTPS）与 zip（macOS .app.tar.gz 解包即在此 feature 内，命名有误导）；更新检查可达性按 ADR-0124 决策 2 端点数组扩展位，环境变量代理 reqwest 默认仍生效。
- **dev 构建去符号**（根包 `[profile.dev.package."*"]` `debug = false`）：依赖 crate 的 dev 构建关闭调试信息——产物不携带 DWARF/OSO 引用，规避链接器（Xcode 26 ld）对 kache 共享 store 还原产物追溯原始编译路径时的 unable to open object file 告警；业务 crate 不受影响、断点调试不变，依赖栈帧回溯仍显示函数名（无行号），需要依赖完整符号时临时还原本段。
- **release 产物瘦身**（根包 `[profile.release]`，#1999）：strip + LTO + codegen-units = 1——v0.11.1 Android APK 60.8 MiB 中 22.3 MiB 是 .strtab/.symtab 调试符号，LTO + 单 codegen-unit 再裁跨 crate 死代码（预期 60.8 → ~30 MiB，代价是 CI release 编译变慢）。刻意不设两项：panic = "abort"（可再省 ~5.9 MiB unwind 表，否决——生产代码依赖 catch_unwind 保护读事务闭包，abort 改变 panic 语义）；opt-level 维持默认 3（"s" 是性能换体积的真实取舍，以 perf-bench 数据决策，#2000）。
- **infra 依赖留守**：tracing-subscriber 留 infra 常规依赖——test_utils（信号捕获 Layer）随 feature 编入 lib、对下游测试构建可见，dev-dependencies 不进下游闭包。
- **行情编码**（market-sync）：腾讯行情报价为 GBK 编码（ADR-0130 源决策的落地细节），encoding_rs 标准 WHATWG 实现，解码出错 fail-closed，不把替换字符混进权威名称字段。
- **kache 配置机理**（.kache.toml）：kache 默认在缓存路径剥掉 `-C incremental` 并删除 Cargo 增量目录（clean_incremental 默认 true），同处一行改动实测 ~3s 退化到 ~18s；`preserve_incremental` 让带增量的本地 crate 走旁路（增量目录隔离到 target/debug/incremental.kache-preserved），registry 依赖仍走缓存，新 worktree 冷构建共享 store 收益照旧。政策口径（热/冷回路分工、禁止另行配置 RUSTC_WRAPPER 等环境变量方案）住 AGENTS.md「编译加速归口 kache」。

另记录一处版本事实漂移：tracing-subscriber 0.3.23 的 reload 模块在 std（默认 feature）下无条件可用、无独立 reload feature——ADR-0006 修订注记「需启用 reload feature」的表述与该版本事实不符，按失准以代码为准处置（本票只删注释不改写 ADR 历史，ADR-0006 修订留待下次触达）。


## 修订注记（#2204 决策记录并入，2026-11-21）：掩码双源登记细节与已知边界自 scan.rs 注记并入

决策记录盘点票 #2204 把 Rust 侧掩码器具模块（根包 `test_support/scan.rs`）头注中本决策 3 未展开的双源登记细节并入本文；注释本体按四分类进入后续清理批次。

- **双源载体对**：Rust 词法掩码 `mask_non_code`（根包 `test_support/scan.rs`）与 TS 侧 `maskNonCode`（`gate-primitives.ts`）是同一条掩码规则的**两个运行时载体**（守门脚本跑 Bun、Rust 测试跑 cargo，单份实现不可共享）；规则改动必须两侧同步。防漂移机制 = 双侧消费共享语料夹具（`scripts/fixtures/` 下 Rust 掩码语料与期望输出，期望以 TS 侧权威实现生成）——任一侧单独改规则即语料测试红。
- **已知双源边界（登记在案的不对称与一致点，语料不含）**：非 ASCII 标识符紧邻原始串、非 BMP 字符入 char 字面量两形态，TS 侧按 ASCII/UTF-16 近似，两侧结果可能不同；`'\u{…}'` 转义两实现**一致地**不按字面量识别；TS 侧另有 `keepLiterals` 扩展形态（保留字面量只掩注释，Rust 消费面无此需求未实现）。边界扩展属规则改动，须两侧同步 + 语料更新。

决策 3 的识别口径、豁免闭集与家族双源同步纪律不变。


## 修订注记（#2206 壳层清零，2026-11-21）：本批删除注释承载的独有决策记录在此收口

- **Windows 控制台抑制载体**（根包 `main.rs`）：`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` 是 Windows release 构建不弹控制台窗口的载体（子系统语义）；删除该**属性**会改变 Windows 发布行为——属性是配置语义，其上的警示注释已按禁令清零，本条留痕防属性被当作无用代码移除。
- **DMABUF workaround 的 set_var 安全前提**（`src/lib.rs` run()）：`WEBKIT_DISABLE_DMABUF_RENDERER` 的 `set_var`（Rust 2024 起 unsafe）由 `main()` 同步首调 `run()` 执行，先于 Tauri 装配出的任何线程与 async runtime，进程环境仅主线程读写——这是该 unsafe 块的正确性前提；workaround 本体与跟踪票见 #2190，接线守门见壳层 webkit 守门模块。

## 修订注记（#2207 例外发现，2026-11-21）：豁免闭集扩入 compile_fail doctest 载体

#2202 分域清理批次 #2207 在 transaction / accounts 域各保留 3 行 compile_fail doctest 本体——活负向守门测试，ADR-0112 #2204 修订注记的 compile_fail 载体约束认可的唯一载体（业务域 crate 不得以 `use tauri_app_lib::…` 形态承载对壳层反向引用负向例，合法载体只剩协议 crate 根文档与 crate 内私有面「再公开即红」形态；sync-protocol 根文档负向用例同属此机制）。现行豁免闭集把这些行计为注释行，两键无法归零。按决策 2「扩豁免 = 修订本 ADR」路径，豁免闭集自「SAFETY + shebang」扩为「SAFETY + shebang + compile_fail doctest 载体」：

- **载体与理由**：Rust 文档注释（`///` 与 `//!`）内位于 ```compile_fail fence 的行（含 fence 开/闭行与体内行）是负向守门测试载荷，属代码非文档——删除即删测试。fence 体内的普通注释行是 doctest 源码的一部分，一并豁免。
- **豁免判定 = fence 状态机（确定性口径）**：开行 = 文档注释行（trim 后 `///` 或 `//!` 起头）内容以 ``` 或 ~~~ fence 标记起头且信息串含 compile_fail；自开行起豁免至闭行 inclusive（闭行 = 文档注释行内容仅为 fence 标记、无信息串）；未闭合 fence 豁免至文件尾（rustdoc 语义下未闭合 fence 的后续文档行仍属测试载荷，确定性优先，靠评审兜底）；非 compile_fail fence（```rust / ```ignore / ```no_run 等）不豁免，文档示例内注释照常计数。机制住门内（`check-comment-gate.ts` rust face），不入守门原语库——单消费方不满足原语 ≥2 门准入判据。
- **豁免计数单列 compileFailExempt**（与 safetyExempt 同构）：门禁报告输出独立计数，不并入其他豁免形态的计数。
- 基线随本注记重计：transaction 3→0、accounts 3→0（#2207 保留的两处载体），sync-protocol 与 backup 按各自 fence 行数下降，其余键零变化。

## 修订注记（#2211 正则字面量词法缺陷修复，2026-11-21）：决策 3 掩码识别口径补正则字面量词法，基线因毒化解除双向重计

决策 3 的 TS/Vue 掩码原语 `maskComments` 原只识别字符串与模板字面量，正则字面量不在词法内——#2204 修订注记的双源边界登记未涵盖此形态，属登记遗漏而非认可的限制，本注记按决策 3「门内自持口径改动 = 修订本 ADR」路径补齐。

- **缺陷现象与双向危害**（spec #2202 清零批次 #2211 发现）：`scripts/test-exec.ts:135` 的正则字面量 `/^(["'])(.*)\1$/` 内的引号被旧词法当字符串起点，伪字符串跨行错位配对、毒化面随引号配对级联放大——scripts 键 854 行真代码被误计为注释行（棘轮虚高）；同一毒化跨度内的真注释不被掩码即不被计出（src 键 `src/__tests__/no-native-confirm.test.ts:31-32` 两行真注释失明，棘轮虚低）。误计与失明同根因：正则字面量词法缺失。
- **修复口径（确定性优先，不追完整 JS 词法）**：正则起点判定用 prev 非空白字符启发式——回扫空白遇文首，或 `/` 为本行首个非空白字符（行首），即正则候选；同行前导字符属闭集 `( , = : [ ! & | ? { } ; >`（`>` 覆盖 `=>` 前导，仓内实证 4 处）即候选；前导标识符为 `return`（带词边界）即候选；`#!` shebang 行的 `!` 前导排除。注释形态（`//`、`/*`）优先于正则起点裁定。正则体内 `\` 转义不闭合（转义后紧跟换行即放弃）、`[...]` 字符类内 `/` 不闭合；换行前无闭合 `/` 即放弃、按普通字符处理（正则字面量不可跨行，候选不逃逸）。正则体只跳过不掩码（是代码非注释）。机制住 gate-primitives（既有收口库，非新单消费方准入问题）。
- **残余面**：启发式不追认的形态维持除法（逐字扫描）解读，毒化风险与修复前同源、靠评审兜底——同行前导不在闭集（如 `)` 之后）、正则起点紧跟块注释（回扫不做注释感知）、换行续行的行首除法（上一行以标识符收尾）。仓内扫描面（scripts/src/packages/根配置）实证这些形态不存在。
- **消费方与基线**：check-frontend-structure 与 check-commands 真实仓库行为零变化（消费面测试与门禁全绿，毒化跨度内不含 import/invoke 形态）；本票分支基点重计 scripts 854→0、src 10748→10750——src +2 为毒化解除后重新计出的失明真注释行，是「基线只许降不许升」的本票例外，登记于本注记。并行 #2210 src/packages 清零合入后按合并树全量重计，最终基线 src=2、其余 47 键全 0：src 的 2 行即 no-native-confirm.test.ts 那两行失明复明行（#2210 清理时旧词法不可见而幸存，如实重计非注释净增）；src 注释清理辖区归 #2210 批次，本票不动其注释内容。

## 修订注记（#2212 归零收口，2026-11-21）：基线清退、门禁切零容忍——棘轮机制转为休眠能力

清理批次 #2205–#2211 全部合并后，归零收口（spec #2202 批次 4）完成，本注记按决策 2/4 的修订路径登记切换事实：

- **基线数据面退役**：`scripts/fixtures/comment-baseline.json` 删除。门禁两模式判定改为「基线文件缺席 = 零容忍模式；基线文件存在 = 棘轮模式」——零容忍模式下任何键出现任何注释行即红，报文给 文件:行 与本文指针；棘轮模式的键集双向全等、非负整数等 schema 校验原样保留。基线文件存在但非法（坏 JSON、非对象）仍 fail loud 红，不静默回退零容忍——缺席是模式判定，存在即校验。
- **棘轮机制保留为休眠能力**：`--write-baseline` 在两模式下均可再生成基线（写入即进入棘轮模式），供未来治理复用；删除基线文件即回到零容忍模式。重开棘轮 = 修订本 ADR。
- **末批处置留痕**：src 键最后 2 行真注释（`src/__tests__/no-native-confirm.test.ts`，#2211 修订注记的失明复明行）按四分类归同义复述——动态引入拦截与「open 动态引入不误伤」语义均已由检测函数「模块名 + confirm 词元」双条件表达，原生 confirm 清零政策本体已收录 ADR-0078 决策 1，直接删。
- **全仓总量终态**：48 键全 0；豁免闭集实测 = `// SAFETY:` 4 行、shebang 14 行、compile_fail 载体 12 行，gate 报文、rg 全仓枚举与本注记及 #2207 修订注记数字三方一致。豁口辩护现存登记与后续逐项修复指向见 #2213。
