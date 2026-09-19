# 行情/报价数据源调研：219 标的、个人自用、Tauri + Rust 桌面直连

- 调研日期：2026-09-15（Asia/Shanghai）；**2026-09-19 复核见第 13 节**（替代源实测与 ADR-0130 决策依据）
- 场景：中国个人记账/投资桌面应用（Tauri + Rust 后端），现用东方财富免费接口，库内约 219 个投资标的（A 股、场内 ETF/LOF、场外基金、可能含港股/美股），全量同步偏慢
- 目标：给出可替换/可补充的行情与净值数据源，并评估批量能力、实时性、成本、合规与「能否落进现有 provider 抽象」
- 证据标注：
  - **【实测】** 本次调研直接 `curl`/脚本调用观测到的行为（含返回结构、数量边界、耗时）
  - **【一手】** 官方文档/官网/官方源码/官方条款/官方 SDK
  - **【社区】** 社区经验或二次转述，未获一手证据
- 重要前提：实测来自本机网络出口（非中国大陆家宽），且测试时点为**A 股当日收盘后**。盘中估值类字段（`GSZ`/`gsz`）、限流阈值、可达性都可能因出口 IP、时段与站点风控而变化；下文对不确定处均显式标注。

---

## 0. 结论摘要

1. **慢的主因不是「报价」而是「历史 K 线 + 基金净值按标的逐个请求」**。现有实现的批量报价每 50 个 secid 一次请求，已经没有大问题；真正随标的数线性增长的是「每只标的单独发一次日 K 请求」和「每只基金单独查一次净值（+ 一次名称）」，且所有请求共享一个 2 秒间隔的串行节流器。详见第 1 节。
2. **东方财富自身就有可用的批量接口，且批量上限远高于当前用的 50**：`ulist.np/get` 实测一次 500–800 个 secid 可用；全市场列表 `clist` 被服务端硬限制为每页 100 条。历史 K 线（`push2his` 的 `stock/kline/get`）**只能单个 secid**，没有官方批量形态。
3. **场外基金净值有真正的批量路径，且不止一条**：
   - 东财「基金排行」接口 `fund.eastmoney.com/data/rankhandler.aspx` **一次请求（`pn=30000`）返回全市场 20,360 只场外基金的最新单位净值 + 净值日期**（3.5 MB），219 只的分页成本从 219 次降到 1 次。
   - 新浪 `hq.sinajs.cn/list=f_000001,f_...` **一次请求返回多只基金净值**，实测 219 只在 1 次请求 / 0.20 秒内全部返回。
   - 东财 App 端 `fundmobapi` 的 `FundMNFInfo?Fcodes=...` 支持多代码，但**单请求上限实测为 30 个**（219 只需 8 次）。
   - **天天基金 `pingzhongdata/<code>.js` 与 `f10/lsjz` 都不支持一次多只**（前者一次给单只全历史，后者每页硬上限 20 条）。
4. **`fundgz` 估值接口已经不能依赖**（本次实测 + 第三方交叉验证）：请求返回 HTTP 200，但正文是东财「页面未找到」HTML。东财现役的估值入口是 `FundGuZhi/GetFundGZList`（akshare 在用）、静态页 `lof_fundguzhi{N}.html`，以及 `FundMNFInfo` 返回里的 `GSZ/GSZZL/GZTIME` 字段。
5. **免费第三方（新浪/腾讯）的「批量上限」本质是 HTTP 请求 URI 约 8 KB 的限制**，不是「多少只」：新浪约 850 只（超出返回 `431`），腾讯约 900 只（超出返回 `414`）。社区常引用的「新浪 800 / 腾讯 50–60」里，新浪偏保守、腾讯明显过时。
6. **官方/权威日频源存在但无法覆盖全部品种**：上交所/深交所官网有可一次拉全市场的日频快照/文件；**中国证监会基金电子披露网站**是场外基金净值的官方逐只披露源（但仍是逐只请求，全市场一次要 54.8 MB）。港股/美股的免费官方实时行情不存在，实时数据受交易所许可约束。
7. **正式商用源对本场景的价值有限**：Tushare Pro 适合当「日频主力 + 场外基金净值」（个人档约 200 元/年门槛，纯 HTTP 可直连 Rust）；AllTick 适合「实时补充」（支持支付宝、REST + WebSocket）；Alpha Vantage/Finnhub/Polygon/Tiingo 对 A 股/港股基本无解；Wind/Choice/iFinD 属机构商务路径。
8. **最划算的改造方向不是换数据源，而是**：(a) 把基金最新净值从 219 次请求压成 1 次批量请求；(b) 用受控并发替代全局 2 秒串行节流；(c) 历史 K 线要么继续用东财并按受控并发拉，要么考虑一次拿全市场日频的官方文件。这三件事都不需要放弃现有 provider 抽象。

---

## 1. 先看清现状：现在的同步为什么慢

来自本仓库 `ledger-market-sync`（行情同步域，`src-tauri/crates/market-sync`）的**只读**核对：

**已有的批量报价**：`ulist.np/get`，批大小常量 50（`ULIST_BATCH_SIZE`），一次把多个 secid 拼成逗号串。这部分已经是批量的。

**六个抓取通道**（现有 provider 抽象的实际形状，用于后文「能否替换」评估）：

| 通道 | 签名（语义） | 现有实现 |
|---|---|---|
| `fetch_ulist` | secid 逗号串 → 报价条目 | 东财批量报价，50/批 |
| `fetch_kline` | 单个 secid → 日线序列 | 东财 `push2his` 日 K，**每只标的 1 次请求** |
| `fetch_fx` | 币种对 → 日线序列 | 东财日 K |
| `fetch_nav` | 净值查询 → 单页净值 | 东财 `f10/lsjz`，每页 20 条 |
| `fetch_nav_full` | 基金代码 → 整只全历史净值 | 东财 `pingzhongdata/<code>.js` |
| `fetch_fund_name` | 基金代码 → 权威名称 | 东财基金搜索，**每只基金 1 次请求** |

**编排里每类标的的实际请求次数**（据编排代码核对）：

- 行情通道（股票/场内 ETF）每只：`ceil(Q/50)` 次批量报价 + **每只 1 次日 K**；
- 净值通道（场外基金）每只：**1 次净值同步**（首刷 = 1 次 `pingzhongdata`；增量 = 1 次起 `lsjz` 分页）+ **1 次名称查询**；
- 全部请求共享同一个限流器，常量间隔 **2000 ms**，串行。

于是同步耗时（下界估计，不含网络往返与重试）≈ `(ceil(Q/50) + Q + 2F) × 2s`：

| 标的构成（Q = 股票/ETF，F = 场外基金） | 请求数 | 理论下界耗时 |
|---|---:|---:|
| 219 全部为股票/ETF | 5 + 219 ≈ 224 | ≈ 7.5 分钟 |
| 100 股票/ETF + 119 场外基金 | 2 + 100 + 238 ≈ 340 | ≈ 11.3 分钟 |
| 219 全部为场外基金 | 438 | ≈ 14.6 分钟 |

结论很明确：**批量报价不是瓶颈，日 K 按标的串行 + 基金净值/名称按标的串行 + 2 秒全局节流才是**。任何「换数据源」的方案，只有在能把这几个按标的展开的通道也变成批量或受控并发时，才会真正提速。

---

## 2. 评价维度说明

每条途径按以下维度给出：覆盖品种（A 股 / ETF / 场外基金净值 / 港股 / 美股 / 加密）、批量能力（一次请求多少标的）、实时性（实时/延迟/日频）、延迟与吞吐量级、是否需注册或付费、是否官方 API 与稳定性/封禁风险、授权与合规（个人自用 vs 分发）、能否被替换进上述六通道抽象。

---

## 3. (a) 东方财富自身的批量接口

### 3.1 批量报价 `ulist.np/get`【实测】

```text
https://push2.eastmoney.com/api/qt/ulist.np/get
  ?fltt=2
  &secids=1.600519,0.000001,1.510300,0.159915,116.00700,105.AAPL
  &fields=f1,f2,f3,f12,f14
  &ut=fa5fd1943c7b386f172d6893dbfba10b
```

- **一次多少标的【实测】**：6 / 50 / 100 / 200 / 300 / 500 / 800 个 secid 均成功返回；补齐到 **1000 个时返回 HTTP 503**，而同刻同主机的 6 个 secid 小请求正常 200 → 该 503 与**请求体量**相关，不是出口 IP 被整体封禁。可保守认定**单请求 500 个稳妥、800 个可用、1000 个不稳**。
- **返回**：`{"data":{"total":N,"diff":[{"f1":精度,"f2":价格,"f12":代码,"f14":名称},...]}}`；`diff` 可能是数组也可能是按序号 key 的对象；全部代码无效时 `rc=102` 且 `data:null`。
- **覆盖品种**：A 股、场内 ETF/LOF、指数、可转债、港股、美股，靠 secid 前缀区分。实测前缀表：`1.` 沪市、`0.` 深市、`116.` 港股、`105./106./107.` 美股（NASDAQ/NYSE/AMEX）。**场外基金不在其中**（OTC 基金无 secid）。
- **实时性**：`push2` 为实时/准实时；`push2delay` 为延迟主机（本仓库注释记录 `push2` 曾被出口 IP 风控、故优先用 delay 主机）。**「delay = 15 分钟」这一说法本次未获一手证据，标【社区】**。
- **吞吐量级【实测】**：500 个 secid ≈ 4.6 KB URL，单次往返亚秒级。
- **注册/付费**：免注册免费。
- **稳定性/风险【实测 + 本仓库经验】**：`onegate` WAF 会间歇返回 429 或「200 但非 JSON」的拦截页，限流窗口约 2–4 分钟恢复（本仓库注释记录）。本仓库默认 2 秒/请求即为此。
- **合规**：非官方公开接口，无 API 契约；个人自用风险低，**随产品分发有再分发风险**。
- **可替换性**：直接对应 `fetch_ulist`；把批大小从 50 提到 200–500 即可，几乎零改造。

### 3.2 全市场列表 `clist/get`【实测】

```text
https://push2.eastmoney.com/api/qt/clist/get?pn=1&pz=100&po=1&np=1&fltt=2&invt=2
  &fid=f3&fs=<市场过滤>&fields=f12,f13,f14&ut=fa5fd1943c7b386f172d6893dbfba10b
```

- **单页上限【实测】**：`pz` 传 100 / 500 / 1000 / 5000 / 10000 **都只返回 100 条**——服务端硬限制每页 100。流传较广的「`pz=5000` 一次拉全市场」**已不成立【实测】**。
- 用途：全市场标的字典同步（本仓库的 clist 全量同步已按 ADR-0081 退役）。对「219 只按代码查询」场景用处不大。

### 3.3 历史 K 线 `push2his` `stock/kline/get`【实测】

```text
https://push2his.eastmoney.com/api/qt/stock/kline/get
  ?secid=1.600519
  &fields1=f1,f2,f3,f4,f5,f6&fields2=f51,f53
  &klt=101&fqt=0&beg=20240101&end=20500101
  &ut=fa5fd1943c7b386f172d6893dbfba10b
```

- **只能单个 secid【实测】**：把 `secid=1.600519,0.000001` 拼两个 → 返回 `rc=100, data:null`。**东财没有公开的批量历史 K 线接口**。
- **性能【实测】**：单只近两年日线（656 根）约 0.22 秒。
- **含义**：Q 只行情标的 = Q 次 K 线请求，这是当前同步的主要成本之一。`fqt=0` 不复权（与本仓库语义一致），`klt=101` 日线。

### 3.4 东财 App 端基金批量接口 `fundmobapi`【实测】

```text
https://fundmobapi.eastmoney.com/FundMNewApi/FundMNFInfo
  ?plat=Android&appType=ttjj&product=EFund&Version=1&deviceid=1
  &Fcodes=000001,110022,161725
```

- **单请求上限【实测】30 只**：传 30 返回 30；传 31/35/40/60 **一律只返回前 30 条**（`TotalCount` 仍报请求数）；传 ~300 只（URL ≈ 2.2 KB）直接返回 HTTP 404。
- **返回字段**：每只含 `FCODE / SHORTNAME / PDATE（净值日期）/ NAV（单位净值）/ ACCNAV（累计净值）/ NAVCHGRT（日增长率）`，以及**估值字段 `GSZ（估算净值）/ GSZZL（估算涨跌）/ GZTIME`**。本次收盘后实测 `GSZ` 等为 `null`（盘中才会填充）。
- **含义**：219 只场外基金 → 8 次请求即可拿到「官方净值 + 盘中估值」。这是目前已知**唯一能一次同时拿官方净值和盘中估值**的多代码接口。
- **注意**：`pageIndex/pageSize` 无 `Fcodes` 时返回 0 条——它不是「全市场分页」接口。

### 3.5 东财估值接口 `FundGuZhi/GetFundGZList` 与静态估值页

- `fundgz.1234567.com.cn/js/<code>.js`（老牌实时估值接口）**已失效**【实测】：多个正常代码都返回 HTTP 200 但正文是东财「页面未找到」HTML，`Content-Type: text/html`，与第三方交叉验证一致。**不要接入**。
- 现役候选 1（JSON）：`https://api.fund.eastmoney.com/FundGuZhi/GetFundGZList?type=1&sort=3&orderType=desc&canbuy=0&pageIndex=1&pageSize=20000&_=<ts>`，`type` 1–9 对应全部/股票/混合/债券/指数/QDII/ETF 联接/LOF/场内交易基金。【一手】akshare 源码正在用它；**【实测】本次收盘后各 `type` 均返回 `ErrCode=-1 暂无数据`**，未能在盘中复核——标记为「有源码支持、本次未取到数据」。
- 现役候选 2（静态页）：`https://fund.eastmoney.com/lof_fundguzhi{1..7}.html`，每页约 201 行，带 `data-gz` 属性；实测第 1–7 页有效、第 8 页起 301。**只覆盖指数型**，不足以覆盖 219 只混合/主动基金。

### 3.6 基金净值相关的其余东财接口

| 接口 | 形态 | 单请求上限【实测】 | 说明 |
|---|---|---|---|
| `fund.eastmoney.com/data/rankhandler.aspx` | 全市场排行 | `pn=30000` → **20,360 只全量**（3.48 MB） | **批量最新净值**，见第 8 节 |
| `fund.eastmoney.com/data/FundGuideapi.aspx` | 全市场列表 | `pn=30000` → 20,363 条 | `rankhandler` 的备用解析面【一手+实测】 |
| `fund.eastmoney.com/pingzhongdata/<code>.js` | 单只全历史 | **只能 1 只**（多代码 301→notfound） | 一次给整只历史净值/持仓/费率等 |
| `api.fund.eastmoney.com/f10/lsjz` | 单只分页历史 | **`pageSize` 硬上限 20**（21–100 静默压回 20，≥500 返回 0 条） | 缺 Referer 返回 `ErrCode=-999` |
| `fundsuggest.eastmoney.com/FundSearch/api/FundSearchAPI.ashx` | 单只搜索 | 1 只 | 现有基金名称通道 |

---

## 4. (b) 新浪财经与腾讯财经

### 4.1 新浪股票/基金行情 `hq.sinajs.cn`【实测】

```text
https://hq.sinajs.cn/list=sh600000,sz000001,sh510300,sz159915,hk00700,gb_aapl,f_000001
```

- **必须带 `Referer`**（如 `https://finance.sina.com.cn/`），否则 HTTP 403【实测，与 easyquotation 源码一致】。
- **编码 GBK**，返回 `var hq_str_<code>="字段1,字段2,...";`。
- **批量上限【实测】**：500 / 600 / 700 / 750 / 800 / **850 只成功**（URL 7,674 B）；**900 只返回 HTTP 431 `Request Header Fields Too Large`**（URL 8,124 B）。→ 上限是 **请求 URI ≈ 8 KB**，约 850 只。
- **覆盖品种【实测】**：A 股（`sh/sz/bj`）、ETF/LOF（`sh/sz` 前缀）、指数（`sh000001`）、可转债（`sh113050`）、**港股 `hk00700`**、**美股 `gb_aapl/gb_msft/gb_tsla`**、**场外基金 `f_000001`**。
- 字段（股票）：名称,今开,昨收,现价,最高,最低,买一,卖一,成交量,成交额,…,日期,时间。
- 字段（基金 `f_`）：名称,单位净值,累计净值,前一日单位净值,净值日期,`<第 6 个字段语义未在本次确认>`（示例：`华夏成长混合A,1.25,3.823,1.235,2026-09-15,24.0598`）。
- **批量基金净值【实测】**：219 个 `f_` 代码一次请求，URL 1,996 B，**0.20 秒**返回 219 行 / 16.4 KB，无空行。这是「219 次 → 1 次」的直接收益。
- **历史数据**（单标的，非批量）：
  - 股票日 K：`https://money.finance.sina.com.cn/quotes_service/api/json_v2.php/CN_MarketData.getKLineData?symbol=sh600519&scale=240&ma=no&datalen=1023` —— 实测 `datalen=1023` 返回 1023 根。
  - 基金历史净值：`https://stock.finance.sina.com.cn/fundInfo/api/openapi.php/CaihuiFundInfoService.getNav?symbol=000001&datefrom=2001-01-01&dateto=<today>&page=1&num=10000` —— **实测 `num=10000` 一次返回该基金全部 6,007 个净值日，0.26 秒**。这比东财 `lsjz`（每页 20 条，同一只基金要约 300 页）高效得多，是**首刷历史回填**的强候选。
- **实时性**：免费 Level-1，收盘后仍返回当日收盘数据；**具体延迟无官方承诺，标【社区】**。
- **注册/付费**：免注册免费。
- **稳定性/封禁风险**：无官方契约，历史上出现过校验变化与限流；`Referer` 强制是已确认的约束【一手：库源码】【社区：封禁史】。
- **合规**：新浪行情属第三方数据，个人自用风险低，再分发需自评。
- **可替换性**：可直接实现 `fetch_ulist`（股票/ETF）、新增「批量基金最新净值」通道、替代 `fetch_nav_full`（单只全历史）。纯文本 + 少数几行解析，Rust 无需 Python。

### 4.2 腾讯财经 `qt.gtimg.cn`【实测】

```text
https://qt.gtimg.cn/q=sh600000,sz000001,sh510300,hk00700,usAAPL,sz161725
```

- **编码 GBK**，返回 `v_<code>="字段~字段~...";`。
- **批量上限【实测】**：200 / 500 / 800 / 850 / **900 只成功**（URL 8,118 B）；**950 只返回 HTTP 414 `Request-URI Too Large`**（URL 8,568 B）。→ 同样是 **URI ≈ 8 KB**，约 900 只。**社区常说的「腾讯 50–60 只上限」在本端点不成立**（可能源自旧路径或 easyquotation 的源码常量）。
- **覆盖品种【实测】**：A 股、ETF、LOF（`sz161725` 白酒基金 LOF）、指数、可转债、**港股 `hk00700`**、**美股 `usAAPL`**。**未见场外基金净值前缀**（与新浪不同）。
- 字段以 `~` 分隔：`[1]` 名称、`[2]` 代码、`[3]` 现价、`[4]` 昨收、`[5]` 今开……（LOF/ETF 同形）。
- **历史 K 线（单标的）**：
  ```text
  https://web.ifzq.gtimg.cn/appstock/app/fqkline/get?param=sh600519,day,2024-01-01,2026-09-15,640,qfq
  ```
  实测返回 `data.sh600519.qfqday`，每条 `[日期, 开, 收, 高, 低, 量]`。**count 上限实测约 800**：320/640/800 分别返回 320/640/800 根；**1000、2000 都被压回 640 根**。仍是**单标的**，无批量。
- **实时性/延迟**：免费 Level-1，无官方延迟承诺【社区】。
- **注册/付费**：免注册免费。
- **合规/可替换性**：同新浪；可作为 `fetch_ulist` 与 `fetch_kline` 的独立备选链路（与东财不同源，适合做故障切换）。

### 4.3 小结：新浪 vs 腾讯 vs 东财（免费三源）

| | 东财 `ulist.np` | 新浪 `hq.sinajs.cn` | 腾讯 `qt.gtimg.cn` |
|---|---|---|---|
| 批量报价上限【实测】 | ~500–800 secid（1000 触发 503） | ~850 只（900 → 431） | ~900 只（950 → 414） |
| 场外基金净值 | 批量走 `rankhandler`（另接口） | **`f_` 前缀直接批量** | 未见 |
| 港股/美股 | ✅（secid 前缀） | ✅（`hk_`/`gb_`） | ✅（`hk`/`us`） |
| 批量历史 K 线 | ❌ | ❌ | ❌ |
| 单只历史上限 | 按区间，实测 2 年 656 根 | 股票 `datalen=1023`；基金 `num=10000` 全历史 | `count≈800` |
| 需注册/付费 | 否 | 否 | 否 |
| 官方契约 | 无 | 无 | 无 |

---

## 5. (c) 聚合数据源库 / SDK

> 这一节的共同结论：**这些库没有一个是「Python 独占能力」**——它们绝大多数是同一批 HTTP 端点的封装，换库本身不会让 219 只更快；价值在于「知道端点与安全并发参数」以及「侧车桥接成本」。

| 库 | 底层真实来源 | 覆盖（A股/ETF/场外基金/港/美/加密） | 批量能力 | 实时性 | 免注册 | License | 官方 | 活跃度（2026-09） | Rust 接入 |
|---|---|---|---|---|---|---|---|---|---|
| **akshare** | 爬东财/新浪/同花顺/交易所等公开接口 | ✅/✅/✅/✅/✅/✅（金十） | 只有「全市场快照」；内部 `pz=100` + 每页 `sleep 0.5–1.5s` | 实时快照 + 日/分钟 K | 是 | MIT | 否 | commit 2026-08-28；1.18.94 | 官方 **AKTools** FastAPI 侧车，文档明示面向 Rust |
| **efinance** | 东财 push2/push2his/fundmobapi | ✅/✅/✅/✅/✅/❌ | `ulist.np` 一次多 secid；K 线多标的**并发上限 50**、重试 3 | 实时快照 + K 线 | 是 | MIT | 否 | commit 2026-07-17；0.5.9 | 无 HTTP 层 → 侧车；但端点可直抄 |
| **easyquotation** | 新浪 `hq.sinajs.cn`、腾讯 `qt.gtimg.cn`、集思录 | ✅/✅(场内)/❌/✅(腾讯)/❌/❌ | 源码常量：**新浪 `max_num=800`、腾讯 `max_num=60`**，多线程切分 | 实时快照；港股日 K | 是 | MIT（PyPI 元数据误标 BSD） | 否 | 2026-02-28 仅文档 | **最容易**：纯文本 HTTP，Rust 直接实现 |
| **baostock** | **自建服务器** `public-api.baostock.com:10030`（私有 TCP 协议） | ✅/✅/❌/❌/❌/❌ | `query_daily_history_k_ETF(date)` **一次返回当日全部 1627 只 ETF 日 K**；A 股按 symbol | 日/分钟频，**无实时** | 是（匿名登录实测成功） | PyPI 标 BSD，包内无 LICENSE 文本 | 服务方自营 | 0.9.3（2026-07-10） | 私有 TCP → 必须侧车；客户端无超时会挂起 |
| **pytdx / 通达信** | 直连券商行情服务器（逆向协议，硬编码 IP:7709） | 仅 A股/指数/基金 | `get_security_quotes` 一次多只；K 线 800 根/次 | 实时快照/分笔 | 是 | **仓库无 LICENSE** | 否（README 称「机构请不要使用」） | **2020-04-15 已归档** | 私有协议 → 侧车或重写；**已停维护** |
| **mootdx / tdxpy** | pytdx 的活跃衍生；另有本地 `.day` 文件路线 | 同上 | 同上 | 同上 | 是 | MIT | 否 | mootdx 2024-07 | 侧车；本地 `.day` 路线无网络亦无授权风险 |
| **qlib** | 不是行情源；CN 日线默认 Yahoo，基金净值取天天基金 | 研究数据集 | 无批量报价概念 | **无实时**，日频为主 | 是 | MIT | 微软开源、数据非官方 | commit 2026-09-02；v0.9.7 | 不适合做实时行情源 |

**关键事实【一手/实测】**

- akshare 的「全市场」接口**比直连东财更慢**（每页 100 条 + `sleep 0.5–1.5s`，约 5,400 只 A 股 ≈ 54 页），不适合做生产主干。
- efinance 的 `MAX_CONNECTIONS = 50` + 重试 3 次，是「该库作者认为安全的并发上限」，可作为你现有 Rust 实现的参数参照。
- baostock 是唯一「服务方自营数据」的免费库，且**一次调用能拿全市场 ETF 日 K**；但它是私有 TCP 协议、无实时、无场外基金净值，且客户端**未设超时**，一旦服务端不响应会无限期挂起【实测到疑似限流后长时间无响应，隔约 3 分钟恢复】。
- pytdx 仓库已被作者归档，**实质停维护**；若要走通达信路线，`mootdx` + 本地 `.day` 文件更现实。
- qlib **不是行情源**，其官方数据集当前已下线，README 建议改用社区数据集。

**对 219 标的场景的取舍**：efinance 当「批量 + 并发参数标定参照」，easyquotation 当「独立于东财的第二条链路」，akshare/AKTools 当「快速验证某数据点在哪个源能拿到」，baostock 当「ETF/指数批量补源」，pytdx/qlib 排除。

---

## 6. (d) 正式商用 / 注册型数据源

| 数据源 | A股 | ETF | 场外基金净值 | 港股 | 美股 | 加密 | 单请求批量 | 实时性 | 个人起步价（官方） | 形态 | Rust 直连 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| **Tushare Pro** | ✅ | ✅（5000 积分） | ✅（≥2000 积分） | ✅ 1000 元/年 | ✅ 2000 元/年 | ❌ | 日线 6000 行/次，多 `ts_code` | 日频；实时日线另付 200 元/月 | 200 元 = 2000 积分 | HTTP POST + Py 官方 SDK | ✅ 纯 HTTP |
| **聚宽 JQData** | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ | `run_query` 5000 行；`run_offset_query` 20 万条 | 日频为主 | 不公开（企微询价） | **Python/RPC SDK** | 需 Python 侧车 |
| **Wind** | ✅ | ✅ | ✅ | ✅ | ✅（40+ 交易所） | 未说明 | 机构级 | 实时（机构行情） | 不公开（Enterprise） | 终端 + SAPI/数据服务 | ❌ 需终端激活 |
| **东财 Choice** | ✅ | ✅ | ✅ | ✅ | ✅ | 未说明 | 流量计费 | 实时（终端） | 不公开（客户经理） | SDK（含 C++）+ 人工激活 | 需激活 |
| **AllTick** | ✅ | ✅ | ❌ | ✅ | ✅ | ✅ | `/batch-kline` ≤500 组；tick 建议 ≤50 | **WebSocket 实时 tick/盘口** | 免费档；9.99 USDT/月起 | REST + WebSocket | ✅ 最顺 |
| **Alpha Vantage** | ✅（无批量） | 部分 | ❌ | 文档未提 | ✅ | ✅ | `REALTIME_BULK_QUOTES` 仅美股 ≤100，且 premium | 免费延迟 | $49.99/月起（免费 25 次/天） | REST | ✅ |
| **Yahoo/yfinance** | ✅ | ✅ | ❌ | ✅ | ✅ | ✅ | `v7/quote?symbols=`（无官方上限文档） | 无承诺 | 免费（**非官方**） | REST（非官方） | ✅ 但**本次实测 429** |
| **Finnhub** | ❌ | 数据贵 | ❌ | ❌（仅 Enterprise） | ✅ | ✅ | 无公开 bulk quote | WS 实时美股 | 免费 60 次/分 | REST + WS | ✅ |
| **Massive（原 Polygon）** | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ | 全市场快照 | 实时需 $199 档 | 免费档 / $29 起 | REST+WS+FlatFiles | ✅ |
| **EODHD** | 沪深 EOD | 全球 ETF | 弱 | 未明确 | ✅ | ✅ | bulk EOD | 15 分钟延迟 + WS | €19.99/月 | REST+WS | ✅ |
| **同花顺 iFinD** | ✅ | ✅ | ✅ | ✅ | ✅ | 未说明 | 历史行情 5 万单元格/次 | 实时（按权限） | **免费版额度大**，正式版不公开 | SDK | 需 FFI/侧车 |
| **IEX Cloud** | — | — | — | — | — | — | — | **2024-08-31 已停运** | — | — | — |

**要点**

- **Tushare Pro** 是唯一在「个人可负担 + 覆盖 A股/ETF/场外基金净值 + 纯 HTTP 可直连 Rust」三项上都成立的正式源。个人档 200 元 = 2000 积分即可覆盖场外基金净值；港股日线 1000 元/年、美股日线 2000 元/年需单独加购。**服务协议明确为「个人的、不可转让的、非商业用途」许可，且禁止把数据对外分发**——个人记账自用合规，把它当作对外分发产品的数据源则违约。
- **AllTick** 支持支付宝（限大陆 IP）、REST + WebSocket，A 股全市场 59.99 USDT/月；缺点是**没有场外基金净值**，且 ToS 禁止再分发。
- **Alpha Vantage / Finnhub / Massive / Tiingo / EODHD** 对中国 A 股、港股与场外基金净值基本无解或不划算。
- **Yahoo/yfinance** 非官方、条款仅限个人研究、本次实测 `query1`/`query2` 直接 429，只能当兜底。
- **Wind / 东财 Choice / 恒生聚源 / 通联数据** 均为机构商务路径：能力没问题，但无公开个人档、依赖终端账号激活，对桌面应用直连属大炮打蚊子。

---

## 7. (e) 交易所 / 基金业协会 / 官方公开数据

| 来源 | 覆盖 | 批量能力【实测】 | 实时性 | 费用 | API 性质 | 合规要点 |
|---|---|---|---|---|---|---|
| **上交所官网 `yunhq`** | 沪市 A 股/基金/债券 | 一次全市场：股票 2,359、基金 1,050、债券 460 | 官网快照/收盘（盘中未见承诺） | 免费访问 | **官网内部接口，非公开契约** | 非商业可浏览/下载；实时与经营需 `sseinfo` 许可 |
| **深交所官网 `ShowReport`** | 深市股票/基金/债券/期权 | 日频 XLSX 一次全量（股票 2,941 行、基金 1,046 行） | 日频/收盘 | 免费访问 | 官网内部接口 | 基本行情许可官方页标 **30 万元/年/类型** |
| **证监会基金电子披露网站** | 场外公募（含货基/QDII/FOF/REITs） | 单基金区间查询；全市场一次 34,074 行（54.8 MB/28s） | 日频，当晚已更新 | 免费、免注册 | 官方披露页接口 | 法定披露渠道，权威性最高；仍应低频礼貌访问 |
| **基金业协会 AMAC** | 公募基金**月度**市场数据、机构/产品公示 | 月度 PDF | 月度/低频 | 免费 | 官方 | **不含逐只日净值** |
| **HKEX** | 港股/ETF/权证/REITs | 无免费批量 REST | 实时/延迟均需许可 | 许可费 | 官方许可产品 | 实时、延迟、历史均受许可约束 |
| **SEC EDGAR** | 美股申报/XBRL | 批量 ZIP 每晚约 3:00 ET | 无报价 | 免费 | 官方 API | **不含任何报价** |
| **CTA / Nasdaq Basic / NYSE** | 美股实时 | — | 实时 | 订阅 | 官方许可产品 | 需合同与费用；免费官网行情多为延迟展示 |

**官方场外基金净值：证监会基金电子披露网站**【实测】是本次调研里权威性最高的日频净值源：

```text
http://eid.csrc.gov.cn/fund/disclose/getPublicFundJZInfoMore.do   （DataTables 风格 aoData 参数）
```

- 单基金最新：1 行 / 1,671 B / 0.13 s；单基金近两年：484 行 / 777,373 B / 0.59 s；全市场「最新」：34,074 行 / 54.8 MB / 28.1 s。
- **不支持多代码**（`fundCode=110022,161725` 返回 0 行）；日期跨度 >1 天时必须指定 6 位基金代码。
- 字段：`code / shortName / shareNetValue（单位净值）/ totalNetValue（累计净值）/ valuationDate`；货基为 `gainPer / yearSevenDayYieldRatePercent`。
- 法定要求：开放式基金**不晚于每个开放日的次日**披露净值（证监会令第 158 号），指定渠道含该网站。

**结论**：官方源能覆盖「A 股/ETF 日频批量」与「场外基金官方日频净值」，但**历史 K 线**和**实时行情**在免费官方渠道都拿不到可零成本替换的能力。

---

## 8. 专题：场外基金净值（OFF-exchange Fund NAV）

### 8.1 估值 vs 净值，先分清

- **官方单位净值（NAV）**：基金公司核算后披露，日频，法定「不晚于次日」；用于份额确认与账本结算。
- **盘中估值（估算净值 `gsz`/`GSZ`）**：按持仓与实时行情估算，**不是官方净值**，QDII、债券型、持仓变动、停牌股、汇率都会造成偏差，**不能用于结算或份额确认**。本仓库现有语义也是「净值日期 + 单位净值」，与估值无涉。

### 8.2 各接口的批量能力【实测】

| 接口 | 一次能取多少只 | 明细 |
|---|---|---|
| 东财 `rankhandler.aspx` | **20,360 只（全市场）/ 1 次** | `pn=30000`；3.48 MB；仅最新净值 + 净值日期，**不含历史** |
| 新浪 `hq.sinajs.cn/list=f_...` | **~850 只/次（URI 8 KB）** | 219 只实测 1 次 / 0.20 s / 16.4 KB |
| 东财 `FundMNFInfo?Fcodes=` | **30 只/次** | 含官方净值 + 盘中估值字段；219 只需 8 次 |
| 新浪基金历史 `CaihuiFundInfoService.getNav` | 1 只/次，`num` 可达 10000 | 实测一只 6,007 个点一次返回 / 0.26 s |
| 东财 `pingzhongdata/<code>.js` | **1 只/次** | 一次给整只全历史（实测单只 56 万–76 万 B）；多代码 301 |
| 东财 `f10/lsjz` | 1 只/次，**每页硬上限 20 条** | 一只 6,007 个净值日 ≈ 300 页；缺 Referer 直接 -999 |
| 证监会 EID | 1 只/次（或全市场一次 54.8 MB） | 官方、权威、免注册 |

### 8.3 更新频率

- 官方净值：**每开放日一次，通常在当晚陆续更新**。实测同一天（2026-09-15）晚间：普通混合/股票基金已更新到当天，QDII 联接仍停在 2026-09-14（T-1）——**净值日期是逐只的，不是全市场统一日期**。
- 盘中估值：仅在交易时段内有值；本次收盘后 `FundMNFInfo` 的 `GSZ/GSZZL/GZTIME` 全为 `null`。

### 8.4 219 只基金：逐个请求 vs 批量请求的差距【实测】

| 方案 | 请求数 | 传输量 | 备注 |
|---|---:|---:|---|
| 现状（逐只 `pingzhongdata` / `lsjz` + 逐只名称） | 219–438+ | 视历史深度可达上百 MB | 还受 2 秒/请求节流；首刷全历史时 `lsjz` 路径可达数百页/只 |
| 东财 `rankhandler` 批量最新净值 | **1** | 3.48 MB | 只给最新净值；需把所需基金从 20,360 条里筛出 |
| 新浪 `f_` 批量最新净值 | **1** | 16 KB（219 只） | 只给最新净值；字段比 rankhandler 少 |
| 东财 `FundMNFInfo` 批量（含估值） | **8** | 每批约 5–7 KB | 唯一同时给「官方净值 + 盘中估值」的多代码接口 |
| 新浪单只全历史（首刷回填） | 219 | 每只 0.2–0.8 MB | 每只 **1 次请求**即可拿全历史，远优于 `lsjz` 分页 |
| 现状首刷（`pingzhongdata`，单只全历史） | 219 | 每只 0.5–0.8 MB | 已是「单只 1 次」，与新浪单只全历史同级 |

**因此对 219 只基金的最优组合大致是**：日常增量用**一次批量接口**（rankhandler 或新浪 `f_`）把最新净值全部刷新；首刷历史回填保留/改用「单只一次」的全历史通道（`pingzhongdata` 或新浪 `getNav?num=10000`）；名称随批量结果一起刷新（rankhandler/新浪都带名称），从而**省掉每只基金一次名称查询**。

---

## 9. 综合对比（按「能否直接解决 219 只同步慢」排序）

| 途径 | 覆盖 | 批量 | 实时性 | 成本 | 官方性/风险 | 个人自用合规 | 落进现有六通道抽象 |
|---|---|---|---|---|---|---|---|
| 东财 `ulist.np`（现有） | A/ETF/港/美 | ~500–800 secid/次 | 准实时/延迟 | 免费 | 非官方，WAF 限流 | 低风险 | `fetch_ulist`（提批大小即可） |
| 东财 `rankhandler` | 场外基金最新净值 | **20,360/次** | 日频 | 免费 | 非官方，3.5 MB/次 | 低风险 | 新增「批量最新净值」通道 |
| 新浪 `hq.sinajs.cn` | A/ETF/港/美/基金 | ~850/次（含 `f_` 基金） | 准实时 | 免费 | 非官方，需 Referer | 低风险 | `fetch_ulist` + 批量净值 + `fetch_nav_full` |
| 腾讯 `qt.gtimg.cn` | A/ETF/港/美 | ~900/次 | 准实时 | 免费 | 非官方 | 低风险 | `fetch_ulist` 备选 |
| 新浪基金历史 `getNav` | 场外基金全历史 | 1 只/次，全历史 | 日频 | 免费 | 非官方 | 低风险 | `fetch_nav_full` |
| 东财 `FundMNFInfo` | 场外基金净值+估值 | 30/次 | 日频 + 盘中估值 | 免费 | 非官方 | 低风险 | 新增「净值+估值批量」通道 |
| 证监会 EID | 场外基金官方净值 | 1 只/次 | 日频 | 免费 | **官方** | 最稳 | `fetch_nav` / `fetch_nav_full` |
| 上交所 `yunhq` / 深交所 `ShowReport` | 沪/深全市场日频 | 全市场/次 | 日频 | 免费 | 官网内部接口 | 非商业可浏览 | 需新增「全市场快照」适配器 |
| Tushare Pro | A/ETF/场外基金/港/美 | 6000 行/次 | 日频（实时另付） | ~200 元/年起 | **官方商用** | ✅ 个人非商业 | 纯 HTTP，可整体替换日频通道 |
| AllTick | A/ETF/港/美/加密 | 500 组/次 | 实时 WS | 9.99 USDT/月起 | 官方商用 | ✅ 个人（禁再分发） | REST/WS，可替换行情通道 |
| Yahoo/yfinance | 全球 | 未公开 | 无承诺 | 免费 | **非官方，实测 429** | 仅个人研究 | 不建议 |
| Alpha Vantage / Finnhub / Polygon / EODHD | 以美股为主 | 有限 | 各异 | 付费 | 官方商用 | ✅ | 对 A/港股无解 |

---

## 10. 事实与传闻分栏

### 有可靠证据支持（本次实测或一手来源）

1. 东财 `ulist.np/get` 一次 500–800 个 secid 可用，1000 个触发 503；`clist` 每页硬上限 100。
2. 东财 `push2his` K 线只能单个 secid（多 secid 返回 `rc=100, data:null`）。
3. 东财 `f10/lsjz` 的 `pageSize` 硬上限 20；`pingzhongdata` 不支持多代码。
4. 东财 `rankhandler.aspx` 的 `pn=30000` 一次返回全市场 20,360 只基金的最新净值。
5. 东财 `fundmobapi` `FundMNFInfo` 的 `Fcodes` 单请求上限 30，且含 `GSZ/GSZZL/GZTIME` 估值字段。
6. `fundgz.1234567.com.cn/js/<code>.js` 已失效（200 + 404 HTML）。
7. 新浪 `hq.sinajs.cn` 不带 Referer 返回 403；批量上限约 850 只（900 → HTTP 431）。
8. 腾讯 `qt.gtimg.cn` 批量上限约 900 只（950 → HTTP 414）；K 线 `count` 上限约 800。
9. 新浪 `f_` 前缀支持批量基金净值（219 只 1 次请求 / 0.20 s）；新浪基金历史可 `num=10000` 一次取全历史。
10. easyquotation 源码常量：新浪 `max_num=800`、腾讯 `max_num=60`；efinance 并发上限 50。
11. baostock 为自建 `public-api.baostock.com:10030` 私有 TCP 协议，一次可返回全市场 ETF 日 K（实测 1627 行）。
12. pytdx 仓库已归档（2020-04-15）；qlib 的 CN 日线默认 Yahoo、官方数据集已下线。
13. 证监会基金电子披露网站可免注册查官方净值（单基金区间、全市场 34,074 行）。
14. 深交所基本行情许可费公开为 30 万元/年/应用类型；上交所/深交所法律声明允许非商业浏览下载、商业使用需许可。
15. Tushare Pro 服务协议为「个人的、非商业用途、不可转让」许可。
16. IEX Cloud 已于 2024-08-31 停运。

### 传闻 / 社区经验（未获一手证据，需自行验证）

- 「东财 `push2delay` = 15 分钟延迟」——只确认了它被用作延迟主机，未见官方延迟说明。
- 「新浪约 800 只上限」——方向正确，但本次实测到 850 只仍成功，真正约束是 URI ≈ 8 KB。
- 「腾讯 50–60 只上限」——在本端点不成立（实测 900 只成功）；可能源自旧路径或库内保守常量。
- 「pytdx 单请求约 80 只上限」——其源码未见分片逻辑。
- 「新浪/腾讯会被封 IP、需要代理池」——只有 Referer 强制与限流提示可佐证。
- 「baostock 免费版有速率限制」——本次实测到疑似限流（长时间无响应后恢复），但官方无明文。

---

## 11. 给这个场景的可执行建议（不涉及代码改动）

1. **先把基金最新净值批量化**：日常增量改成一次批量请求（`rankhandler` 或新浪 `f_`），把 219 次「净值 + 名称」压成 1 次，这是收益最大、风险最低的一步。
2. **把全局 2 秒串行节流改为受控并发**：efinance 的「并发 50 + 重试 3」是现成的参数量级；东财 WAF 下可从更保守的值起步。这一步直接影响 K 线回填时间。
3. **历史 K 线**：东财没有批量接口，短期只能靠受控并发；若只关心日频，可考虑用交易所官网的「全市场日频文件/快照」一次取全市场再本地过滤（需接受日频粒度与官网内部接口的稳定性）。
4. **首刷历史回填**：基金侧保留「单只一次全历史」通道（`pingzhongdata`），或用新浪 `getNav?num=10000` 作为独立备选。
5. **估值**：`fundgz` 已死；用 `FundMNFInfo` 的 `GSZ/GSZZL/GZTIME`，或 `GetFundGZList`（需盘中复核）。**估值不得用于结算/份额确认**。
6. **合规底线**：免费源都是非官方接口，个人自用风险低、**对外分发需重新评估**；若未来要分发，Tushare Pro 等也明确禁止非个人用途，需走交易所/数据商正式许可。

---

## 12. 主要来源

**东方财富 / 天天基金**

- 东方财富行情页（接口宿主）：<https://quote.eastmoney.com/>
- 天天基金净值估算页：<https://fund.eastmoney.com/fundguzhi.html>
- 天天基金净值排行：<https://fund.eastmoney.com/data/fundranking.html>
- 基金档案（pingzhongdata 宿主）：<https://fund.eastmoney.com/>
- akshare 对上述接口的现行实现：<https://github.com/akfamily/akshare/blob/main/akshare/fund/fund_em.py>、<https://github.com/akfamily/akshare/blob/main/akshare/fund/fund_rank_em.py>
- efinance 对东财端点的封装：<https://github.com/Micro-sheep/efinance/blob/main/efinance/common/getter.py>、<https://github.com/Micro-sheep/efinance/blob/main/efinance/fund/getter.py>

**新浪 / 腾讯**

- 新浪行情：<https://finance.sina.com.cn/>（`hq.sinajs.cn`、`money.finance.sina.com.cn`、`stock.finance.sina.com.cn`）
- 腾讯行情：<https://gu.qq.com/>（`qt.gtimg.cn`、`web.ifzq.gtimg.cn`）
- easyquotation 批量上限源码：<https://github.com/shidenggui/easyquotation/blob/master/easyquotation/basequotation.py>

**聚合库 / SDK**

- akshare：<https://github.com/akfamily/akshare> ・ <https://akshare.akfamily.xyz/introduction.html> ・ <https://akshare.akfamily.xyz/special.html>
- AKTools（HTTP 侧车，文档明示面向 Rust）：<https://aktools.readthedocs.io/>
- efinance：<https://github.com/Micro-sheep/efinance>
- easyquotation：<https://github.com/shidenggui/easyquotation>
- baostock：<https://pypi.org/project/baostock/> ・ <http://www.baostock.com>
- pytdx（已归档）：<https://github.com/rainx/pytdx>；衍生 <https://github.com/mootdx/mootdx>
- qlib：<https://github.com/microsoft/qlib>

**商用 / 注册型**

- Tushare Pro：<https://tushare.pro/document/2> ・ 权限表 <https://tushare.pro/document/1?doc_id=108> ・ 频次表 <https://tushare.pro/document/1?doc_id=290> ・ 服务协议 <https://tushare.pro/document/1?doc_id=405>
- 聚宽 JQData：<https://www.joinquant.com/help/api/doc?name=JQDatadoc> ・ <https://github.com/JoinQuant/jqdatasdk>
- Wind：<https://www.wind.com.cn/portal/zh/WDS/sapi.html>
- 东财 Choice：<https://quantapi.eastmoney.com/>
- AllTick：<https://alltick.co/en-us/pricing> ・ <https://alltick.co/apis/en/getting-started/http-interface-restrictions>
- Alpha Vantage：<https://www.alphavantage.co/documentation/> ・ <https://www.alphavantage.co/premium/>
- Yahoo Finance 条款：<https://legal.yahoo.com/us/en/yahoo/terms/product-atos/apiforydn/index.html>；yfinance：<https://github.com/ranaroussi/yfinance>
- Finnhub：<https://finnhub.io/docs/api> ・ <https://finnhub.io/pricing>
- EODHD：<https://eodhd.com/pricing>
- 同花顺 iFinD：<https://quantapi.51ifind.com/>

**交易所 / 官方**

- 上交所：Level-1 <https://www.sseinfo.com/services/assortment/level1/> ・ 授权声明 <https://www.sseinfo.com/aboutus/authstatement/> ・ 法律声明 <https://www.sse.com.cn/home/legal/>
- 深交所法律声明：<https://www.szse.cn/application/laws/index.html>；深证信许可费标准：<http://www.szsi.cn/cpfw/fwsq/hq/sfbz.htm>
- 证监会基金电子披露网站：<http://eid.csrc.gov.cn/fund/disclose/index.html> ・ 信披管理办法（令第 158 号）<https://www.csrc.gov.cn/csrc/c101877/c1029542/content.shtml>
- 中国证券投资基金业协会：<https://www.amac.org.cn/sjtj/>
- HKEX 行情与许可：<https://www.hkex.com.hk/Services/Market-Data-Services/Real-Time-Data-Services/Overview?sc_lang=en>
- SEC EDGAR API：<https://www.sec.gov/edgar/sec-api-documentation>
- CTA：<https://www.ctaplan.com/pricing>；Nasdaq Basic：<https://www.nasdaq.com/products/data/equities/nasdaq-basic>；NYSE 数据产品：<https://www.nyse.com/data-products>

---

## 13. 2026-09-19 复核：替代源实测与决策落地（ADR-0130 决策依据）

本节是「移除东方财富、按链路分工选源」这一决策（ADR-0130）的直接依据，票面为 #1554 及其子票 #1555–#1572。第 1–12 节记录的是 2026-09-15 的原始调研，仍然有效；本节只补新测到的形态、陷阱与量级。

**证据的时效与环境 caveat**：全部取自 2026-09-19（周六）单日、单出口，且本机 DNS 被代理接管为 fake-IP 段（`dig @223.5.5.5` 亦返回 `198.18.x`，仅 HTTPS DoH 能取到真实地址）。因此东财 K 线的空响应**不排除**是该出口路径的处置；本节结论只用于「可达性与报文形态」的比较，不用于断言任何一方的长期稳定性。当日 A 股、港股与基金净值均停在 2026-09-18 收盘态。

### 13.1 当日东财可用性（对照基线）

| 面 | 结果 |
|---|---|
| 历史 K 线 `push2his` | **0/6 全失败**：`push2his` 及 `21`/`40`/`70` 子域、`push2` 均返回连接层空响应（`curl: (52) Empty reply from server`）；同主机 `stock/get` 时通时断 |
| 实时主机 `push2` 批量报价 | **0/5 全失败**（同为空响应） |
| 延迟主机 `push2delay` 批量报价 | 5/5 成功（100 只 5.4 KB / TTFB 0.13s） |
| 搜索面（股票 / 基金） | 正常 |
| 基金净值面（`rankhandler` / `f10/lsjz`） | 必须带 Referer，带上即正常；缺 Referer 返回 `ErrCode=-999` |

含义：**历史 K 线是唯一没有降级通道的链路**——它一断，价格历史后台补全整体停摆，这正是使用者最先撞到的症状。

### 13.2 腾讯行情报价 `qt.gtimg.cn`（场内主源）

- **批量**：一次请求携带多只，实测 100 只 39,624 B / TTFB 0.15s；支持沪深、港股、美股与场内基金（ETF / LOF）。
- **编码**：GBK。**无需 Referer**。
- **字段数按市场不同**：A 股 88、港股 78、美股 73。**类型码位置随之不同，不能按固定下标取**：

| 市场 | 类型码位置 | 观测取值 | 币种位置 |
|---|---|---|---|
| A 股 | 62 | `GP-A` / `ETF` / `LOF` / `ZQ-KZZ` | 83（CNY） |
| 港股 | 64 | `GP` | 76（HKD） |
| 美股 | 57 | `GP` / `GP-ETF` | 36（USD） |

- **美股交易所后缀**（字段 3）：`AAPL.OQ`（纳斯达克）/ `BABA.N`、`IBM.N`（纽交所）/ `SPY.AM`（美交所）——与既有市场闭集三值一一对应，因此市场闭集无需改动。
- **含义**：类型、币种与精确市场都能继续自动给出，不必改成用户手选。

### 13.3 腾讯 K 线 `web.ifzq.gtimg.cn`（历史补全）

- **沪深港美均可用**：茅台近两年 800 根 / 48,971 B / 0.24s；`hk00700` 与 `usAAPL` 均正常返回日线。
- **根数上限实测约 800**（近两年约 490 个交易日，够用）。
- **对照**：新浪 K 线（`money.finance.sina.com.cn` 的 `getKLineData`）**仅覆盖 A 股**——`symbol=hk00700` 与 `gb_aapl` 均返回 `null`。因此 **K 线只能由腾讯承担**，这是分链路选源里最硬的一条约束。

### 13.4 新浪：报价与场外基金（批量净值主源）

- **混合批**：`hq.sinajs.cn/list=` 一次可同时携带沪深、港股、美股与场外基金净值；实测 10 只混合（股票 + ETF + 港股 + 美股 + 4 只场外基金）1,981 B / 0.12s。**必须带 Referer**（缺则 403），GBK 编码。
- **场外基金批量**（`f_` 前缀）：100 只 7,308 B / 0.12s；219 只 1 次 / 0.20s。
- **单只全历史**（`CaihuiFundInfoService.getNav`）：一次可取整只历史；**已终止基金同样可取**——实测 `002503` 返回 1,771 条，末点 2023-09-18 / 1.144，与官方披露、东财三方差值一致。
- **陷阱（必须显式处理）**：**货基行把万份收益放在单位净值位**。实测 `f_000198` 返回 `天弘余额宝货币,0.2229,0.824,,2026-09-19,6799.46`——第 2 字段是万份收益而非单位净值。不识别就复现「货基市值少计约七成」（#1342）。

### 13.5 证监会基金电子披露（判定与权威兜底）

- **参数门槛**：DataTables 风格请求必须完整携带 `sEcho` / `iDisplayStart` / `iDisplayLength` 等参数，缺则直接返回 500「系统异常」（不是空数据）。
- **记录形态陷阱**：同一日期可能返回**两条记录**——一条汇总行（名称不带份额后缀、字段全空）与一条份额行（名称带 A/C 后缀、字段有值）。解析必须先过滤空值行再按份额匹配；只看第一条会取到空净值。
- **货基自报形态**：单位净值为空、`gainPer`（万份收益）与七日年化有值——与 ADR-0126 决策 3 依赖的第三类信号同形。
- **已终止基金**：#1212 记录的 5 只清盘基金 5/5 命中，名称、最后一期单位净值与净值日期与东财证据逐值一致（如 `002503` → 1.144 / 2023-09-18）。
- **全市场单日面**：不传基金代码、起止同日时 `iTotalRecords = 35,193`，服务端分页可用（该面本决策不使用，仅记录能力）。
- **不含**：基金分类展示串（其分类码不区分货基 / ETF / 混合），因此分类字段在替代方案里无来源。
- **性质**：官方数据，但为站点内部接口，**无公开契约**——数据权威不等于接口有契约。

### 13.6 决策落地与票面映射

ADR-0130 定稿的源分工与对应实施票：

| 链路 | 采用源 | 票 |
|---|---|---|
| 场内现价与报价（含类型、币种、精确市场） | 腾讯行情报价 | #1558（取数层）、#1560（接线）、#1567（查询与创建） |
| 场内历史 K 线（沪深港美） | 腾讯 K 线 | #1559、#1561 |
| 场外基金净值（批量 + 单只全历史） | 新浪 | #1564、#1565、#1566、#1568 |
| 货基判定与基金存在性 / 已终止兜底 | 证监会基金电子披露 | #1562、#1563、#1568 |
| 汇率 | ECB | 独立议题（#1540） |

三条预重构（#1555 / #1556 / #1557）把「查询键构造」与「页形态命名」从编排里剥出，使上述换源不必再动编排。

### 13.7 本节新增的来源

- 腾讯行情报价与 K 线：<https://gu.qq.com/>（`qt.gtimg.cn`、`web.ifzq.gtimg.cn`）
- 新浪行情与基金：<https://finance.sina.com.cn/>（`hq.sinajs.cn`、`stock.finance.sina.com.cn`）
- 证监会基金电子披露：<http://eid.csrc.gov.cn/fund/disclose/index.html>
