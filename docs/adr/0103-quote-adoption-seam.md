# ADR 0103: 行情接入接缝——QuoteAdoption 统一行情查询与建档落价，价格写入单点改命名参数入口

- 状态：已接受（决策入档；实现随 spec #1010 落地）
- 日期：2026-09-11
- 作者：Ledger 项目
- 关联：ADR-0081（本接缝是「按代码查询主干道」在新增标的路径的接缝化命名）；ADR-0038 / ADR-0039（场外基金按代码即拉与创建增强先例，股票侧镜像同构）；ADR-0036（价格来源词表与写入单点）；ADR-0095（标的信息同步——留接缝外的批量通道）；词汇表：投资域「行情接入（QuoteAdoption）」；来源：架构走查（2026-09-11）候选 8 · Worth exploring，spec #1010 grilling 定稿

## 背景

基金与股票的新增标的路径各走三段同形阶梯——取行情（sync 域网络层 `fetch_fund_detail_production`（`src-tauri/src/sync/fund.rs:136`）/ `fetch_stock_quote_production`（`src-tauri/src/sync/stock.rs:169`））→ 按代码解析/建档（investment 域 `add_fund_by_code_with`（`src-tauri/src/investment/fund.rs:139`）/ `fetch_stock_quote_for_add`（`src-tauri/src/investment/stock.rs:222`）+ `persist_fund_detail`（`src-tauri/src/investment/fund.rs:93`）/ `persist_stock_quote`（`src-tauri/src/investment/stock.rs:362`））→ 落价格（`upsert_market_price`，`src-tauri/src/investment/prices.rs:65`）。形状经 ADR-0038/0039/0081 演进已经对齐：注入拉取闭包同构（慢闭包在连接锁外）、persist 均为「建档 + 落价」一体。但接缝没有名字：payload 各写一份（`FundDetail` / `StockQuote` 同构不同形）、拉取闭包签名各异（`FnMut(&str)` vs `FnMut(&str, &str)`）、`upsert_market_price` 以 conn + 6 个业务位置参数暴露，7 个生产调用点各自重新记忆 `nav_date` / `source` 的通道语义。壳层编排节奏也不相同：基金 IPC 一枪式（查询即落库）、股票对话框两段式（查询回显、确认后落库）、AI 创建端点直调 persist。

## 决策

1. **命名行情接入接缝（QuoteAdoption）**：两成员——查询半边（按代码取行情，纯取数不落库）与落库半边（**建档 + 落现价一体**）。persist 含建档是裁决而非巧合：三个消费壳都消费一体形态，拆开会把编排重新暴露给壳层；两成员可分别调用，恰好承载「查询回显 → 确认落库」的两段式与纯查询消费（标的查询端点）。单方法带 `mode` 不成立——查询与落库不是同一动作的两种模式，消费者要么只要查询、要么要全程，没有第三种编排。
2. **不引入 Rust trait**：接缝由统一形状与命名承载——统一报价载荷 `Quote`（`FundDetail` / `StockQuote` 退役：代码、权威名称、价格、价格日期为公共成员；市场、类型提示、基金分类、净值日期等通道差异以可缺省成员承载）与统一的注入拉取签名。依赖方向是 sync → investment（查询半边实现在 sync、落库半边在 investment），单一 trait impl 表达不了跨域两半；且全库没有「遍历一组 adapter 统一处理」的多态消费点，trait 是纯机制税——与 #1005「不引入注册表机制」同一口味。接缝落点为投资域 `quote` 模块。
3. **边界：手动报价与标的信息同步留接缝外**。手动报价无查询步骤（用户直给日期 + 价）且落库形状不同（两落点 + 最新点映像规则 + op 产出）；标的信息同步是批量编排（分批报价 + 日 K 回填 + 确定进度 + 净值水位增量）。接缝语义收窄为「按代码从数据源取行情 → 建档 → 落价」的**新增标的路径**；三条价格写入路径（手动 / 同步 / 新增标的）共用价格写入单点——共用发生在写入单点层，不在接缝层。
4. **价格写入单点改命名参数入口**：conn + 6 业务位置参数收敛为单一输入结构（含净值日期成员），不搞 builder。`source` 维持字符串词表**不 enum 化**——重放与既有写价命令两处透传 `Option<&str>` 是已发布行为，enum 化引入「未知值拒绝」的行为变化，违反纯重构纪律，类型化另立议题。通道语义留在各自通道、不收进写入单点：股票现价 `priced_at` = 写入时刻、基金 = 净值日期、净值水位比较归净值通道、最新点映像归手动报价。
5. **契约零变化**：IPC / HTTP / AI 契约、错误码、schema 零变化；`AddFundResult` / `AddStockInstrumentResult` 回显投影保持各自形状。纯重构。
6. **定序：与 #1005（lots / unwind 拆模块）无阻塞**。文件面不重叠（#1005 动 trade / behavior 与新增 lots / unwind；本票动 fund / stock / prices 与 sync 侧），可并行；共同触碰点仅 `investment/mod.rs` 模块声明，后者合入时一次 rebase。若实际串行执行，本票先行（体量小、不碰 trade）。

## 理由

- 形状早已存在，缺的只是名字与载荷统一：「两个 adapter ⇒ 真接缝」的判据在基金/股票通道已满足；不收口则第三种行情来源接入时重推 payload 形状，且 `nav_date` / `source` 的通道语义继续由每个调用点重新记忆。
- 两方法是形状的下界：查询与落库的消费者集合不同（纯查询端点只要查询），合并即引入 mode；三阶段拆分的唯一增量受益者是降级建档路径——那是镜像接缝的异常分支，不值得为它拆开一体形态。
- 名字覆盖两个成员（行情接入）而非单边（QuoteLookup）：persist 是有状态的半边，「Lookup 名下写库」是未来读者必问的惊讶；Rust 侧标识符落在模块名与函数族上，改名零代码成本。

## 代价

- `FundDetail` / `StockQuote` 退役波及 wire 解析投影、api_server 注入类型与测试桩——全部为机械适配，契约投影不动。
- 统一载荷以可缺省成员承载通道差异，基金/股票各自的强约束（基金类型恒 fund、市场恒 unknown 等）从类型形状退到通道内判定。

## 替代方案

- **Rust trait `QuoteLookup` + 两 impl**：跨域两半无法落进单一 impl（依赖方向 sync → investment），且无多态消费者，否决。
- **三阶段拆分（查询 / 建档 / 落价）**：为降级建档路径服务的过度拆分，否决。
- **单方法带 mode**：见理由，否决。
- **builder 或位置参数不动**：7 字段场景 builder 是机制税；位置参数即本 issue 病灶，均否决。
- **source enum 化**：给透传通道引入行为变化，另立议题。
- **与 #1005 原生阻塞或同批**：无必要，见决策 6。

## 影响

- 词汇表：投资域新增「行情接入（QuoteAdoption）」词条（随本 ADR 落盘）。
- 实现：spec #1010 单票交付（worktree + PR），验收判据见 issue 正文——既有测试全绿、`rg` 枚举价格写入单点全部调用点逐一核对「全部经单一输入结构」、`FundDetail` / `StockQuote` 无残留、契约零变化、删除落库半边内落价调用则添加基金落现价与价格失效信号测试变红、`./scripts/check.sh` 全绿。
- Schema / 契约：零变化，无迁移。
