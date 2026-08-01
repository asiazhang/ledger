# 投资价格刷新与股票代码库 Spec

> **已归档（2026-08-01）**：本方案被 `specs/stock-instruments-sync.md`（东方财富 API 一次性全量同步、直接写入 `instruments` 表、无并发限制/重试）取代，不再按此实现。

## Problem Statement

用户在 Ledger 中记录投资持仓（`Holding`）和标的（`Instrument`）时，无法自动拉取最新的公开市场价格（Market Price），导致持仓的最新市值（`market_value_cents`）与未实现盈亏（`unrealized_pnl_cents`）无法更新。此外，用户在建立持仓选择股票标的时，缺乏本地只读的股票代码参考库（目前优先涵盖港股），需要手动逐个录入代码与名称。

## Solution

1. **全量股票代码库（`securities`）同步**：增加系统只读表 `securities`，通过 API 全量拉取港股股票数据存入本地 SQLite。用户新增标的或记账选择股票时，完全基于本地 `securities` 数据进行搜索，无需在选择时触发实时网络请求。
2. **在线刷新价格功能（`PriceRefresh`）**：在持仓/投资页面提供全局“刷新价格”按钮。点击后由 Rust 后端向 Yahoo Finance API 批量查询所有持仓标的的最新价格。
3. **覆盖式更新与持仓重新计算**：查询到的价格按 `instrument_id` 覆盖更新写回 `market_prices` 表（不保留历史低效数据），并同步重新计算并更新 `v_holdings` 中的最新单价、最新市值与未实现盈亏。
4. **容错与结果反馈**：采用并发控制（最多 3 个并发请求）与失败重试机制（自动重试 1 次，间隔 1 秒）。刷新完成后在前端展示清晰的成功/失败汇总提示（如“已更新 5/6，1 个失败”），并提示币种不一致等情况。

## User Stories

1. 作为投资用户，我希望系统提供通过 API 全量同步港股股票代码库的功能，以便本地拥有完整的港股标的数据库。
2. 作为投资用户，我在创建标的或记账选择股票时，希望能够在本地 `securities` 库中快速检索，不需要每次选股都等待网络 API 响应。
3. 作为投资用户，我希望标的数据由系统从 API 获取并只读展现，防止我自己手动填错股票代码或名称。
4. 作为投资用户，我希望在持仓页面点击一个“刷新价格”按钮，就能一次性更新我持有的所有股票的最新市场价格。
5. 作为投资用户，我希望点击“刷新价格”后按钮展示加载状态（Loading），防止我不小心连续多次重复点击。
6. 作为投资用户，我希望价格刷新过程快速且稳定，后端限制并发请求数（如最多 3 个），避免触发第三方接口限流。
7. 作为投资用户，当某只股票因网络抖动刷新失败时，我希望系统能自动尝试重试 1 次，提高刷新的成功率。
8. 作为投资用户，当刷新完成后，我希望看到明确的弹窗/Toast 提示告诉我刷新了多少只股票、失败了多少只，让我对更新状态心里有数。
9. 作为投资用户，当数据源返回的价格币种与我设置的标的币种不一致时（如 Yahoo 返回 HKD 但标的配置为 CNY），我希望收到友情提示，但价格依然正常更新。
10. 作为投资用户，价格刷新成功后，我希望能立即在持仓列表中看到最新的单价、最新的持仓市值以及最新的未实现盈亏。
11. 作为投资用户，我不希望每次价格刷新都写入海量的历史价格记录膨胀数据库，系统只需覆盖保存每个标的的最新一条价格报价。

## Implementation Decisions

### 1. 数据库 Schema 调整

- **新增 `securities` 表（股票代码参考表）**：
  - 字段：`id` (TEXT PK), `symbol` (TEXT UNIQUE, 如 `0700.HK`), `name` (TEXT), `type` (TEXT, 固定 `stock`), `currency_code` (TEXT, 如 `HKD`), `source` (TEXT, 如 `yahoo_finance`), `created_at` (TEXT), `updated_at` (TEXT)。
  - 对用户只读，通过后端 API 全量同步接口填充与更新。
- **`instruments` 表关联**：
  - 用户创建标的时，关联选择 `securities.symbol`。
- **`market_prices` 表覆盖写入**：
  - 使用 `ON CONFLICT(instrument_id) DO UPDATE SET price_cents=excluded.price_cents, currency_code=excluded.currency_code, priced_at=excluded.priced_at, updated_at=excluded.updated_at` 保证每个标的仅保留最新一条报价数据。

### 2. 后端模块设计 (Rust / Tauri Command)

- **证券库同步模块**：
  - `sync_securities` Tauri Command：通过 HTTP API 获取港股股票全量列表，事务批量 upsert 入 `securities` 表，返回新增/更新的数量。
- **价格刷新模块 (`price_refresh`)**：
  - 接口抽象：定义 `PriceFetcher` trait，方便单元测试进行 Mock。
  - 请求实现：使用 `reqwest` 发送 HTTP 请求调用 Yahoo Finance Chart API (`query1.finance.yahoo.com/v8/finance/chart/{symbol}`)。
  - 并发控制：使用 `tokio::sync::Semaphore` 或 `futures::stream` 限制最大并发数为 3。
  - 重试策略：遇到网络错误或 HTTP 非 200 时，`tokio::time::sleep` 延迟 1 秒后重试 1 次。
  - 结果结构体 `RefreshResult`：
    - `instrument_id`: String
    - `symbol`: String
    - `success`: bool
    - `price_cents`: Option<i64>
    - `currency_code`: Option<String>
    - `currency_mismatch_warning`: Option<String>
    - `error`: Option<String>
  - `refresh_prices` Tauri Command：查询数据库中所有已有持仓/配置的标的，调用刷新逻辑，更新 `market_prices` 并在同一事务或批处理中更新 `v_holdings` 相关字段，返回 `Vec<RefreshResult>`。

### 3. 前端交互设计 (Vue 3 / Naive UI)

- **持仓页面新增“刷新价格”按钮**：
  - 点击绑定调用 `api.refreshPrices()` 命令。
  - 调用期间按钮处于 `loading` 状态，自然防抖。
- **结果汇总反馈**：
  - 根据返回的 `Vec<RefreshResult>` 计算成功数 $S$ 与失败数 $F$。
  - 若 $F > 0$，展示 warning/info 级 Message："已更新 $S$/$N$ 个标的，$F$ 个失败"。
  - 若包含币种不匹配预警（`currency_mismatch_warning`），提示具体标的与返回币种。
- **持仓列表实时刷新**：
  - 刷新完成后触发 Pinia store / Holdings view 重新加载持仓数据，反映最新市值与未实现盈亏。

## Testing Decisions

### 1. 自动化测试原则

- 仅测试模块的外部行为（API 返回值、数据库持久化状态、重试机制），不依赖 Yahoo Finance 真实网络调用。

### 2. 测试接缝与层级

1. **`securities` 数据库批量同步测试**：
   - 测试点：给定 API 响应数据，批量写入 `securities` 表，重复同步时支持 `ON CONFLICT` 更新不重复插入。
   - 优先参考：现有 `src-tauri/src/commands/investment/tests.rs` 中的内存数据库测试模式。
2. **`refresh_prices` 编排逻辑与重试测试 (Rust Mock Testing)**：
   - 测试点：通过 实现 Mock `PriceFetcher`：
     - 测试全部成功场景：`market_prices` 正确更新，`v_holdings` 最新市值与未实现盈亏正确重新计算。
     - 测试第一次失败、重试第二次成功场景：确认最终记录为成功。
     - 测试连续二次失败场景：确认不崩溃，结果结构体中对应标的标记为 `success: false` 并附带 error。
     - 测试币种不一致场景：确认正常写入价格，同时 `currency_mismatch_warning` 被正确标记。
3. **前端组件测试**：
   - 测试点：刷新按钮状态切换、网络请求完成后 toast 消息展示逻辑与持仓列表刷新触发。

## Out of Scope

- 历史价格走势图与 K 线图（MVP 阶段只保留最新价格，不记录历史价格流水）。
- 自动后台定时刷新或定时任务调度（仅支持用户手动点击触发）。
- 实时 WebSocket 行情推流。
- 港股以外其他市场（如美股、A股）的全量 API 列表同步（MVP 仅涵盖港股股票 API 全量同步）。
- 用户手动编辑 `securities` 股票参考表数据。

## Further Notes

- 严格遵循项目的 `CONTEXT.md` 领域术语表（如使用 `Instrument`、`MarketPrice`、`Holding`、`PriceRefresh`）。
- API 响应与 Tauri Command 错误处理均复用项目统一的 `{kind, message}` 格式。
