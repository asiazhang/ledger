# 投资交易 UI 需求

> 记录投资交易前后端录入功能的完整需求。当前投资 schema 已落地（`instruments`、`security_transactions`、`security_lots` 等），但用户仍无法通过界面录入买入、卖出、现金分红等交易。

## 问题

`src/components/TransactionForm.vue` 当前只支持 `expense/income/transfer/refund`，没有 `buy/sell/dividend` 类型。投资功能目前只停留在 schema 层，用户无法录入。

## 优化方向

1. 在交易表单中增加“投资”相关 kind 选择。  
2. 选择 `buy/sell` 时显示：账户、标的（instrument）、数量、单价、手续费、币种、日期。  
3. 选择 `dividend` 时显示：账户、标的、分红金额。  
4. 后端需要新增 `create_security_transaction` 命令，同时写入 `transactions` 和 `security_transactions`，并在买入时创建 `security_lots`。

## 关联

- `src/components/TransactionForm.vue`
- `src-tauri/src/commands.rs`
- `src-tauri/migrations/V002__investment.sql`
