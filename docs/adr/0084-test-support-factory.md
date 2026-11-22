# ADR-0084: 统一测试数据库工厂与共享测试器具——深模块收敛、白名单转纯禁令、固定时刻显式化

- 状态：已接受（按域迁移已收官，白名单已转纯禁令；正文已按 #758、#956、#1111、#1433、#1514、#1645、#1699/#1702、#1868、#1896、#1989 现行态收敛）
- 日期：2026-09-07（现行态收敛：2026-10-31）
- 作者：Ledger 项目
- 关联：spec #728（前身 #250）；ADR-0056（守门哲学与测试豁免）；ADR-0067（余额缓存对拍）；ADR-0083（守门 Bun 运行时）；ADR-0086（BDD 步骤层互斥边界）；ADR-0112（workspace crate 拆分与 dev-dependency 环）；ADR-0139（跨库原子性文件库测试前提）；词汇表：测试基础设施域

## 背景

后端测试建库样板（`open_in_memory` + `init_db`）、账户种子（8 种异名/变体）、裸 SQL 夹具、固定时刻字面量（160+ 处）以及余额缓存对拍断言曾散落七个域辅助文件（652 行），造成重复维护与漂移。同时，随着多端同步、源码扫描守门、读快照一致性与隔离环境等跨编译单元测试需求增长，测试基础器具需要统一的准入边界与防回潮守门。

## 决策

### 1. 深模块收编与准入规则

建立统一测试数据库工厂与共享测试器具：打开库、种子、共享断言与跨编译单元测试器具收口一处；各域测试辅助文件仅保留单域特有的薄皮（计划创建、occurrence 读取器等）。

- **准入规则**：仅收编**跨 ≥2 域或编译单元同体消费**的夹具与测试器具；单域特有的种子、读取器与业务写中途失败注入留各域，防止工厂长成大杂烩。
- **与 BDD 层互斥（ADR-0086）**：e2e 层不消费本工厂的建库、种子与默认值集（避免一次默认值调整同时打红两层）；但通道线格式替身、WebDAV 协议替身、暂存目录 guard 等纯协议/基础设施器具可跨层共享。

### 2. 住址、可见性与 dev-dependency 环

- 工厂住根包 `src-tauri/src/test_support/`，以 `pub mod` + `#[doc(hidden)]` 暴露给集成测试与各域 crate 测试目标（经 `[dev-dependencies] tauri-app` 测试专用环，Cargo 允许且不污染生产依赖图，结构守门仅辖生产依赖）。
- 仅在负向 `compile_fail` 用例会被 dev-dependency 环击穿时（如 `ledger-sync-protocol` 根文档验证不可见 `tauri_app_lib`），该 crate 保持无环（见 ADR-0112 修订注记）。
- 注：初版在单 crate 阶段曾以「自依赖循环不可解」否决独立测试 crate；workspace 落地（ADR-0112）后该前提已解除，独立测试支持 crate 成为后续可选目标形态。

### 3. 建库接口形态：`&Connection` 自由函数与模板还原

- **`open() -> Connection`**：零配置内存库入口，不引入包装类型或 builder 链（与 `&Connection` 全仓流转一致）。内部采用**模板还原**（#1514，`ledger_infra::db::open_in_memory_initialized`）：进程内首次 `serialize` 固化迁移后 schema 与默认种子，后续每次 `deserialize` 还原独立内存库（约 31ms → 40µs/次），再经 `finish_open` 单点重施外键与 hook，用例间隔离性不变。
- **文件库变体（#1699 / #1868）**：双连接读快照探针需同文件多连接，跨库（main + ATTACH sync.db，ADR-0139）集合级原子性断言需真实落盘文件库——提供 `open_file(dir)` 与 `open_file_scratch()`（自带 `ScratchDir` guard 返回元组，逐用例重放迁移链），建库接线与 `open()` 共用 `install_test_wiring` 单点。

### 4. 种子、固定时刻与共享断言

- **建账户签名归一**：`seed_account` 单一全位置签名 `(conn, id, name, kind, currency, initial_balance_cents)`，调用方显式传 `id`（显式外键引用；否决 options struct + `Default` 隐藏关键字段）；投资铺垫（账户+标的+汇率）收口组合种子。
- **固定时刻两类分治**：夹具簿记戳（`created_at`/`updated_at`）由工厂命名常量 `FIXED_NOW` 内部发放，调用点清零；域时刻（价格序列、预算窗口、定时触发点）属行为输入，由调用方显式传入（否决递增发放隐藏时序）。
- **共享断言**：余额缓存「回填 == 实时」对拍（`assert_balance_cache_matches_realtime`，ADR-0067 唯一测试维护点；`ledger-perf` 保留独立断言不归一）+ 通用行读取器。

### 5. 跨编译单元共享测试器具清单（按准入规则收编）

除建库/种子/断言外，工厂收编以下跨 ≥2 编译单元的测试器具（对应词条见 `CONTEXT-testing.md`）：

| 模块 | 职责与核心约束 |
|---|---|
| `channel`（#956） | 通道线格式替身 `publish_raw_segment`，摘要口径直用产品 `channel::sha256_hex`，防夹具误算 payload 摘要 |
| `scan`（#1433） | Rust 源码扫描掩码 `mask_non_code` + `matching_brace_end`；与 TS 侧 `maskNonCode` 双源登记，共享语料夹具防漂移 |
| `scratch`（#1645） | `ScratchDir` / `ScratchFile`（`ledger-test-` 前缀），drop（含 panic unwind）整树清理，终结 `/tmp` 残留 |
| `snapshot_probe`（#1699/#1702） | 基于 `sqlite3_trace_v2` STMT 回调的读快照探针，在多语句读闭包中间由第二连接确定性注入并发写，覆盖七域读事务回归 |
| `op_write_failure`（#1896） | `temp` schema 触发器注入 `sync.sync_ops` 写失败（跨库可用、仅本连接可见、内存/文件库同形） |
| `isolate_home`（#1989） | 进程级现场隔离：`Once` 同步重定向 `$HOME` + `$XDG_DATA_HOME` + `$XDG_CONFIG_HOME` 到独立临时目录，防 Linux `dirs::data_dir()` 逃逸污染真实用户目录 |

### 6. 纯禁令守门（`scripts/check-test-support.ts`）

按域渐进迁移于 #758 收官后，原白名单机制整体移除，转为**纯禁令守门**（挂入 `check.sh`，Bun 运行时 ADR-0083）：

1. 禁在工厂与 db 域外直连建库样板（`open_in_memory` / `init_db`；不辖 `tests/e2e`）。
2. 禁在工厂与域薄皮外对夹具表写裸 SQL（`tests/e2e` 存量例外逐条登记并严格计数，见 ADR-0086）。
3. 禁在工厂外出现 `FIXED_NOW` 默认时刻字面量。
4. 禁测试代码手工构造通道段/清单或自建通道摘要（#956）。

## 备选与否决

- **仅去重不建统一入口**：8 种签名多为偶然漂移，无法消除样板与时刻戳噪音，否决。
- **包装类型 + builder 链**：与全仓 `&Connection` 流转习惯冲突，迁移摩擦翻倍，否决。
- **options struct + `Default` 种子签名**：隐藏关键外键/币种字段、损测试可读性，否决。
- **时刻递增发放**：调用点无法看出先后时序，否决。
- **不加守门靠评审**：历史样板累积到 15 处无人拦即证其失效，否决。

## 后果

- 后端测试建库、夹具、簿记时刻、缓存对拍与跨编译单元器具各有单一权威入口，回潮由 `check-test-support.ts` 自动拦截。
- 模板还原使单测建库成本从毫秒级降至微秒级（文件库变体因需真实落盘仍为毫秒级）；进程级 `isolate_home` 与 `ScratchDir` 消除了本地环境污染与 `/tmp` 堆积。
- `test_support` 的测试专用域间边（`test_support → sync_engine` / `accounts` 等）不进产品依赖图，由本 ADR 与结构守门豁免承接。
