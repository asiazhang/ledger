# Tauri IPC API 文档

所有 Tauri 命令通过 `@tauri-apps/api/core` 的 `invoke` 调用，前端统一封装在 `src/api/index.ts` 的 `api` 对象中。

## 领域索引

| 领域 | 文件 | 命令 |
|------|------|------|
| 币种 | [currencies.md](./currencies.md) | `list_currencies` |
| 账户 | [accounts.md](./accounts.md) | `list_accounts`, `create_account`, `delete_account`, `list_account_balances` |
| 预算 | [budget.md](./budget.md) | `list_budgets`, `create_budget`, `delete_budget`, `budget_progress` |
| 报表 | [reports.md](./reports.md) | `monthly_summary`, `category_shares` |
| 错误处理 | [errors.md](./errors.md) | — |

## 新增命令流程

新增后端命令时必须同步四处：

1. `src-tauri/src/commands/` 下对应 `.rs` 文件加 `#[tauri::command]` 函数
2. `src-tauri/src/lib.rs` 的 `generate_handler!` 宏中注册
3. `src/api/index.ts` 的 `api` 对象上加对应方法
4. 本目录下对应领域文件加文档
