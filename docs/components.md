# 前端组件目录

## 组件结构

```
src/
  components/
    TransactionForm.vue      # 交易表单入口（含 6 种交易类型切换）
    CategoryForm.vue          # 普通收支表单（income / expense）
    TransferForm.vue          # 转账表单
    RefundForm.vue            # 退款表单
    InvestmentForm.vue        # 投资表单（buy / sell）
```

所有表单子组件均通过 `TransactionFormContext` 接口与 `useTransactionForm` composable 通信，而非独立管理各自状态。

---

## TransactionForm.vue

**交易表单根组件**。通过单选按钮切换交易类型，按类型渲染对应子表单。

**Props**：
| 名称 | 类型 | 说明 |
|------|------|------|
| `onCreated?` | `() => void` | 交易创建成功后的回调 |

**Emits**：
| 名称 | 说明 |
|------|------|
| `created` | 交易创建成功 |

**类型切换逻辑**：
- `expense` / `income` → `CategoryForm`
- `transfer` → `TransferForm`
- `refund` → `RefundForm`
- `buy` / `sell` → `InvestmentForm`

**路径**：`src/components/TransactionForm.vue`

---

## CategoryForm.vue

**收支表单**。用于 `income` 和 `expense` 类型交易。

**Props**：
| 名称 | 类型 | 说明 |
|------|------|------|
| `ctx` | `TransactionFormContext` | 表单上下文 |
| `kind` | `'income' \| 'expense'` | 交易类型 |
| `submitLabel` | `string` | 提交按钮文本 |

**包含字段**：
- 金额（`NInputNumber` + `NSelect` 选择币种）
- 账户（`NSelect`）
- 分类（`NTreeSelect`，支持过滤）
- 日期（`NDatePicker`）
- 备注（`NInput`）

**路径**：`src/components/CategoryForm.vue`

---

## TransferForm.vue

**转账表单**。用于 `transfer` 类型交易。

**Props**：
| 名称 | 类型 | 说明 |
|------|------|------|
| `ctx` | `TransactionFormContext` | 表单上下文 |

**包含字段**：
- 金额（`NInputNumber` + `NSelect` 选择币种）
- 转出账户（`NSelect`）
- 转入账户（`NSelect`）
- 日期（`NDatePicker`）
- 备注（`NInput`）

**路径**：`src/components/TransferForm.vue`

---

## RefundForm.vue

**退款表单**。用于 `refund` 类型交易。必须先选择被退款的原始支出交易，自动填充账户和币种。

**Props**：
| 名称 | 类型 | 说明 |
|------|------|------|
| `ctx` | `TransactionFormContext` | 表单上下文 |

**包含字段**：
- 退款关联（`NSelect`，从已有 expense 交易中选择，显示日期/金额/分类/备注）
- 原交易信息（`NText`，只读显示原交易详情）
- 退款金额（`NInputNumber` + 币种 `NSelect` 禁用）
- 账户（`NSelect` 禁用，由原交易决定）
- 日期（`NDatePicker`）
- 备注（`NInput`）

**路径**：`src/components/RefundForm.vue`

---

## InvestmentForm.vue

**投资表单**。用于 `buy` 和 `sell` 类型交易。金额字段自动计算（只读）。

**Props**：
| 名称 | 类型 | 说明 |
|------|------|------|
| `ctx` | `TransactionFormContext` | 表单上下文 |
| `kind` | `'buy' \| 'sell'` | 交易类型 |
| `submitLabel` | `string` | 提交按钮文本 |

**包含字段**：
- 金额（`NInputNumber` 禁用，自动计算 `数量×单价±手续费`）
- 币种（`NSelect` 禁用，由选择的投资账户决定）
- 投资账户（`NSelect`，仅显示 `type='investment'` 的账户）
- 标的（`NSelect` 支持过滤 + `新增标的` 按钮）
- 新建标的弹出区（`NSpace` 条件渲染）：
  - 代码（`NInput`）
  - 名称（`NInput`）
  - 类型（`NSelect`：股票/基金/债券/ETF/其他）
- 数量（`NInputNumber`，precision 4）
- 单价（`NInputNumber`，precision 2）
- 手续费（`NInputNumber`，precision 2，可选）
- 日期（`NDatePicker`）
- 备注（`NInput`）

**金额自动计算**：
- 买入：`数量 × 单价 + 手续费`
- 卖出：`数量 × 单价 - 手续费`

**路径**：`src/components/InvestmentForm.vue`

---

## useTransactionForm composable

**表单状态管理**。集中管理所有交易类型的表单状态和提交逻辑。

**路径**：`src/composables/useTransactionForm.ts`

**暴露的状态**：
| 名称 | 类型 | 说明 |
|------|------|------|
| `kind` | `Ref<TransactionKind>` | 当前选中的交易类型 |
| `amount` | `Ref<number \| null>` | 金额（元） |
| `currencyCode` | `Ref<string>` | 币种代码 |
| `accountId` | `Ref<string \| null>` | 账户 ID |
| `toAccountId` | `Ref<string \| null>` | 转账目标账户 ID |
| `categoryId` | `Ref<string \| null>` | 分类 ID |
| `refundTargetId` | `Ref<string \| null>` | 退款目标交易 ID |
| `note` | `Ref<string>` | 备注 |
| `date` | `Ref<number>` | 日期（时间戳） |
| `instrumentId` | `Ref<string \| null>` | 投资标的 ID |
| `quantity` | `Ref<number \| null>` | 投资数量 |
| `price` | `Ref<number \| null>` | 投资单价 |
| `fee` | `Ref<number \| null>` | 手续费 |
| `instruments` | `Ref<Instrument[]>` | 可用标的列表 |
| `showNewInstrument` | `Ref<boolean>` | 是否显示新建标的表单 |
| `transactions` | `Ref<Transaction[]>` | 交易列表（用于退款关联选择） |

**暴露的计算属性**：
| 名称 | 说明 |
|------|------|
| `accountOptions` | 所有账户的 select 选项 |
| `investmentAccountOptions` | 仅投资账户的 select 选项 |
| `instrumentOptions` | 标的 select 选项（显示 `symbol · name`） |
| `currencyOptions` | 币种 select 选项 |
| `treeOptions` | 分类树选项（按 `kind` 过滤） |
| `expenseTransactions` | 仅 expense 类型的交易 |
| `refundTargetOptions` | 退款目标 select 选项 |
| `refundTarget` | 当前选中的退款目标交易 |
| `isInvestmentTransaction` | 当前是否为 buy/sell |
| `investmentAmount` | 自动计算的投资金额（元） |

**关键方法**：
- `submit()` — 提交交易。按 `kind` 走不同校验和提交逻辑：buy/sell 调 `createTransaction` 传 `instrument_id/quantity/price_cents/fee_cents`；普通交易传标准字段。
- `createNewInstrument()` — 调用 `api.createInstrument` 创建新标的并刷新列表
- `loadInstruments()` / `loadTransactions()` — 加载标的 / 交易列表
- `resetForm()` — 重置所有表单字段

**副作用**：
- 参考数据由 `useReferenceStore` self-init + `ledger:changed` 信号兜底（不再手工 `loadAll`）；交易列表 / 标的数据按需加载
- `watch(kind)` — 切换类型时重置相关字段
- `watch(accountId)` — buy/sell 时自动更新币种为账户币种
- `watch(refundTargetId)` — 自动填充账户和币种

---

## 相关 store

### `src/stores/app.ts` — `useAppStore`

纯 UI 设置 store（本地持久化）：主题 / 默认币种 / 备份设置。参考数据（`currencies / accounts / categories`）及全部派生映射已迁至 `useReferenceStore`（见 `src/stores/reference.ts`），本 store 不再暴露参考数据接口（issue #85）。

| 名称 | 类型 | 说明 |
|------|------|------|
| `theme` | `ref<'dark' \| 'light'>` | 主题（默认 `dark`） |
| `defaultCurrency` | `ref<string>` | 默认币种代码（默认 `CNY`） |
| `backupDir` | `ref<string>` | 备份目录（默认空） |
| `backupMaxCount` | `ref<number>` | 受管备份保留上限（默认 30） |
| `setTheme(t)` | `function` | 切换主题并持久化到 localStorage |
| `setDefaultCurrency(code)` | `function` | 设置默认币种并持久化 |
| `setBackupDir(dir)` | `function` | 设置备份目录并持久化 |
| `setBackupMaxCount(n)` | `function` | 设置保留上限并持久化 |

**初始化**：值从 localStorage 恢复；参考数据由 `useReferenceStore` 首次访问 self-init 自动加载，无需本 store 参与。

### `src/stores/reference.ts` — `useReferenceStore`

参考数据（Reference Data）单一来源 store：持有 `currencies / accounts / categories` 三张参考表与全部派生映射（`currencyMap / accountMap / categoryMap`）、分类树逻辑（`rootCategories / expenseCategories / incomeCategories / categoryChildren / categoryPath / treeCategoryOptions`）与失效信号（`status / version`）。加载为 push 生命周期：首次访问 self-init 自动拉取，订阅后端 `ledger:changed` 信号自动重拉（stale-while-revalidate，不闪空）；显式控制用 `refresh()`（强制重拉，在途去重）与 `ensureFresh()`（新鲜窗口内零 IPC）。
