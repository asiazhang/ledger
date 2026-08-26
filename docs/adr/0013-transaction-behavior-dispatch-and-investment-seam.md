# ADR 0013: 交易类型行为收敛分派 + investment 出口收窄为 prepare/apply/revert

- 状态：已接受
- 日期：2026-08-26
- 作者：Ledger 项目

## 背景

交易写入路径在行为层上没有单一权威（spec #69 候选 2）：三类 kind 行为——校验、归一化、应用副作用（buy 建仓 / sell 卖出匹配）、回退（buy 清理 / sell 回补）——的 `match kind` 散落在三处：

1. `insert_transaction`（创建：buy/sell 转发投资层，其余走通用 writer）；
2. `update_transaction_internal`（修改：先按旧 kind 清理/回补，再按新 kind 校验并应用）；
3. `delete_transaction_internal`（删除：按 `is_buy` 决定是否清理持仓）。

同时 `commands::investment` 对外暴露 9 个 buy/sell 写函数（create_buy/create_sell/apply_buy/apply_sell/cleanup_buy/cleanup_buy_side_effects/reverse_sell/prepare_buy/prepare_sell），耦合面宽；`dividend` / `split` 已声明但未实现，经交易接口创建落入 `writer::normalize` 的通用兜底、返回语义不明的「仅处理通用交易类型」报错，无显式「暂不支持」。

本决策（issue #72，承接 issue #70 的 Writer 接缝）把 kind 行为收敛到单一分派层，并把 investment 出口收窄为 `prepare / apply / revert` 三件套。

## 决策

1. **行为层 = `commands::transactions::behavior`，单点分派全部 8 种 kind**。对外暴露 `plan`（normalize→plan，校验 + 归一化，不落库、不产生副作用）、`apply`（应用副作用）、`revert`（按旧 kind 回退副作用）三个能力；创建路径 `plan → insert_row → apply`，修改路径 `revert(旧 kind) → plan(新 kind) → update_row → apply`，删除路径仅 buy 走 `revert`（sell 删除不清理持仓关联，既有行为保持不变）。
2. **分派用薄而穷尽的 `match`，不做 trait 注册表**。普通 kind（income/expense/transfer/refund）经 Writer 接缝 `writer::normalize`；buy/sell 委托投资域（正向分派保留——本就是 kind 语义的正确归属）；`dividend` / `split` 显式 `Invalid("交易类型 {kind} 暂不支持（MVP 未实现）")` 拒绝。不引入 trait-object 注册表：8 种 kind 是静态闭集，注册表只增加间接层而无扩展收益。
3. **investment 对外出口收窄为 `prepare / apply / revert` 三件套**。`prepare` 校验并归一化 buy/sell 输入产出 `Plan`（不落库）；`apply` 应用副作用（buy 建仓 / sell 卖出匹配）；`revert` 回退副作用（buy 守卫+清理 / sell 回补）。lot / 匹配 / pnl 数据逻辑仍留在 investment（投资域概念，物理不搬迁）；交易行 INSERT/UPDATE 一律经 `transaction::writer` 接缝，行为函数与 writer 只接受连接、不内嵌事务——事务边界由调用方持有（修改/批量路径在编排层显式 BEGIN/COMMIT），在这些事务内买卖的行写入与 lot/匹配副作用同处一个事务。
4. **`dividend` / `split` 显式「暂不支持」**。这是本重构唯一对外的可观测行为变化：此前经创建/修改交易接口返回 `writer::normalize` 兜底的「仅处理通用交易类型」（语义不明），现在返回明确的「暂不支持」错误；两者均不落库（不落库行为不变）。实现完整分红/拆股行为（拆股调整 lot、分红现金与盈亏）明确暂缓。
5. **writer::normalize 保留对非通用 kind 的防御性拒绝**（仅直接误用可达；行为层在到达 writer 前已分派），writer 接缝职责不变。

## 理由

1. **行为单点可达**。新增一类 kind 只改行为层一处 match；「每类 kind 的校验、归一化与副作用在一个位置可达」使因果在单一位置可读，消除了三处散落 `match kind` 的漂移风险（如删除路径只清 buy 不清 sell 的隐性不一致）。
2. **收窄出口面 = 收窄契约面**。investment 的 9 个写函数收窄为 3 个，耦合面最小且清晰：调用方只关心「校验归一化 / 应用 / 回退」三个时机，不必知道 create 与 update 的差异细节；配合共享 writer，investment 不再反向依赖 transactions 的行更新函数（双向依赖在 issue #70 已斩断，本决策把正向调用也收成三件套）。
3. **薄 match 优于 trait 注册表**。8 种 kind 是编译期闭集、无第三方扩展点；trait-object 注册表只增加间接层（动态分派、注册仪式、测试更绕）而无扩展收益。薄 match 穷尽性由编译器保证，新增 kind 漏分派即编译错误。
4. **显式拒绝优于语义不明的兜底**。dividend/split 已声明（CHECK 约束、度量矩阵、枚举）但未实现，此前经交易接口创建落入 `writer::normalize` 的通用兜底（「仅处理通用交易类型」），报错语义不明；显式「暂不支持」让用户/AI 编程助手立刻知道能力边界，也为后续实现留出明确位置（在行为层 match 内替换拒绝分支）。

## 代价

1. **行为层与投资域之间多一层计划类型**（`behavior::Plan` / `investment::Plan`），创建/修改路径需两次归一化行转换（`NormalizedTransaction` ↔ `writer::NormalizedRow`），多一层薄转换。
2. **investment 内部保留 prepare_buy/prepare_sell 等私有函数**，出口收窄不改变内部结构，纯函数测试仍以行为层公开入口（`insert_transaction` / `update_transaction_internal`）断言外部行为。
3. **唯一对外的可观测变化是 dividend/split 的报错信息**，已有依赖旧报错文案的外部脚本（若有）需跟随更新——BDD/HTTP seam 测试已锁定新文案。

## 替代方案

- **trait-object 注册表**（`trait TransactionBehavior { fn plan/apply/revert }` + 注册表）：动态分派 + 注册仪式，8 种静态闭集 kind 下无扩展收益，且测试更难直接断言穷尽性，放弃。
- **在 `transaction` 领域模块内做行为分派**：行为层需反向依赖命令层（investment），破坏「命令层 → transaction」的依赖方向，放弃。
- **保留 investment 的 9 个出口、仅收拢调用点**：散落分支虽集中但耦合面不变，验收标准「investment 对外仅暴露 prepare/apply/revert」不满足，放弃。
