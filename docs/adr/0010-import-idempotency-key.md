# ADR 0010: 批量导入以客户端幂等键为主，内容哈希降为兜底并新增交易修改 API

- 状态：已接受
- 日期：2026-08-23
- 作者：Oz / Ledger 项目

## 背景

`POST /api/v1/transactions/batch` 原先用内容哈希 `dedup_hash = sha256(date|kind|amount_cents|currency_code|account_id|to_account_id)` 做幂等去重（排除 note/category，因 AI 文本非确定性会导致哈希漂移）。这带来两个结构性问题：

1. **"重跑幂等"与"不吃掉雷同真实交易"无法两全。** 内容哈希回答的是"这条交易内容长相如何"，而非"这是否同一批已导入的记录"。同一天、同账户、同金额但确属不同的两笔交易（尤其 note/category 不同的），会被误判为重复而丢弃；加 note 只能收窄 90%，剩下 note 也相同的边缘情况仍被吞，且会重新引入哈希漂移（重跑时 note 变一字就匹配不上从而导致重复）。
2. **buy/sell 存在结构性盲区。** 哈希字段集不含 `instrument_id`/`quantity`/`price_cents`/`fee_cents`，而 buy/sell 的 `amount_cents` 为 0，于是同一账户、同一天、同一币种的两笔不同标的买入/卖出会被误判为重复。

同时，批量导入已成为常用功能（降低人工记账负担），纠错闭环原先是"软删→重导"（`AICleanupDeletion`），重导非原子、且依赖"删得准"来避免重复。

## 决策

1. **去重身份改为内容无关的客户端幂等键 `idempotency_key`。** 每条 `TransactionInput` 新增可选 `idempotency_key`（客户端提供，指向"这条交易来自源文件的哪一行"）。带键时，去重以幂等键为准：命中已存在的未删除交易 → 跳过并返回 `duplicate`，与内容无关。一次源行拆多笔时，客户端派生"源文件:行号:交易序号"的独立键。
2. **内容哈希保留为无键行的兜底（冻结契约，只增不改）。** 不带 `idempotency_key` 的行仍走原 `dedup_hash` 内容哈希去重；目标是通过更新提示词，让新导入一律带键，使内容哈希退化为历史兼容路径。
3. **纠错改为新增交易修改 API。** 新增 `PUT /api/v1/transactions/{id}`（按 id 全字段替换），`idempotency_key` 不可编辑。纠错从"软删→重导"改为"按 id 修改"，避免重导覆盖界面手动编辑，也不产生重复。

## 理由

1. **幂等键把"身份"与"内容"解耦。** 重跑同一批（同键）→ 跳过；文件内两笔内容完全相同的独立交易（不同键）→ 都保留。二者兼得，而内容哈希只能二选一。
2. **幂等键内容无关，编辑不使身份漂移。** 配合修改 API，改任意字段都不影响去重身份，无需担心哈希漂移或"改了就匹配不上"。
3. **跳过而非 upsert。** upsert 会在重跑时覆盖界面手动编辑，且模糊"导入"与"编辑"的边界；纠错职责交给独立的修改 API 更清晰。
4. **契合冻结契约。** `dedup`/`dedup_hash`/`transactions/batch` 均已在 v0.2.0 发布，只能"只增不改"。新增可选字段、新增端点、新增前向迁移均属允许范围。

## 代价

1. **幂等键唯一性依赖客户端。** 服务端用部分唯一索引兜底（"一键至多一活交易"），但不替客户端生成键，需在提示词中强制"一律带键、键唯一"。
2. **内容哈希兜底仍是旧行为。** buy/sell 盲区与"雷同交易被吞"仍在无键行发生，需依靠"新导入一律带键"绕开，并在文档注明为历史局限。
3. **实现面增大。** 需新增迁移、模型字段、去重分支、修改端点、两处提示词与 CONTEXT/ADR 同步，并补测试。

## 替代方案

- **内容哈希 + 加入 note**：仅缓解，不根治（note 相同时仍吞），且重新引入哈希漂移，放弃。
- **批导入 upsert**：覆盖手动编辑、模糊边界，放弃。
- **只靠"软删→重导"纠错**：非原子、需"删得准"，且内容哈希仍会吞雷同交易，放弃。
- **移除内容哈希、强制要求幂等键**：因契约已冻结（v0.2.0 含 `dedup_hash`/batch），不可移除，故仅在无键行为兜底路径保留。

## 影响

- 新增前向迁移 `V007__transaction_idempotency_key.sql`：`transactions` 加 `idempotency_key TEXT` 列，建部分唯一索引 `CREATE UNIQUE INDEX ... ON transactions(idempotency_key) WHERE idempotency_key IS NOT NULL AND is_deleted=0`。
- `src/models.rs`：`TransactionInput` 加可选 `idempotency_key`；`CreateTransactionResult` 在 duplicate 分支返回已有 `id`（更丰富，不破坏现有调用）。
- `src/commands/transactions.rs`：去重逻辑分支——有键按键查、无键走 `compute_dedup_hash`；新增 `update_transaction_internal`。
- `src/api_server.rs`：新增 `PUT /api/v1/transactions/{id}` 端点；更新 openapi 描述。
- `src-tauri/prompts/ledger-api.md`、`import-knowledge.md`：批量导入一律带 `idempotency_key`；纠错用修改 API 而非"删后重导"。
- `CONTEXT.md`：新增 `IdempotencyKey`、`AICleanupModify`，修订 `ImportDedup`，同步 `AI API`。
- 前端 `src/types/index.ts`：加 `idempotency_key`；IPC `create_transactions` 仍为 `dedup=false` 不变。
- 测试：BDD 补"同键重跑跳过""同键不同内容仍跳过（内容无关）""不同键雷同内容都保留""按 id 修改后重跑不重复"等场景。
