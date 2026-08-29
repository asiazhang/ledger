# 领域词汇表：投资域

> Ledger 领域词汇表的投资域分域。全部分域与彼此关系见 `CONTEXT-MAP.md`；决策记录见 `docs/adr/`（ADR-0015 / ADR-0019 / ADR-0020 等）。
> 跨域共享术语（Transaction、Amount Model、Transaction Kind Mapping、DefaultCurrency 等）见核心交易域 `CONTEXT-core.md`，本文不复制定义。
> 若与代码行为冲突，以代码为准并同步修正本文件。

## Instrument（金融工具/标的）

- **定义**：用户可交易的金融产品，例如某只股票、某只基金、某只债券。
- **边界**：
  - 包含五种类型：`stock`（股票）、`fund`（基金）、`bond`（债券）、`etf`（ETF）、`other`（其他）。
  - `symbol` 为显示代码（如 `600519`、`000001`、`00700`），`market` 区分市场（`sh`/`sz`/`hk`；手动创建未指定时为 `unknown`），两者共同标识一个标的。
  - 股票类标的由"标的全量同步"从东方财富 API 批量拉取填充（代码、名称、市场、币种），用户无需手动录入；基金、债券等非股票标的仍可手动创建。
  - `name` 为标的名称（如 "贵州茅台"），同步时自动填充。
  - **投资状态（`invested`）**：列表返回的派生布尔字段，= 是否为持仓标的（见 InvestedInstrument）；标的页据此展示"持仓"标记列，并可用"只看持仓"开关过滤（`only_invested` 过滤参数，issue #102）。
- **别名**：不使用"证券"（偏法律概念）、"产品"（过于泛化）。不单独使用 "market_symbol" 或 "ticker"——统一使用 "symbol"。

## MarketPrice（市场价格）

- **定义**：某个 Instrument **当前**的市场报价——现价缓存，即 PriceHistory 中该标的最新一条的即时映像，供持仓市值与未实现盈亏（Holding）计算消费。
- **边界**：
  - 仅代表"现在"：每次同步覆盖更新本条记录；历史报价序列由 PriceHistory 承载，MarketPrice 不再是价格的唯一载体。
  - `price_cents` 和 `currency_code` 由数据源（东方财富）返回，币种按市场固定：`sh`/`sz` → `CNY`，`hk` → `HKD`。
  - `priced_at` 记录同步时间。
  - `source` 标记数据来源，当前为 `eastmoney`。
- **别名**：不使用"行情"（偏实时交易概念）、"报价"（偏询价场景）、"最新价快照"。

## PriceHistory（价格历史）

- **定义**：某个 Instrument 的市场报价按周粒度累积的历史序列，用于绘制单标的走势与投资资产走势（PortfolioValueTrend），是价格在「现价」之外的第二条承载线。
- **边界**：
  - **仅覆盖股票类持仓标的**（口径同 InvestedInstrument）：基金、债券等无行情来源的标的不参与价格记录与走势图（MVP 决策，非股票仅股票/ETF 走东财日 K 数据源）。
  - **周采样**：每周至多记录一条，取该周最后一个有报价交易日的价格；整周无有效报价（停牌、节假日全周覆盖等）则该周无点，曲线跨越空档连续。一周内重复获取时以新数据**整周覆盖**（幂等），不产生重复。
  - **采集单通道**：经 K 线接口一次性回填近两年，此后随持仓价格增量同步持续累积；日线最后一根即今日最新价，故"回填历史"同时也是"刷现价"，不存在两条刷新通道。
  - 已清仓标的的价格历史保留不删，供回看过去的组合市值；清仓只影响后续不再新增。
  - 港股以 HKD 计价，跨币种汇总走势须经 FxRateHistory 同期折算，不用当前汇率近似历史。
- **别名**：不使用"历史行情"（口语偏全市场）、"K 线"（那是数据源的原始形态，本地不保留日线）、"净值曲线"（基金净值语义不同）。

## FxRateHistory（汇率历史）

- **定义**：货币间兑换比率按周累积的历史序列，与 PriceHistory 同源同时段采集（东方财富汇率日 K），用于把非默认币种的**历史**市值折算到核心交易域 DefaultCurrency。
- **边界**：
  - 与现有 ExchangeRate（当期汇率表）并存分工：当期折算走 ExchangeRate，历史期折算走 FxRateHistory；同期同规则（正反向兜底）。
  - 周采样与整周覆盖规则与 PriceHistory 一致。
  - 只随本域市值折算消费；MVP 阶段核心交易域流水折算（raw/native）仍只用当期汇率，不变。
- **别名**：不使用"历史汇率表"（易与当期表混淆指代）；可简称"汇率历史"。

## PortfolioValueTrend（投资资产走势）

- **定义**：把用户全部（股票类）持仓在各时点的市值汇总成的时间序列视图，回答"我的投资总价值如何变化"；同一功能内也呈现单个标的自身价格序列的单标的走势。
- **边界**：
  - 组合市值 = 各持仓标的「当期持有数量 × 当期价格」之和：数量由核心交易域 buy/sell 交易流水在查询时推算（不物化每日/每周快照），价格取自 PriceHistory。推算**仅认 buy/sell 流水**：dividend/split 目前被写入层显式拒绝故无影响，未来支持拆股等改变持有量的公司行为时，逐期推算必须同步纳入，否则静默失真。
  - 跨币种按 FxRateHistory 折算为核心交易域 DefaultCurrency 后汇总成一条曲线；曲线为周采样点连线。
  - 展示位置 MVP 为投资页内的走势视图（预设区间切换：1 月 / 3 月 / 1 年 / 全部；组合走势与单标的走势同视图切换）；多标的叠加对比属后续迭代。
  - **不含现金账户**：总资产走势（现金余额历史重建 + 投资市值）是明确的后续迭代，不在本概念范围内。
  - 标的在区间起点之前买入无早期价格时，曲线从首个有效采样点开始。
- **别名**：不使用"净值走势"（基金净值语义不同）、"收益曲线"（混入收益率口径）、"总资产走势"（明确含现金账户的后续概念）。

## Holding（持仓）

- **定义**：用户在某个投资账户中持有的某个 Instrument 的数量及成本信息。
- **边界**：
  - `quantity` 为当前持有数量（由各持仓批次 `remaining_quantity` 实时聚合）。
  - `cost_basis_cents` 为持有成本（买入总金额），`cost_currency_code` 为成本币种。
  - `latest_price_cents`、`market_value_cents`、`unrealized_pnl_cents` 由 `v_holdings` 视图实时计算（关联最新 MarketPrice 与汇率折算到账户本位币），不落库存储。
  - 市值 = quantity × latest_price_cents（按汇率折算）；未实现盈亏 = 账户币市值 - 账户币成本。
- **别名**：不使用"仓位"（偏交易术语）、"库存"（偏实物）。

## NetWorth（净资产）

- **定义**：用户全部真实财富在某一时刻的单一合计数字（本位币）：Σ 非投资账户余额 + Σ 持仓市值，均折算为核心交易域 DefaultCurrency 后相加。
- **边界**：
  - 由后端只读聚合命令 `dashboard_overview` 实时计算，不落库存储；账户侧沿用核心交易域 `account_flow` 余额口径（符号归属见核心交易域 Transaction Kind Mapping，与账户列表/余额页一致，排除隐藏/黑洞账户——见 AI 导入域 BlackHoleAccount），投资账户余额不计入——其价值经持仓市值体现，避免同一笔资产重复计算。
  - 持仓市值取 `v_holdings` 的 `market_value_cents`（账户本位币），再折算到 DefaultCurrency；从未录价（或缺折算汇率）的持仓按空值语义跳过，不以零计入。
  - 折算遇缺失汇率让错误上抛（中文错误信息），不静默返回不完整的合计数字。
  - 负债账户按 `account_flow` 现行符号忠实求和，不在净资产层强制取负。
  - 「总资产走势」（净资产的时间序列，现金余额历史重建 + 市值）是明确的后续迭代，不在本概念范围内。
- **别名**：不使用"总资产"（与「总资产走势」既有预留概念区分，避免同一页面两套口径混淆）、"净值"（基金净值语义不同）。

## 时点持仓（AsOfHolding）

- **定义**：某标的（或全组合）在**某交易日**的持有数量，由核心交易域 buy/sell 交易流水在查询时推算。「仅认 buy/sell 流水、sell 取负、按日期前缀求和」三件事单点收敛在投资域的推算模块内（as-of 持仓推算接缝，spec #168）。
- **边界**：
  - **与 Holding 的分工**：Holding 答「现在持有多少」（持仓批次聚合），时点持仓答「某时点持有过多少」（流水前缀推算）；同一批买卖流水下两者在「今天」必须一致（绑定不变式测试钉住）。
  - **时间语义是交易日**：as-of 键为交易日（含当日的前缀求和）；周采样键（week_start）是 PortfolioValueTrend 查询侧的时间语义，不进推算模块——双时间键契约由此显式分界。
  - **推算仅认 buy/sell 流水**（CONTEXT 核心域 Transaction Kind Mapping）：dividend/split 目前被写入层显式拒绝故无影响；未来公司行为落地时只改推算模块内部，走势查询不感知。
  - **消费者**：仅 PortfolioValueTrend 的组合市值（逐价格行取该标的当期数量）；单标的走势是价格直出，不消费本概念。对外 IPC 契约不变。
  - **已知口径边界**：流水口径不排除软删除账户的交易（与 Holding / InvestedInstrument 排除软删账户批次存在既有分叉，行为保持现状，待另 issue 定案）。
- **别名**：不使用「历史持仓」（易读成历史持仓记录）、"position"（交易术语）、与 Holding 混称「持仓数量」（无法区分现值与 as-of）。

## Investment（投资域）

- **定义**：承载核心交易域证券交易（buy/sell）背后持仓批次、卖出匹配与已实现盈亏的概念域，物理实现为 `commands::investment` 模块（lot / 匹配 / pnl 数据逻辑）。
- **边界**：
  - **buy/sell 首先是交易 kind**：一笔 buy/sell 先是一笔核心交易域 `Transaction`（交易行落库经 Writer 接缝），Investment 是它背后的持仓/盈亏载体。
  - 对外出口收窄为 `prepare / apply / revert` 三件套（issue #72）：prepare 校验归一化（不落库）、apply 应用副作用（buy 建仓 / sell 卖出匹配）、revert 回退副作用（buy 守卫+清理 / sell 回补）；交易行写入由核心交易域行为层编排（经 `transaction::writer`），Investment 不再反向依赖 transactions 的行更新函数（双向依赖已斩断，issue #70）。
  - 分派用薄而穷尽的 `match`，不引入 trait 注册表（避免过度设计）。
- **别名**：不使用“投资账户”（那是 `AccountType::Investment` 账户）、“证券模块”（偏数据层）。

## TransactionTrade（交易买卖明细）

- **定义**：一笔 buy/sell 核心交易域 Transaction 在投资域扩展表中的投影——标的、数量、单价、手续费，物理实现在 `security_transactions`（按 `transaction_id` 关联交易行）。
- **边界**：
  - **核心交易行不含投资字段**（ADR-0003 核心表 + 扩展表）：买入/卖出编辑（issue #180）回填标的/数量/价格/费用须经只读命令 `get_transaction_trade`（IPC 域命令）读取本投影，不把投资字段塞进核心 `Transaction` 模型。
  - 投影随读带出 `symbol`/`instrument_name`（JOIN `instruments`），供回填后标的选择框直接显示标的而非裸 id。
  - 无明细（交易不存在/非 buy/sell）返回 `NotFound`「无买卖明细」。
  - **回填 ≠ 可改金额**：编辑提交走行为层全字段替换权威（`update_transaction`，与 HTTP PUT 同源），金额由 `prepare` 按数量×单价±手续费重算，明细投影只读。
- **别名**：不使用“成交记录”（易与交易所对账单混淆）、“投资交易详情”。

## InvestedInstrument（持仓标的）

- **定义**：有**当前持仓**（`security_lots.remaining_quantity > 0`，即 `v_holdings` 视图有行、且排除软删除账户的批次）的 Instrument，即“已投资”标的（ADR-0015）。
- **边界**：
  - 判定谓词单点定义（`commands::investment::crud` 的 `INVESTED_EXISTS`），同一口径驱动四处：`list_instruments` 的 `invested` 派生列、标的页"只看持仓"过滤（`only_invested`）、增量同步（`sync_holding_prices`）的标的集合、盈亏页持仓概览（issue #102/#103/#110）。
  - **不含已清仓标的**：已清仓的得失由已实现盈亏（RealizedPnl）承载，不混入“已投资”。
- **别名**：不使用"已投过的标的"（含已清仓）、"自选/关注标的"（那是用户主观收藏，与持仓无关）。

## InstrumentSync（标的全量同步）

- **定义**：用户手动触发、系统从东方财富 API 一次性全量拉取沪市/深市/港股股票标的信息（代码、名称、最新价），upsert 到本地 `instruments` 与 `market_prices` 的过程。**职责 = 修标的字典**（ADR-0015 职责切分：全量修字典 / 增量刷价格 + 沉淀历史；ADR-0019 价格历史化）。
- **边界**：
  - 触发方式：手动触发（标的页"全量同步"入口，issue #101 迁移后设置页不再提供同步入口），不做自动定时刷新。
  - 同步范围：沪市（`sh`）、深市（`sz`）、港股（`hk`），币种分别固定为 `CNY`/`CNY`/`HKD`。
  - 执行方式：一次性全量同步、分页拉取，无并发限制、无失败重试。
  - 去重：按 `symbol` 匹配已有标的，名称或市场变更则更新，不存在则插入，并同步 upsert 该标的最新价到 `market_prices`。
  - 进度反馈：通过 `sync-instruments:progress` 事件向前端汇报当前页数/总数与累计新增、更新数量。
  - **中断机制（issue #104）**：分页循环每页检查共享取消标志（`AtomicBool`），经 `cancel_sync_instruments` 命令置位后即提前返回并推送中断态事件；已落库数据保留（upsert 幂等）、下次重跑自动续上。进度事件终态以 `cancelled` 字段区分完成（`done=true, cancelled=false`）与中断（`done=true, cancelled=true`）。
  - 无同步进行时调用 `cancel_sync_instruments` 无副作用，返回明确提示。
  - **结束即发价格失效信号（ADR-0031）**：结束时只要本次运行有落库（含用户中断——中断保留已落库价格，不发信号即失真）即 emit `ledger:prices-changed`，与增量同步共用同一信号。
- **与增量同步的边界**：全量修字典（增删改 `instruments` + 刷价），增量刷价格并沉淀历史（见 HoldingPriceSync、ADR-0019）；日常刷新现价与走势走增量同步，全量仅在标的字典需要修补时触发（二次确认说明范围与数百次 API 请求代价）。
- **别名**：不使用"同步价格"（偏持续同步）、"更新行情"（偏实时）。

## HoldingPriceSync（持仓价格增量同步）

- **定义**：用户手动触发、从当前持仓（口径同 InvestedInstrument）收集股票类标的，按 `(market, symbol)` 构造东财 secid（`1.600519` / `0.000001` / `116.00700`）批量查询最新价（`ulist.np/get`，约 50 只/请求），并按日 K 接口回填近两年周线历史：upsert `market_prices`（现价）+ 把日线降采样落 PriceHistory 与 FxRateHistory（ADR-0019）——不增删、不改 `instruments` 的名称与市场。UI 文案为"同步持仓价格"，与"全量同步"区分（ADR-0015 职责切分：全量修字典 / 增量刷价格 + 沉淀历史）。
- **边界**：
  - 触发方式：手动触发（标的页"同步持仓价格"按钮与盈亏页"当前持仓"概览卡右上角按钮，两处共用同一 `useHoldingPriceSync` 接缝、行为一致），不做自动定时刷新。
  - 只覆盖股票类持仓：非股票持仓（基金/债券等，数据源不含行情）、市场未知（无法构造 secid）的标的计入"跳过 M 只"统计提示，不报错。
  - 停牌/无效价（f2≤0）跳过、保留旧价，不中断同步；响应 data:null（全部代码无效）优雅降级为空。
  - 重复触发幂等：每标的一条 `market_prices` 覆盖更新（`source` 沿用 `eastmoney`）；PriceHistory 同周整周覆盖，均不产生重复数据。
  - 完全无持仓时返回明确提示（"无持仓标的可同步"）而非报错。
  - **成功即发价格失效信号（ADR-0031）**：实际写入价格（`synced > 0`）时 emit `ledger:prices-changed`，各价格消费方由信号驱动重拉；接缝 `useHoldingPriceSync` 的承诺是「同步完成即通知失效」，调用方无需自行重拉，零变化（无持仓/全部跳过）不广播。
  - 复用全量同步的 HTTP 层（主机池、重试、限流 pacer）与价格换算（A 股 f2 直接得分、港股 ÷10），增量同步 API 访问量从全量数百次降到个位数（issue #103）。
- **别名**：不使用"增量行情"（口语），正式术语为"持仓价格增量同步"；UI 文案固定为"同步持仓价格"。

## 价格失效信号（PriceChangeSignal）

- **定义**：价格数据（MarketPrice / PriceHistory / FxRateHistory）发生写入后，后端发出的无 payload、粗粒度的 `ledger:prices-changed` 事件信号（ADR-0031）；前端价格消费方各自订阅并重拉自身数据，替代「同步后记得手动刷新」的调用方自觉。
- **边界**：
  - **生产者仅两处**：HoldingPriceSync 成功且实际写入价格（`synced > 0`）时；InstrumentSync 结束且本次运行有落库时（含用户中断）。零变化（无持仓标的、全部跳过）不广播——失效信号的本义是「数据变了」。
  - **与平行信号的关系**：与参考失效信号 `ledger:changed`（见参考数据与设置域 Reference Data）、备份信号 `ledger:backups-changed`（见备份与数据文件域 Backup）同一 `ledger:*` 命名空间、同一形状（无 payload、粗粒度），各域语义各自锚定；不复用 `ledger:changed`——其语义锚定参考写入（ADR-0012），价格同步误发会触发参考表无谓重拉而价格消费方无人响应。
  - **消费方自选订阅**：信号可用性全局，订阅与否由消费方决定（本期为持仓概览与标的页标的列表；走势、仪表盘等留后续）；未订阅者行为不变。
- **别名**：不使用"行情刷新事件"、"价格推送"（推送暗示带 payload 的实时行情流，本信号只是失效通知）、与 `ledger:changed` 混称"失效信号"（不区分域）。
