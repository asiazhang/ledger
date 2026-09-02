# ADR-0056: 后端域目录化——域/壳/基础设施三层定义与依赖方向守门

- 状态：已接受
- 日期：2026-09-02
- 作者：Ledger 项目
- 关联：spec #394 / issue #396（本文档票）；前置 #395（口径修缮先行，先修后搬）；逐域搬迁 #397–#401；收口 #402；守门先例 ADR-0047（命令注册单一来源的质量门槛化）；与 ADR-0032（置脏收口连接层）/ ADR-0033（行为层事务）/ ADR-0044（信号映射）无冲突——彼等约束接缝契约，本文只动实现住址

## 背景

后端约 73% 的非测试实现逻辑住在 `commands/`，而这个名字承诺的是「IPC 命令壳」。导航假设在最重的接缝上恰好失效：域行为核心（买入/卖出协议、保单校验与统计、账户余额口径、物品 CRUD）要到「命令层」里找；同一时刻核心交易、定时计划两域的实现住顶层目录，物品域更是「一个域两个家」（成本口径在顶层 `item/`、主体在 `commands/item/`）——三种目录形状并存，「某域的逻辑在哪」需要试错。依赖方向（域不许依赖壳）目前只靠模块头注释自律，目录树无法表达：域 import 壳的反向依赖在编译期与评审期都没有机器信号，任何人都可能无意打破。

## 决策

### 1. 三层定名与职责

- **壳层**：IPC 命令（`src-tauri/src/commands/`）与 HTTP 端点（`src-tauri/src/api_server.rs`）。只做参数解包、事务壳（ADR-0033）、信号发射（ADR-0044），不含业务语义——打开壳层任何文件都应能确信它读不到领域规则。
- **域目录**：顶层按域组织的引擎实现（既有先例：`src-tauri/src/transaction/` 核心交易、`src-tauri/src/scheduled_transactions/` 定时计划）。域接口用域语言命名，测试以外挂测试模块/目录随迁。
- **基础设施**：数据库连接（`db/`）、信号映射（`signals.rs`）、模型（`models/`）、错误（`error.rs`）、设置（`settings.rs`）。无域语义，被所有层消费。

### 2. 依赖方向：壳 → 域 → 基础设施，域永不依赖壳

- 单向依赖；机器化守门见决策 4。域与域之间的横向消费允许且是既有事实（如定时计划消费核心交易的 Writer 接缝），被禁止的只有「域/基础设施依赖壳」这一个方向。
- 本守门只机器化这一条最危险的边；壳内部互调、域间横向依赖等其余方向仍靠评审。

### 3. 路线图：前置口径修缮 + 五阶段 + 两检查点

- **前置（先修后搬，#395）**：两处金额口径修缮（期次合计过矩阵、搜索金额过滤切本位分）独立提交先行，保证搬迁提交零语义变化、diff 可机械核对。
- **阶段 1（#397）物品域归位——样板工程**：定格域目录形状（纯声明入口含再导出、每天成本口径模块不动、域 API 单文件、溯源守卫独立成文件、测试外挂随迁、迁空子目录压平为单文件壳、接口去 internal 黑话）。完成后设**检查点 ①**：以真实收益对照搬迁成本，再决定后续投入。
- **阶段 2–4（#398/#399/#400）叶子域清空**：保单 → 预算 → 商户先后归位，使剩余横向依赖最重的投资域问题面最小。商户完成后设**检查点 ②**，大体量搬迁前再确认。
- **阶段 5（#401）投资域归位**：最大体量、横向依赖最多，故放最后。
- **收口（#402）**：剩余内容归属 triage + 文档同步（词汇表地图的结构句子在全部迁完后才修改）。
- **每域纪律**：独立 git worktree、整文件 `git mv` 优先（历史可跟随）、一域一提交、一键质量门槛全绿（含结构守门）、守门白名单追加一行、引用旧路径的注释随所在阶段清扫。

### 4. 结构守门：白名单式脚本

- `scripts/check-structure.js`：白名单 = 已归位域目录 + 全部基础设施（均先验证对壳层零依赖）；白名单内出现对壳层的模块路径依赖即红。挂入 `scripts/check.sh` 与 CI，与命令注册一致性检查并列。白名单本身即规格——它是质量门槛而非测试（先例：命令注册双向全等检查、i18n key 全等检查、文档一致性检查）。
- 扫描边界：文本级扫描，注释与字符串字面量掩码后匹配壳层模块路径引用（`commands::` 路径与 `use … as` 别名引入）；经别名改名的间接引用文本不可达，属违规形态，靠评审兜底。
- fail loud：白名单路径缺失、条目内扫不到非测试 Rust 文件、发现反向依赖，均非零退出并列出 `文件:行`。

### 5. 测试豁免约定

- **豁免** = 外挂测试模块与测试目录（`tests.rs` 文件、`tests/` 目录；先例：写入接缝的外挂测试目录）：BDD/单元 fixture 合法引用壳层入口（既有先例：BDD 直调命令层内部函数），不制造虚假违规。
- **不豁免** = 普通文件内的内联 `#[cfg(test)]` 模块：白名单内当前零壳层引用；内联测试若需引用壳层，应外挂为测试目录——豁免不以放宽主文件约束为代价。

## 迁移状态与剩余内容 Triage（#402 终态收口）

- **路线图五域（#397–#401）全部归位完成**：
  - 核心交易 `transaction/`（既有）
  - 定时计划 `scheduled_transactions/`（既有）
  - 物品 `item/`（阶段 1 #397 归位：域 API `item::domain` + 溯源守卫 `item::guard` + 成本口径 `item::cost`，壳层压平为单文件 `commands/item.rs`）
  - 保单 `policy/`（阶段 2 #398 归位：CRUD / 统计 / 校验分主题模块，壳层压平为单文件 `commands/policy.rs`）
  - 预算 `budget/`（阶段 3 #399 归位：CRUD / 进度分主题模块，壳层压平为单文件 `commands/budget.rs`）
  - 商户 `merchants/`（阶段 4 #400 归位：字典 CRUD 与按名查找/即建，壳层压平为单文件 `commands/merchants.rs`）
  - 投资 `investment/`（阶段 5 #401 归位：买卖协议三件套 / 持仓 / 走势 / 行情与汇率录入 / 基金接入分主题模块，价格写入单点自 `sync::persist` 迁入 `investment::prices`，统一模糊搜索语义纯函数迁入 `transaction::search_text`；壳层压平为单文件 `commands/investment.rs`）
  - 基础设施五处（`db/`、`signals.rs`、`models/`、`error.rs`、`settings.rs`）已入守门白名单。

### 剩余内容逐项 Triage 判定表（无未判定项）

| 模块 / 文件 | 当前位置 | 归属判定 | 目标位置 | 判定理由 | 后续票 |
|---|---|---|---|---|---|
| **交易行为与读取** | `commands/transactions/` (`behavior.rs`, `read.rs`) | 迁移 | `src-tauri/src/transaction/` | 交易创建/修改/删除编排三入口、嵌套事务感知、副作用分派与读取实现是核心交易域引擎本体，非 IPC 壳 | #403 |
| **交易搜索查询** | `commands/search/` (`query.rs`) | 迁移 | `src-tauri/src/transaction/` | `TransactionSearch` 的 SQL 候选全量扫描与流式分页实现，与既有 `transaction::search_text` 纯文本匹配汇流归位 | #403 |
| **交易批量写入** | `commands/batch/` (`mod.rs`) | 迁移 | `src-tauri/src/transaction/` | `TransactionBatch::run` 批量落库、幂等键/内容哈希去重判定与批次汇总日志为核心交易域批量编排能力 | #403 |
| **账户** | `commands/accounts/` (`core.rs`) | 迁移 | `src-tauri/src/accounts/` | 账户 CRUD、自然键幂等创建、币种锁定守卫、黑洞账户创建与余额调整交易编排等独立领域规则 | #404 |
| **分类** | `commands/categories/` (`core.rs`) | 迁移 | `src-tauri/src/categories/` | 分类 CRUD、自然键幂等创建、两级分类校验、预算删除守卫与排序重排等独立领域规则 | #404 |
| **币种** | `commands/currencies/` (`mod.rs`) | 迁移 | `src-tauri/src/currencies/` | 参考数据币种列表查询实现，独立建立顶层微域目录，壳层压平为单文件 | #404 |
| **报表** | `commands/reports/` (`mod.rs`) | 迁移 | `src-tauri/src/reports/` | 月度汇总、分类下钻、商户排行与日期极值聚合读模型，消费 `transaction::amount` 矩阵 | #405 |
| **仪表盘** | `commands/dashboard.rs` | 迁移 | `src-tauri/src/dashboard/` | `query_dashboard_overview` 全仓净资产跨币种折算聚合逻辑下沉域目录，壳层退化为薄壳 | #405 |
| **财务自由度** | `commands/financial_freedom.rs` | 迁移 | `src-tauri/src/investment/` | `query_financial_freedom` 自由度计算口径（投资域 InvestableAssets 词条），下沉投资域 | #405 |
| **备份与自动备份** | `commands/backup/core.rs` + `src-tauri/src/auto_backup.rs` | 迁移 | `src-tauri/src/backup/` | 备份/恢复/受管备份清理核心引擎与自动备份调度、到期判定纯函数、本地日界门整合归入顶层 backup 域 | #406 |
| **行情同步** | `commands/sync/` | 迁移 | `src-tauri/src/sync/` | HTTP 网络客户端、东财基金净值爬取、增全量同步编排独立建顶层域目录，壳层压平 | #407 |
| **数据位置** | `commands/data_location.rs` | 迁移 | `src-tauri/src/db/data_location/` | `validate_and_commit` / `gather_info` 数据库引导与位置切换三步校验下沉 db 基础设施，壳层压平 | #408 |
| **AI 提示词** | `commands/ai.rs` | 确认纯壳 | `src-tauri/src/commands/ai.rs` | 纯 IPC 壳命令，仅读取静态内置提示词模板文件，零领域逻辑，无需单独建域 | — |
| **日志查看** | `commands/logs.rs` | 确认纯壳 | `src-tauri/src/commands/logs.rs` | 系统控制类薄壳，仅调用平台 opener 打开日志目录，零领域逻辑 | — |
| **应用重启** | `commands/backup/mod.rs` 中的 `restart_app` | 确认纯壳 | `src-tauri/src/commands/backup.rs` | 系统控制类命令，调用 `app.restart()` | — |
| **取消行情同步** | `commands/sync/mod.rs` 中的 `cancel_sync_instruments` | 确认纯壳 | `src-tauri/src/commands/sync.rs` | 控制类命令，操作同步状态取消标志 | — |
| **原子文件工具** | `src-tauri/src/fs_util.rs` | 基础设施 | `src-tauri/src/fs_util.rs` | 通用文件原子操作与临时文件工具，零业务语义，列入守门白名单 | #408 |
| **日志基础设施** | `src-tauri/src/logger.rs` | 基础设施 | `src-tauri/src/logger.rs` | tracing 日志初始化与 7 天自动滚动清理，零业务语义，列入守门白名单 | #408 |
| **事件投递机制** | `src-tauri/src/events.rs` | 基础设施 | `src-tauri/src/events.rs` | 失效信号与 payload 事件主线程非阻塞投递机制（ADR-0044/0054），列入守门白名单 | #408 |
| **HTTP 接口服务** | `src-tauri/src/api_server.rs` | 壳层 | `src-tauri/src/api_server/` | 与 `commands/` 并列为外部壳层，单文件 1100+ 行待后续专项目录化重构 | 另立专项 |

## 开放问题（已决收口）

- **行为层编排入口归位时点**：已裁决。`transactions/behavior.rs` 与 `read.rs`、`search/query.rs`、`batch/` 共同构成核心交易域的完整读写编排与搜索能力，正式归位 `src-tauri/src/transaction/`（由后续票 #403 执行）。
- **行情同步与备份的归属**：已裁决。`commands/sync/` 拥有完整的 HTTP 网络爬虫、基金净值解析与全增量同步编排，单独立顶层域目录 `src-tauri/src/sync/`（由后续票 #407 执行）；`commands/backup/core.rs` 与 `auto_backup.rs` 整合为顶层 `src-tauri/src/backup/` 域目录（由后续票 #406 执行）。
- **至此，ADR-0056 开放问题全部收口落定。**

## 备选方案与否决理由

- **维持现状 + 注释自律**：即本文要替换的现状——目录形状三态并存、反向依赖无机器信号，已被判定失效。
- **黑名单式守门**（列出禁止依赖壳的目录）：默认放行新代码，每加一个域都要记得拉黑；白名单默认约束、每迁一域追加一行，把「已验证零依赖」固化为规格。
- **编译期/依赖图工具**（cargo-deny 类）：模块级方向不是 crate 依赖图上的边，无现成工具；引入重依赖或自定义 lint 的维护成本高于百行文本扫描，且既有门槛脚本先例已验证「文本级扫描 + fail loud」有效。
- **一次性大搬迁**：review 无法机械核对移动完整性，出问题无法按域回滚；逐域 + 两检查点把风险切片，投资域前有真实收益证据再投入。

## 后果

- **终态**：`commands/` 退化为纯 IPC 壳（迁空子目录压平为单文件壳），「某域的行为逻辑在哪」一次命中；「打开命令层文件必无业务语义」从注释变成可守门的承诺。
- **导航分工**：代理导航文档（AGENTS.md）记分层规则与已定格路径，不背进度清单；本 ADR 记决策、路线图与迁移状态（每域随手更新）；词汇表地图的结构句子全部迁完后才同步——文档始终描述当前真实状态。
- **措辞**：存量「命令层」措辞随所触阶段统一为「壳层」。
- **零影响面**：schema、迁移文件、IPC/HTTP 对外契约、命令注册清单全部不变；「已发布 API 只增不改」约定不受影响。
