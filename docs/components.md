# 前端组件与 composable 职责

本文档**不罗列**组件 props / emits / 状态字段 / 方法签名——这些可枚举信息以代码为唯一事实来源，直接读组件 `.vue` 与 `src/composables/` 即可。本文档只回答代码里读不出来的问题：各部分的职责边界与协作方式。

## 交易表单

### 入口分发

`src/components/TransactionForm.vue` 是交易表单根组件：用单选按钮切换交易类型，按类型渲染对应子表单：

| 类型 | 子表单 |
|------|--------|
| `expense` / `income` | `CategoryForm.vue` |
| `transfer` | `TransferForm.vue` |
| `refund` | `RefundForm.vue` |
| `buy` / `sell` | `InvestmentForm.vue` |

子表单各自通过同名 composable 管理状态与提交逻辑，**没有共享的 TransactionFormContext / useTransactionForm**（早前版本的集中式设计已被拆分取代）：

- `src/composables/useCategoryForm.ts` ↔ `CategoryForm.vue`
- `src/composables/useTransferForm.ts` ↔ `TransferForm.vue`
- `src/composables/useRefundForm.ts` ↔ `RefundForm.vue`
- `src/composables/useInvestmentForm.ts` ↔ `InvestmentForm.vue`

### 职责边界

- **表单 composable 与组件一一对应**：组件只负责 Naive UI 绑定与布局，状态、校验、`submit()`（构造 `TransactionInput` → `api.createTransaction`）都在 composable 里。
- **金额口径**：前端输入以「元」为单位（`NInputNumber`），提交时 `Math.round(amount * 100)` 转分；展示一律走 `formatAmount(cents, currency)`（`src/utils/money.ts`，经 `@/types` 转出），**不要手写 `/100`**。
- **参考数据**：表单的账户/币种选项来自 `useFormShared`（包装 `useReferenceStore` 的派生映射）；`refund` 表单的账户/币种**自动继承原支出交易**（选择退款目标后填充），分类选项用 `treeCategoryOptions(kind)`。
- **投资表单**：`InvestmentForm` 的金额为只读自动计算（买入 `数量×单价+手续费`，卖出 `数量×单价−手续费`），币种禁用（由投资账户决定）；买入/卖出提交 `amount_cents: 0` + 投资字段（`instrument_id/quantity/price_cents/fee_cents`），金额实际由后端投资域 `prepare`（内部 `prepare_buy`/`prepare_sell`）计算并校验（数量/单价 > 0、投资账户、可卖数量充足），经行为层单点分派（issue #72）。
- **提交语义**：`submit()` 成功后清空表单并通过 `onCreated` 回调通知父组件（如交易列表刷新）；新建标的（`createNewInstrument`）走 `api.createInstrument` 后回查列表。

## 参考数据（单一来源）

- `src/stores/reference.ts` — `useReferenceStore`：`currencies / accounts / categories` 三张参考表 + 派生映射（`currencyMap / accountMap / categoryMap`）+ 分类树逻辑（`rootCategories / expenseCategories / incomeCategories / categoryChildren / categoryPath / treeCategoryOptions`）+ 失效信号（`status` / `version`）。
- 生命周期为 **push-first**：首次访问 self-init 拉取一次；订阅后端 `ledger:changed` 自动重拉（stale-while-revalidate，不闪空）；显式控制用 `refresh()`（强制，在途去重）与 `ensureFresh()`（新鲜窗口内零 IPC）。
- **消费约定**：所有视图/组件不再手工 `loadAll()`；账户/分类管理、交易/搜索、报表/预算/投资/设置各流一律从本 store 读取并依赖信号刷新（issue #78–#85）。派生映射为 computed，随数组自动更新。
- `src/stores/app.ts` — `useAppStore`：**纯 UI 设置 store**（主题 / 默认币种 / 备份设置，localStorage 持久化），不暴露任何参考数据接口。

## 其他领域 composable

| composable | 职责 |
|------------|------|
| `useInstrumentSync` | 股票标的全量同步：调用 `api.syncInstruments`（后台线程执行），监听 `sync-instruments:progress` 事件更新进度/结果（视图挂载时注册，切换 tab 不丢事件） |
| `useRealizedPnl` | 已实现盈亏汇总面板：筛选（账户/标的远程搜索，服务端分页）→ `api.realizedPnlSummary` |
| `useBackup` | 备份/恢复：创建 zip 备份、列出受管备份、恢复后 `restart_app` 重启 |
| `useCategoryForm` / `useTransferForm` / `useRefundForm` / `useInvestmentForm` | 见上文交易表单 |
| `useFormShared` | 表单公共派生：账户选项 / 币种选项（包装 `useReferenceStore`） |

## 其他共享模块

- `src/components/transactionColumns.ts` — 交易列表与搜索视图**共用同一列配置**（日期/类型/分类/账户/备注/金额）；备注为唯一弹性列（不设 `width`），固定列宽总和作为 `scroll-x` 横向滚动下限。
- `src/components/categories/` — 分类管理 UI：`CategoryTree.vue` 用 `NTree` + 同级拖拽重排（跨级/跨 kind 拖拽被拦截，层级变更走编辑面板选父分类），`sort_order` 经 `reorder_categories` 落库。
- `src/utils/category-tree.ts` — 分类树纯函数（`rootCategories / categoryChildren / categoryPath / buildCategoryTree`），被 reference store 复用。
- `src/utils/viewState.ts` — 视图状态持久化（当前视图名、侧边栏折叠）跨启动恢复。

## 视图与路由

- hash 路由（`createWebHashHistory`），视图：dashboard / transactions / **search** / accounts / reports / investments / budget / **ai** / settings（共 9 个；search 与 ai 为后加视图）。
- Naive UI 按需 import；主题由 `useAppStore.theme` 决定（`darkTheme` 硬编码于 `App.vue` 的 `NConfigProvider`）。
