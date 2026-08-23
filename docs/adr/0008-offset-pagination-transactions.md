# ADR-0008: 交易列表服务端分页采用 offset 页码而非游标

- 状态：已接受
- 日期：2026-08-23
- 作者：Ledger 项目

## 背景

交易页（交易流水列表）此前一次性把全部交易加载进内存，分页只是前端切片展示（每页 20 条）。个人账本运行几年后交易上万条，整表 `SELECT` + 全量 IPC 传输成为真实瓶颈：首屏变慢、内存占用上升；分页控件也没有总数显示与页大小选择。

需要把交易查询改为服务端分页：一次只返回当前页交易 + 满足过滤条件的总条数。查询应支持 `page`/`page_size`、日期/账户/类型过滤、确定性排序；IPC 与 HTTP（AI 读回）共用同一套查询实现，缺省行为（不传分页参数返回全部）保持不变，不破坏现有调用方与 AI 导入读回流程。

分页有两条技术路线：**offset 页码分页**（`page`/`page_size` + `LIMIT/OFFSET`）与**游标分页**（cursor / keyset，以排序键为锚点取下一页）。

## 决策

交易列表采用 **offset 页码分页**：

- 查询模型：`TransactionListFilter{from, to, account_id, kind, page, page_size, limit}` 与 `TransactionListResult{items, total}`，字段命名与标的列表（`InstrumentListFilter/InstrumentListResult`）先例对齐；
- 分页语义：`page` 从 1 开始，缺省 1；`page_size` 缺省时**返回全部**（`total` 仍返回）；`limit` 作为独立"取前 N 条"参数保留（仪表盘"最近 N 条"场景），传 `page_size` 时分页路径生效；
- `total` 口径恒为"满足过滤条件的未删除交易总数"，与 `items` 共用同一 WHERE 子句；
- 确定性排序：`ORDER BY date DESC, created_at DESC, id DESC`。`id` 是最终 tiebreaker——同一批导入的交易 `created_at` 相同（每批一个时间戳），不加 id 翻页会漂移；
- IPC 命令与 HTTP `GET /api/v1/transactions` 共用 `list_transactions_internal` 同一实现，只有一套分页语义；HTTP 响应从裸数组改为 `{items, total}`，新增 `page`/`page_size` 查询参数，OpenAPI 文档同步。

## 理由

1. **简单性**：offset 分页实现直观，无需维护游标状态；排序键只有"按日期倒序"一种形态，不需要为多列排序键构造编码/解码。
2. **支持跳页与总数**：产品需求明确要求快速跳转到任意页（user story 4）与总数显示（user story 2）；`total` 同时支撑 AI 读回对账"全部落库"判定与分页条展示。cursor 分页天然无法跳页，总数只能额外 `COUNT(*)` 补齐。
3. **与标的列表先例对齐**：投资标的列表已采用 `{items, total}` + `page/page_size` 的 offset 模式（`InstrumentListFilter/InstrumentListResult`），交易查询在字段命名、结果结构与分页语义上对齐，调用方只需一套心智模型。
   - 唯一有意差异是缺省行为：标的 `page_size` 缺省 50 条，交易缺省**返回全部**——由理由 #5（AI 读回与现有调用方零迁移）决定，属有意设计而非疏漏。
4. **性能在目标规模内无感**：MVP 个人账本规模（几万条封顶），`OFFSET` 深分页的线性扫描代价可忽略；`date DESC` 走索引，最坏情况是翻到末页。
5. **缺省返回全部 + total** 保证 AI 读回与现有调用方零迁移成本，是"接口一次设计好"与"不破坏现状"的折中点。

## 代价

1. **翻页漂移是已知行为**：分页浏览期间新增交易会使列表整体前移，后续页可能出现条目重复或遗漏。这是 offset 分页的固有属性，不视为数据错误，已在 CONTEXT.md 的 Transaction 条目中记录为已知边界。
2. **深分页性能**：`OFFSET` 越大扫描越多。目标规模内可接受；若未来数据量级显著增长，可再评估 keyset 分页或按日期区间分段读回。
3. **`total` 与 `items` 需要两次查询**：每次分页取数都要额外执行一次 `COUNT(*)`。代价在几万条规模下无感。

## 替代方案

- **游标分页（cursor / keyset）**：以 `(date, created_at, id)` 组合键为锚点，只返回"严格晚于锚点"的下一页。
  - 优点：数据变更时翻页稳定（新交易前置不漂移）、深分页性能恒定；
  - 缺点：无法任意跳页、总数需要额外查询、实现与编码复杂（多列键）、与标的列表先例不一致；
  - 结论：否决。MVP 规模下 offset 的性能短板不成立，而跳页与总数是明确产品需求。

## 影响

- 交易页前端切换为 remote 分页模式，提供总数显示与页大小选择器（10/20/50/100，默认 20），不持久化页大小（遵守 ViewState"不做过度记忆"决策）。
- 仪表盘"最近 N 条"与退款表单取数改为从 `items` 取值，语义不变。
- AI 读回：`GET /api/v1/transactions` 改为取 `.items`（缺省返回全部），`ledger-api.md` 提示词同步。
- 与 ADR-0004（交易搜索）的分页决策一致：搜索命令同样采用 `LIMIT/OFFSET` + 命中总数。

## 参考

- issue #30（父 spec）：交易列表服务端分页（items + total，offset 页码模式）
- issue #32（T1）：服务端分页查询与全链路契约替换
- `src-tauri/src/commands/investment/`（`InstrumentListFilter/InstrumentListResult` 先例）
