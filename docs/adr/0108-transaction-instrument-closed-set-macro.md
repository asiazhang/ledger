# ADR-0108: TransactionKind / InstrumentType 闭集单一化——共享 `closed_set!` 宏同体派生，DB CHECK 字面量测试期互核兜底

- 状态：已接受（grilling 定稿，2026-09-11）
- 日期：2026-09-11
- 作者：Ledger 项目
- 关联：#1022（本票）；ADR-0102（`write_op_set!` 宏模式先例与「构造性保证 > 可检测」选型框架）；#1011/#1023（先例 spec 与落地验证）；ADR-0056（基础设施住址与结构守门白名单）；ADR-0073（消费方原则——本票 ALL 有生产消费方，其「仅测试不抬编译期」顾虑不适用）；ADR-0087（断言强度——互核断言对准实际应用的 schema）

## 背景

#1022（建票后按代码复核修正）确认两个闭集的字符串面比 ADR-0102 立票时的初判更宽：

- **`TransactionKind`（`transaction/amount.rs`，9 变体）五份表示**：enum 本体（真源）、`ALL` 手抄定长数组、`as_str` 手写 9 臂穷尽 match（DB 存储形状 + `Display` + serde serialize 底座）、`parse` 手写 9 臂穷尽 match（serde 反序列化 + `FromSql` DB 读边界复用）、DB `CHECK(kind IN (...))`（V001，已发布迁移冻结副本）。`as_str` / `parse` 是编译期强制面（穷尽 match），**`ALL` 漏登完全静默**，且静默面是生产消费方：OpenAPI `enum_values` 直达已发布 AI API 契约、SQL 片段生成（`kind_case_expr` / `contributing_kinds`）直达服务端聚合口径。
- **`InstrumentType`（`investment/model.rs`，5 变体）四份表示 + 一条无守门的双字符串面**：enum 本体 + 手抄 `ALL`、`Display`/`FromStr` 手写 match（`ToSql`/`FromSql` 骑行其上）、serde `rename_all = "snake_case"` derive——与 `Display`/`FromStr` 是两套独立字符串面，当时恰好同形、漂移无守门。

与 ADR-0102 的 WriteOp 场景有一处关键差异：WriteOp 的 `ALL` 消费方全在测试侧，而本票 `ALL` 有生产消费方，漏登静默直达用户可见契约——痛面更宽，机制升级的理由更强。

## 决策

1. **机制 = 宏同体派生（ADR-0102 模式复用），新宏 `closed_set!`**。调用清单吃 `(变体 => 字面量)` 对，同一 token 流同体展开五产物：enum 本体（derive 与 `///` 文档经 meta 原位透传）、`ALL`、`as_str`（const fn）、`parse`（未知值报参数错误，文案附合法值清单）、`Display`（骑行 `as_str`）。字符串字面量每变体只出现一次，「新增变体漏登 `ALL` / 漏写 `as_str` / `parse` 臂 / 双面漂移」的失败类**不可表达**（构造性保证）。
2. **域自有接缝不由宏生成**：serde 手写 impl（serialize 骑 `as_str`、deserialize 复用 `parse`）、`FromSql`/`ToSql`、utoipa `PartialSchema` 保持各域手写——它们骑行宏产物，本身不含第二份字面量事实。两域 impl 形状同构（先例互引），是否上收为宏第六产物留待第三处消费出现再议（ADR-0084 准入规则同款纪律：≥2 重复才收编）。
3. **宏住址 = 新基础设施文件 `src-tauri/src/closed_set.rs`**（`macro_rules!` + `pub(crate) use` 路径导出），check-structure 白名单追加一行（ADR-0056：无域语义、被所有层消费）。不同于 `write_op_set!` 的单域就地形态——其「局部性即卖点」前提是单一使用点，两域共用后共享基础设施是正确归位。
4. **`ALL` 保持定长数组**：`[$name; [$( $name::$variant ),*].len()]`（匿名 const 技巧；`${count}` 元变量表达式在 rustc 1.98 仍不稳定，#83527 未落地）。既有消费方（`ALL.map(...)` 生成 OpenAPI 枚举值、`into_iter()` 遍历矩阵）零适配——与 `write_op_set!` 的切片改形不同，此处行为保持优先。
5. **`InstrumentType` 对齐 `TransactionKind` 形态**：去 `rename_all = "snake_case"` derive，serde 改手写 impl 骑行 `as_str`/`parse`（双字符串面收敛为一份）；`FromStr` 收口为宏生成的 `parse`（域内无其他 `FromStr` 消费点；e2e 步骤层 4 处 `.parse()` 随迁改调 `InstrumentType::parse`）。5 变体全单词，`snake_case` 展开与 `as_str` 逐字同形——**wire 形状不变**。唯一用户可见差异：未知类型报错文案追加「（合法值: …）」后缀（与 `TransactionKind` 文案同构，grilling 定谳采纳）。
6. **DB CHECK 冻结副本 = 测试期互核兜底**：宏够不到已发布迁移的字面量。新增两域各一道守门测试，从 `sqlite_master` 读回**实际应用的 schema**（非迁移文件文本，ADR-0087 断言强度），断言 `CHECK(<col> IN (...))` 字面量 == `ALL` 的 `as_str` 集，**顺序敏感**（enum 序 = CHECK 序 = OpenAPI `enum_values` 序）；漂移从静默降为测试红。V017 视图的 kind 子集字面量是度量语义（≠ 全集、随发布冻结），等值断言不适用、子集断言增益边际，**不覆盖**。
7. **验收判据对准「契约形状不变」**：既有 API 契约测试（`contract_kind_enum_is_closed_lowercase_set`、`contract_type_expressions_cover_all_forms` 对两闭集的表达式逐字符断言）零改全绿即契约不变；新增 kind 本就走新 migration + 契约演进，本决策不改变该边界。

## 理由

- **保证等级差距仍是选型决定项**（ADR-0102 同款推理）：宏清单即 enum 本体，不存在第二份事实；测试期互核作为主机制会把保证押在「检测器正确」上并永续三处同步义务。模式成本在 #1023 已一次性支付，本票复用为共享宏的边际成本极低。
- **`parse` 报错文案同源生成**：合法值清单从同一批字面量拼接（`[$($str),*].join("/")`），`TransactionKind` 文案逐字不变；清单随变体增减自动跟随，消灭「报错文案里的手抄清单」这一隐藏第五面。
- **`err_label` 宏参数**而非整条文案模板：两域文案形状统一（`未知{label}: {s}（合法值: …）`），差异只在名词，不值得为逐字保留旧文案加模板旋钮。

## 代价

1. 仓库第二个自定义声明宏：宏内文档 rustdoc 不入调用域文档链接（已避免跨展开 intra-doc link）；展开调试依赖 `cargo expand`（清单形态简单）。
2. `InstrumentType` 未知值报错文案变化（追加合法值后缀）——用户可见，已被 grilling 接受为改进。
3. e2e 步骤层 4 处 `.parse()` 改调 `InstrumentType::parse`（`FromStr` 退役的随迁成本）。
4. **与 ADR-0050 的关系（显式记录）**：宏生成的 `parse` 保持既有裸 `AppError::Invalid` 构造（两枚举既状如此，本票不扩大范围）；「Invalid 构造点逐点码化」是 ADR-0050 的存量收口债，宏固化该形状使第三枚举接入时会照抄——已立票单独追踪，不静默延续。**（本句所述裸 `Invalid` 形状已被 #1071 取代**：宏增必填 `err_code` 参数、`parse` 改报码化错误；见文末「实施 recorded · #1071 收口」。）

## 替代方案（防重提）

- **测试期互核作为主机制**（`signals_cross_check` 扫描器具复用）：零新机制，但放弃「新增变体只改一处」，双字符串面漂移只能检测不能消灭。作为宏路线被否决时的保守候补记录在案。
- **维持现状**：`ALL` 漏登静默直达 AI API 契约与 SQL 聚合口径，失败面比 WriteOp 更宽（生产消费方），否决。
- **strum derive（`EnumIter`/`EnumString`）**：构造性保证同宏，但引入新依赖族、`ALL` 从 const 变迭代器、`parse` 错误文案定制受限；ADR-0102 已否决，本票维持。
- **宏迁移 `write_op_set!` 同体复用**：WriteOp 无字符串字面量面（ident-only + `stringify!`），强行合一会让宏吃两套形态；且 ADR-0102 明文「宏定义与调用就地本模块」。两宏并存、模式同源，各有住址。
- **保留 `rename_all` derive + 测试期断言两字符串面同形**：把「恰好同形」从结构巧合降级为测试断言，仍保留两套面；不如收敛为一份。

## 后果

- 新增 `TransactionKind` 变体 = 宏清单加一行 + `coefficient` 矩阵各度量加决策臂（穷尽 match 编译红强制，语义决策行保持显式）；新增 `InstrumentType` 变体 = 宏清单加一行 + 域内穷尽 match 决策臂（如手动创建守卫）。漏登/漏臂失败类整体消亡。
- 代码侧闭集字符串面全部同源；DB CHECK 剩余的冻结字面量有测试互核兜底，漂移必红。
- 落地：#1022（本票单票承载两 enum + 共享宏 + 互核，grilling 定谳不拆分）。

## 实施 recorded

（#1022 落地，2026-09-11；ADR-0102 先例：scratch 变体走查、删除即变红记录。）

- **scratch 变体走查**：宏调用清单暂加 `InstrumentType::Walkthrough => "walkthrough"`——`ALL`/`as_str`/`parse` 由宏同源展开自动带入（无第二处可漏登），`create_instrument_manual` 的穷尽 match 即 `E0004 non-exhaustive patterns`（编译红，证明「新增类型必须过手动创建守卫决策行」的强制面不因宏而弱化）；补臂后 V002 CHECK 互核测试红（CHECK 5 字面量 vs `ALL` 6 项），证明互核对 enum/schema 漂移有牙、非恒绿装饰；删 scratch 回绿。
- **删除即变红（宏接线）**：宏产物被域接缝直接消费——`FromSql`/serde 反序列化消费 `parse`、`ToSql`/`Display`/OpenAPI `enum_values` 消费 `as_str`/`ALL`，删除任一产物即消费点 `E0599` 编译红；「接线即宏本身」，红在编译期。
- 等价性：`cargo test` 全量 1369 项零改全绿（含 API 契约测试对两闭集的逐字符断言），仅 e2e 步骤层 4 处 parse 调用形态随迁；`./scripts/check.sh` 全绿。

### #1071 收口（2026-09-11，代价 4 的存量债结清）

- **宏增必填 `err_code` 参数**（`err_label` 保留）：`parse` 未知值由裸 `AppError::Invalid` 改为 `AppError::codedp`——稳定码 `transaction.kind-unknown` / `instrument.type-unknown`，`params` = `[未知值, 合法值清单]`。必填而非可选，是让「第三枚举接入时照抄裸 Invalid」不可表达（漏参数即编译红）。
- **合法值清单不落前端码表**：zh/en 模板写 `未知交易类型: {0}（合法值: {1}）` / `unknown transaction kind: {0} (valid values: {1})`，清单仍由宏同一批字面量同源拼接、经 `{1}` 插值——ADR-0108「消灭手抄清单」跨本地化边界保持；`message` 与清单逐字不变（ADR-0050 只增不改）。
- **包装路径行为语义保持**：serde 反序列化与 `FromSql` 继续骑行 `parse`（未知值即错，不静默映射）；两者经 serde/rusqlite 错误类型扁平化后只承载 `message`，`code`/`params` 不随之外传——与既有的 `account.type-unknown`（同为 serde 包装的闭集码）同一形态，按 ADR-0050 决策 2 的「构造点码化」口径落地。**已知边界**：新增两码在当前 wire/DB 路径不可达前端；若要经边界直达，须改解包形态（如壳层改收原始字符串后域内 `parse`），超出本票「不改解析行为」边界，建议另立票评估。
- **同族扫描**：`rg "AppError::Invalid\(" src-tauri/src` 剩余存量构造点无既有收口票，清单化立为 #1072（区分用户可见条件候选与 ADR-0050 允许不转的程序性/内部错误）。

### #1072 收口（2026-09-11，同族扫描结清）

- **转码化（8 条件 / 9 构造点，`message` 逐字保留、动态值进 `params`）**：
  - `budget.period-unknown`（`budget/model.rs`，`BudgetPeriod` 闭集解析）；
  - `scheduled-plan.kind-unknown` / `scheduled-plan.status-unknown` / `scheduled-plan.recurrence-unknown` / `scheduled-occurrence.status-unknown`（`scheduled_transactions/models.rs` 四处 `FromStr` 闭集解析）；
  - `scheduled-plan.occurrence-date-invalid`（`scheduled_transactions/engine.rs` 两处期次日期守卫——`recurrence_day` 未在入口校验，0 日等非法值落在 `from_ymd_opt` 的 None 分支，是用户可达条件）；
  - `instrument.query-required`（`api_server/handlers/instruments.rs` 标的搜索缺 `query`，AI 导入按码自纠的 HTTP 入参条件）；
  - `db.integrity-check-failed`（`db/mod.rs` `check_integrity`）。**码归 `db.*` 而非票面猜测的 `boot.*`**：构造点在基础设施，启动引导、备份恢复校验、同步 checkpoint 重建三域共用，按启动场景命名会把后两者的同名失败误标为启动条件；启动路径经 `BootFailureGate` 记录该码，失败恢复屏仍按「库不可读」呈现（只有 `boot.schema-drift` 走恢复优先排布）。
- **票面候选中判定保留裸 `Invalid` 的两点（ADR-0050 决策 2「程序性/内部错误可不转」的逐点裁定，非票面预授权）**：
  - `scheduled_transactions/engine.rs` 的 `未知周期类型` 防御臂：`recurrence_type` 列只由 `RecurrenceType` 闭集写入，闭集外的值只可能来自外部改库——判「内部不一致」；同一条件的稳定码已归 `RecurrenceType` 解析边界（`scheduled-plan.recurrence-unknown`），此处另立码会让一条条件长出两个码。
  - `api_server/handlers/categories.rs` 的请求体反序列化失败：`message` 是 JSON 解析器的技术错误原文（英文、位置相关），没有可逐字保留的中文模板——判「程序性输入格式错误」。同族先例：其余 handler 的 `Json<T>` / `Query<T>` extractor 拒绝（如标的搜索非法 `type`）走框架默认 400 体、同样无码不经 `AppError`，本点与之一致；同族的 sync_engine 序列化失败亦在同票排除清单。
- **测试层归口（对 ADR-0050 决策 6 的显式例外，先例 #1071）**：决策 6 把「错误契约的字段断言」归 `src-tauri/tests/api_server/` 集成测试。本票 8 条码的可达面分三类：`instrument.query-required` 走 HTTP 面——已在 `tests/api_server/instrument_search.rs` 断言码、`message` 与 `params` 缺席（决策 6 正例）；5 条闭集解析（`budget.period-unknown` + `scheduled-plan` 三条 + `scheduled-occurrence.status-unknown`）只经 rusqlite/serde 扁平化，`code`/`params` 不随之外传，HTTP/IPC 面都无从断言（与 `account.type-unknown` 同形的已知边界）；`scheduled-plan.occurrence-date-invalid`（计划建档 IPC）与 `db.integrity-check-failed`（失败恢复 / 备份恢复 IPC）虽经 IPC 上抛，但两者都不经过 HTTP 面，且命令面集成测试现无同类归口。故其余 7 条按 #1071 的构造点口径在域/基础设施单测钉码形态：`scheduled_transactions/tests/parse_codes.rs`、`budget/tests.rs`、`db/tests/integrity.rs`（`PRAGMA writable_schema` 造非 `ok` 结果，断言实际 pragma 输出），zh/en 模板插值归 Vitest `src/__tests__/errors-adr-0050-sweep.test.ts`；两条 IPC 可达码的壳层断言若需要，归后续命令面集成测试票。
- **`db.*` 命名空间说明**：`db.integrity-check-failed` 与系统通用码 `db.error` 同前缀。CONTEXT-core（错误码词条）只约定 `Db`/`Parse`/`Io` **系统错误**携带通用码、底层驱动消息不入码表，未禁止 db 层条件码；构造点在 `db/mod.rs` 基础设施、启动引导/备份恢复/同步 checkpoint 三域共用，沿用 `db` 前缀而不新造域。
