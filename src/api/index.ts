import { invoke } from '@tauri-apps/api/core'
import type {
  Account,
  AccountBalance,
  AccountInput,
  Budget,
  BudgetInput,
  BudgetProgress,
  Category,
  CategoryInput,
  CategoryShare,
  Currency,
  ImportedRow,
  MonthlySummary,
  Transaction,
  TransactionInput,
} from '@/types'

export const api = {
  // 币种
  listCurrencies: () => invoke<Currency[]>('list_currencies'),

  // 账户
  listAccounts: () => invoke<Account[]>('list_accounts'),
  createAccount: (input: AccountInput) => invoke<string>('create_account', { input }),
  deleteAccount: (id: string) => invoke<void>('delete_account', { id }),
  listAccountBalances: () => invoke<AccountBalance[]>('list_account_balances'),

  // 分类
  listCategories: () => invoke<Category[]>('list_categories'),
  createCategory: (input: CategoryInput) => invoke<string>('create_category', { input }),
  deleteCategory: (id: string) => invoke<void>('delete_category', { id }),

  // 交易
  listTransactions: (limit?: number) =>
    invoke<Transaction[]>('list_transactions', { limit: limit ?? null }),
  createTransaction: (input: TransactionInput) =>
    invoke<string>('create_transaction', { input }),
  deleteTransaction: (id: string) => invoke<void>('delete_transaction', { id }),

  // 预算
  listBudgets: () => invoke<Budget[]>('list_budgets'),
  createBudget: (input: BudgetInput) => invoke<string>('create_budget', { input }),
  deleteBudget: (id: string) => invoke<void>('delete_budget', { id }),

  // 报表
  monthlySummary: (year: number) => invoke<MonthlySummary[]>('monthly_summary', { year }),
  categoryShares: (kind: string, month?: string) =>
    invoke<CategoryShare[]>('category_shares', { kind, month: month ?? null }),
  budgetProgress: () => invoke<BudgetProgress[]>('budget_progress'),

  // 导入
  previewImport: (path: string) =>
    invoke<ImportedRow[]>('preview_import', { req: { path } }),
}
