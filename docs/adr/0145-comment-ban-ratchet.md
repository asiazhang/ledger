# ADR-0145: 代码注释零容忍禁令——注记住址改 ADR 本体与守门脚本数据面

- 状态：已接受（spec #2202；全仓 48 键已于 #2212 清零收口，门禁切换为零容忍模式，棘轮机制转为休眠能力）
- 日期：2026-11-21（归零收口：2026-11-22）
- 作者：Ledger 项目
- 关联：废止 ADR-0113 决策 5（「注记归 crate `//!` 与模块文件头」）；ADR-0112 / ADR-0056（守门家族与 crate 政策核）；ADR-0083（守门脚本 Bun 运行时）；ADR-0084（掩码双源登记）；ADR-0087（删除即变红断言强度）；#2202（spec）、#2203–#2212（治理与清零批次）；Excuse 跟踪 issue #2213

## 背景

在 agent 主导的开发模式下，代码注释的两类系统性危害被显著放大：
- **第二事实源漂移**：代码改动不同步注释时，过时注释反向误导后续 agent 与评审者；治理前全仓存量注释约 5.2 万行（占代码 ~16%），无机械防线阻止净增长。
- **政策数据错位**：此前将模块/crate 注记安放于文件头与 `//!`（ADR-0113 决策 5），机器不消费却被当作权威引用。

同时，注释中混杂的真实知识（`unsafe` 正确性前提、依赖/构建配置理由、未决技术债豁口）必须先迁往权威住址再删除，确保「删注释不蒸发信息」。

## 决策

### 1. 禁令辖域与扫描边界

- **覆盖全部代码与配置注释**：Rust 全形态（`///`、`//!`、`//`、`/* */`）；TS/Vue/JS 行与块注释（含 JSDoc）；shell 行首 `#`；`Cargo.toml` 与 `.kache.toml` 的 `#` 行；`package.json` 的 `"//"` 数据键。
- **不辖范围**：Markdown 文档（`docs/`、词汇表、ADR、skill）；`node_modules` / `target` / `dist` / `.git`；`scripts/fixtures/`（掩码语料与门自用数据面）。其他 JS 方言扩展（如 `.mjs`）与 Vue 模板 HTML 注释不入扩展名闭集，靠评审兜底。

### 2. 豁免闭集（扩豁免 = 修订本 ADR）

仅三类语义载体豁免，门禁报告分别单列计数：
1. **`// SAFETY:` 形态**：行 trim 后以 `//`（可带 `/` 或 `!`）起头且紧跟 `SAFETY` 加冒号/全角冒号/括注——承载 `unsafe` 块正确性前提（仅首行豁免，续行照常计数）。
2. **shebang 行**：shell 首行 `#!`（执行语义行，非注释）。
3. **`compile_fail` doctest 载体（#2207 扩入）**：Rust `///` / `//!` 中位于 ```` ```compile_fail ````（或 `~~~compile_fail`）fence 起止行及体内行——属于活负向编译守门测试载荷，删除即删测试；非 `compile_fail` fence（如 `rust`/`ignore`/`no_run`）不豁免。

### 3. 注释识别口径

- **Rust 与 TS/Vue**：采用行掩码口径（复用守门家族共享原语 `maskNonCode` / `maskComments`，掩码后与原文逐行比对，字符串字面量内注释字样不误报）。
  - **双源登记与已知边界（#2204）**：Rust 侧 `test_support::scan::mask_non_code` 与 TS 侧 `maskNonCode` 为同一规则的两个运行时载体，经共享语料夹具防漂移；`'\u{…}'` 转义两实现一致不按字面量识别，非 ASCII 标识符紧邻原始串/非 BMP char TS 侧按 ASCII/UTF-16 近似，`keepLiterals` 仅 TS 侧提供。
  - **TS/Vue 正则字面量词法（#2211 补齐）**：`maskComments` 通过前导非空白字符闭集与 `return` 启发式识别同行正则字面量并跳过不掩码，消除正则内引号错配导致的误计与失明毒化；不追完整 JS 词法（极少见的续行除法等靠评审兜底）。
- **shell / TOML / `package.json`**：单消费方不入共享原语库，由门内自持确定性规则（shell/TOML 计行首 `#`、豁免 shebang；`package.json` 计 `"//":` 行；行尾 `#` 与折行值靠评审兜底）。

### 4. 运行模式：零容忍（棘轮休眠，#2212 归零收口）

- 清理期采用按目录键（48 键）的棘轮基线 `scripts/fixtures/comment-baseline.json`（只降不升、键集双向全等、归零即零容忍）。
- **终态（#2212）**：48 键已全部归零，基线文件已删除。门禁按基线文件是否存在自动切换模式：
  - **文件缺席 = 零容忍模式（现行）**：任何受守文件新增任意非豁免注释行即红，报错指向文件:行与本 ADR；
  - **文件存在 = 棘轮模式（休眠能力）**：`--write-baseline` 仍可重新生成基线，文件存在但格式非法仍 fail loud 红；重开棘轮须修订本 ADR。

### 5. 存量四分类处置（优先级：决策记录 > 豁口辩护 > 失准 > 同义复述）

流程载体为 `/delete-comment` skill，术语归测试基础设施词汇表：
- **决策记录**：承载代码查不到的真实决策/理由 → 先核对并并入对应 ADR、词汇表或守门数据面，再删注释（信息只搬家不蒸发）。
- **豁口辩护**：为 hacky 实现辩护 → 登记至跟踪 issue #2213（位置 + 豁口 + 修复票）后删注释，修复不与清理捆绑。
- **失准**：与代码矛盾 → 以代码为准删注释（若暴露真需求缺口按范围外纪律报告）。
- **同义复述**：复述代码已有语义 → 直接删。

### 6. 注记住址改址

模块/crate 的职责、不变量、ADR 指针与陷阱注记，统一改住 **ADR 本体 + 守门脚本数据面**（废止 ADR-0113 决策 5 的文件头/`//!` 作者位条款）。

## 随清零收口至本 ADR 的独有决策记录（#2205 / #2206）

以下决策记录原仅存于配置与壳层注释，无其他专属 ADR 住址，清零时收口于此以防信息蒸发（vitest 配置决策记录已同期归位 `CONTEXT-testing.md`，依赖 hold 归位 `docs/agents/dependency-upgrades.md`）：

- **备份版本锁步（backup crate）**：`after_commit_hook` 以编译期 `env!("CARGO_PKG_VERSION")` 写备份元数据 `app_version`，workspace 成员升版须同步，否则元数据版本失真。
- **默认 bin 与 bin 单测排除（根包）**：第二 bin `ledger-perf` 存在下显式 `default-run` 指向主程序；`main.rs` 声明 `test = false` 免链免跑（受 `scripts/test-exec.ts` 守门），`ledger-perf` 留单测豁免。
- **updater `default-features = false`（根包，伴随 ADR-0124）**：default 含 `system-proxy`，在 macOS 下 `hyper-util` 只读代理地址不读例外表，会劫持行情测试对 `127.0.0.1` 的直连请求；故显式仅启 `rustls-tls` 与 `zip`（macOS `.app.tar.gz` 解包依赖此 feature）。
- **dev 依赖去符号（根包 `[profile.dev.package."*"] debug = false`）**：消除 Xcode 26 `ld` 对 kache 共享 store 还原产物追溯 DWARF/OSO 路径的告警，不影响业务 crate 断点调试。
- **release 产物瘦身（根包 `[profile.release]`，#1999）**：启用 strip + LTO + `codegen-units = 1` 压减 APK/二进制体积；刻意不设 `panic = "abort"`（生产代码依赖 `catch_unwind` 保护事务闭包，ADR-0125）；`opt-level` 保持默认 3（是否切 `"s"` 以 perf-bench 数据裁决，#2000）。
- **infra 保留 `tracing-subscriber` 为常规依赖**：`test_utils` 信号捕获 Layer 随 feature 编入 lib 供下游测试可见，`dev-dependencies` 无法传递进下游闭包。
- **腾讯行情 GBK 解码（market-sync，伴随 ADR-0130）**：以 `encoding_rs` 标准 WHATWG 实现解码并 fail-closed，不把替换字符混入权威名称。
- **kache `preserve_incremental` 机理（`.kache.toml`）**：避免 kache 剥离 `-C incremental` 致单行修改重编译退化（~3s → ~18s），把本地增量隔离在旁路同时保留 registry 共享缓存。
- **Windows 控制台抑制（根包 `main.rs`）**：`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` 为 release 子系统行为载体，删除该属性会令 Windows 发布版弹出控制台窗口。
- **DMABUF workaround `set_var` 安全前提（根包 `lib.rs` `run()`，#2190）**：由 `main()` 同步首调、早于 Tauri 创建任何线程与 async runtime，仅主线程读写进程环境。

## 备选与否决

- **评审劝退不设守门**：无法阻断 agent 顺手留注释的净增长，否决。
- **借道 oxlint / clippy**：模型不匹配豁免闭集与棘轮，且覆盖不到 TOML / `package.json`，否决。
- **clippy `missing_docs`**：强制文档注释与禁令方向相反，否决。
- **per-file 基线 / 先清后禁**：前者键数不可维护，后者让治理被清理周期阻塞数月，均否决。

## 后果

- 全仓维持零非豁免注释（新增即红），信息沉淀于 ADR、词汇表、守门数据面与自解释代码；历史技术债豁口集中跟踪于 #2213。
- 豁免闭集扩展、重开棘轮或更改门内词法识别口径，均须修订本 ADR。
