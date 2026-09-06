# Changelog

本文件记录开源记账（OpenLedger）各版本对使用者可见的变更，格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/)，版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)规则。

## [Unreleased]

### Added

- **设置**：路径与备份文件新增「复制路径」「在访达中显示」按钮（[#653]）。
- **设置**：备份文件列表新增「刷新」按钮（[#651]）。
- **备份**：备份与恢复跟随加密语义——列表新增「加密」列，恢复时模式不一致显著警告，密文备份需输对应主口令（[#572]）。
- **加密**：新增加密最小闭环——设置页开启加密并设主口令，整库一次性转换，启动经解锁屏进入（[#570]）。
- **加密**：新增「关闭加密」与「修改主口令」，复用同一套整库转换机制（[#571]）。
- **加密**：解锁屏新增「忘记口令」——重置为全新明文空库，旧库保留为密文副本（[#573]）。
- **加密**：新增「本机记住主口令」——口令缓存系统钥匙串，下次启动 Touch ID 认证后自动解锁（[#574]）。
- **加密**：开发/未签名构建的自动解锁免 Touch ID 门（[#662]）。
- **界面**：新增金额隐私模式，一键掩码全应用金额数字（[#566]）。
- **报表**：商户消费排行每行新增交易笔数（[#617]）。
- **设置**：About 页新增日志等级配置（[#611]）。
- **备份**：启动失败不再弹窗退出，改由恢复屏接管，可重置为空库或从备份恢复；顺带修复恢复路径错位等两处缺陷（[#601]）。
- **备份**：启动失败恢复屏新增「从备份文件恢复」通道（[#602]）。
- **备份**：解锁屏新增「从备份文件恢复」入口，密文库无需先解锁即可恢复（[#603]）。

### Changed

- **应用更名**：应用更名为「开源记账」（OpenLedger），仅改显示层，数据与升级路径不变（[#584]）。
- **交易**：转账/买入/卖出携带分类改为报错，不再静默落库（[#582]）。
- **报表**：商户消费排行改为表格，商户名可点击下钻（[#618]）。
- **设置**：备份页签卡片重排，一键备份免滚动直达，恢复入口降为次要形态（[#651]）。
- **加密**：危险确认统一升级为应用内弹窗；主口令设最短 8 位（[#650]）。

### Fixed

- **界面**：修复「移回侧栏」菜单提示文案溢出容器的问题（[#647]）。
- **加密**：开发/未签名构建下，设置页与解锁屏的自动解锁提示不再误标 Touch ID，按运行形态区分文案（[#687]）。

## [0.5.0] - 2026-09-05

### Added

- **搜索**：新增时间范围快捷选择（芯片＋步进器＋直达面板），与报表页同构（[#526]）。
- **界面**：新增全局忙碌条；命令 async 化后界面不再卡顿（[#500]）。
- **发布**：Windows/Linux 安装包进入发布矩阵（[#496]）。
- **设置**：新增拼音搜索数据一键修复命令（[#513]）。

### Changed

- **性能**：交易搜索 SQL 下推，50 万笔库搜索 p95 154ms→56ms（[#515]）。
- **性能**：搜索取行两段式优化，拼音搜索 p95 2932ms→155ms（[#492]）。
- **性能**：余额与净资产持久化缓存，总览读取从 ~88s 大幅下降（[#491]）。
- **性能**：新增 6 条覆盖索引，批量导入后自动重跑统计（[#490]）。
- **依赖**：前端依赖升级（pinia 3→4 等；TypeScript 刻意留在 6.x）。
- **依赖**：zip crate 2→8.6，备份文件新旧版本互通。

## [0.4.0] - 2026-09-04

### Added

- **报表**：新增期间筛选，三张报表卡随期间重算（[#411]）。
- **保险**：新增保单视图，支持保单档案的新建、编辑与软删除（[#360]）。
- **保险**：保单展示累计保费、现金流入与下期扣款日（[#363]）。
- **界面**：侧栏改为「记账 / 资产 / 洞察」三组（[#359]）。
- **概览**：新增「财务自由度」卡片（[#344]）。
- **定时计划**：新增自动执行，到期期次自动追补落账（[#307]、[#308]）。
- **投资**：基金申赎记账，按确认单录入金额与份额、赎回 FIFO 匹配（[#302]）。
- **投资**：输入 6 位代码自动拉取基金名称、类型与净值（[#301]）。
- **投资**：「同步持仓价格」股票与场外基金一起刷新（[#303]）。
- **投资**：无行情数据源的标的支持手动录价（[#291]）。
- **AI 导入**：新增基金查询与标的创建端点（[#304]）。
- **AI 导入**：新增标的搜索与幂等创建端点（[#294]、[#296]）。

### BREAKING

- **数据库 schema**：投资价格列刻度由「分」重定义为万分之一元，新增净值日期列。仅全新安装正确；**存量库直接升级会按 100 倍错读且缺列，不提供自动修复**（[#300]）。
- **数据库 schema**：定时交易外键补全显式 `ON DELETE` 动作，仅影响全新安装，存量库行为零差异（[#273]）。

### Changed

- **界面**：交易金额与月度收支图按六种交易类型语义着色（[#435]）。
- **报表**：分类下钻跳转随所选期间携带日期边界（[#412]）。
- **搜索**：金额区间筛选改按本位币分过滤（[#395]）。
- **分类**：删除名下有预算的分类明确拒绝并引导先删预算（[#355]）。
- **投资**：价格展示与录入升级为万分之一元刻度（[#300]）。
- **AI 导入**：契约注明价格例外——单价与现价以万分之一元为单位（[#298]）。
- **AI 导入**：入口提示词最小化为三步骨架（[#286]）。

### Fixed

- **稳定性**：修复写操作与行情同步期间界面永久卡死的跨线程死锁（[#364]、[#365]、[#369]）。
- **AI 导入**：修复导入后前端商户字典不自动刷新（[#331]）。
- **AI 导入**：引用不存在标的改返回 400 中文错误，批量导入降为逐行失败，AI 可自纠（[#295]）。
- **日志**：修复「打开日志目录」按钮始终报错（[#283]）。

## [0.3.0] - 2026-08-27

### Added

- **交易过滤**：交易页新增账户/日期/类型组合筛选与一键清除。
- **账户名下钻**：账户列可点击，跳转并按该账户过滤；转账行双向账户名各自可点。
- **搜索**：新增搜索视图，支持关键字、金额区间与日期范围组合筛选。
- **分页**：交易与投资标的列表改服务端分页，支持页大小切换。
- **备份**：新增文件级备份与恢复，支持保留上限与自动滚动清理。
- **导入**：批量导入按幂等键去重；新增按 id 全字段替换交易接口。
- **快捷键**：视图切换快捷键（Cmd/Ctrl+1..9）。
- **状态记忆**：持久化窗口位置/大小与各视图查看状态。

### Changed

- **外观**：主题定制——琥珀强调色、更大圆角、暗色分层；评估后决定留在 Naive UI。
- **日志**：数据库操作耗时日志上线，慢查询 warn 级。

### Fixed

- **分类**：修复界面无法显示外部新增的分类数据。
- **交易列表**：修复列宽被拉伸及备注溢出。
- **界面**：禁用 overscroll 橡皮筋，消除滚动白条。

## [0.2.0] - 2026-08-22

### Added

- **AI 导入**：新增提示词视图，可在应用内查看并一键复制 AI 导入入口提示词。

## [0.1.1] - 2026-08-22

### Changed

- **依赖**：升级后端 Rust 依赖，HTTP 客户端切换为 rustls。

### Fixed

- **构建**：修复 clippy 警告，CI 补充后端检查。

## [0.1.0] - 2026-08-22

### Added

- **账本**：首个可用版本——多币种账户、支出/收入/转账、分类管理、预算与报表。
- **投资**：投资账户与股票标的、买卖交易、FIFO 卖出匹配、东方财富全量同步。
- **AI 导入**：本地 HTTP API 供 AI 助手幂等写入账户/分类/交易，附 OpenAPI 文档。
- **计划交易**：定时交易模块（订阅/分期/转账）。
- **日志**：按天滚动日志，设置页可打开日志目录。
- **发布**：macOS DMG 构建，打 v* tag 自动发布 GitHub Release。

### Fixed

- **同步**：修复港股漏抓、进度条重置、启动崩溃、JPY 精度等问题。

<!-- Unreleased 条目引用的 issue 链接（引用式链接，正文保持简洁） -->

[#273]: https://github.com/asiazhang/ledger/issues/273
[#283]: https://github.com/asiazhang/ledger/issues/283
[#286]: https://github.com/asiazhang/ledger/issues/286
[#291]: https://github.com/asiazhang/ledger/issues/291
[#294]: https://github.com/asiazhang/ledger/issues/294
[#295]: https://github.com/asiazhang/ledger/issues/295
[#296]: https://github.com/asiazhang/ledger/issues/296
[#298]: https://github.com/asiazhang/ledger/issues/298
[#300]: https://github.com/asiazhang/ledger/issues/300
[#301]: https://github.com/asiazhang/ledger/issues/301
[#302]: https://github.com/asiazhang/ledger/issues/302
[#303]: https://github.com/asiazhang/ledger/issues/303
[#304]: https://github.com/asiazhang/ledger/issues/304
[#307]: https://github.com/asiazhang/ledger/issues/307
[#308]: https://github.com/asiazhang/ledger/issues/308
[#331]: https://github.com/asiazhang/ledger/issues/331
[#344]: https://github.com/asiazhang/ledger/issues/344
[#355]: https://github.com/asiazhang/ledger/issues/355
[#359]: https://github.com/asiazhang/ledger/issues/359
[#360]: https://github.com/asiazhang/ledger/issues/360
[#363]: https://github.com/asiazhang/ledger/issues/363
[#364]: https://github.com/asiazhang/ledger/issues/364
[#365]: https://github.com/asiazhang/ledger/issues/365
[#369]: https://github.com/asiazhang/ledger/issues/369
[#395]: https://github.com/asiazhang/ledger/issues/395
[#411]: https://github.com/asiazhang/ledger/issues/411
[#412]: https://github.com/asiazhang/ledger/issues/412
[#435]: https://github.com/asiazhang/ledger/issues/435
[#490]: https://github.com/asiazhang/ledger/issues/490
[#491]: https://github.com/asiazhang/ledger/issues/491
[#492]: https://github.com/asiazhang/ledger/issues/492
[#496]: https://github.com/asiazhang/ledger/issues/496
[#500]: https://github.com/asiazhang/ledger/issues/500
[#513]: https://github.com/asiazhang/ledger/issues/513
[#515]: https://github.com/asiazhang/ledger/issues/515
[#526]: https://github.com/asiazhang/ledger/issues/526
[#566]: https://github.com/asiazhang/ledger/issues/566
[#582]: https://github.com/asiazhang/ledger/issues/582
[#584]: https://github.com/asiazhang/ledger/issues/584
[#570]: https://github.com/asiazhang/ledger/issues/570
[#571]: https://github.com/asiazhang/ledger/issues/571
[#572]: https://github.com/asiazhang/ledger/issues/572
[#573]: https://github.com/asiazhang/ledger/issues/573
[#601]: https://github.com/asiazhang/ledger/issues/601
[#602]: https://github.com/asiazhang/ledger/issues/602
[#603]: https://github.com/asiazhang/ledger/issues/603
[#617]: https://github.com/asiazhang/ledger/issues/617
[#618]: https://github.com/asiazhang/ledger/issues/618
[#650]: https://github.com/asiazhang/ledger/issues/650
[#647]: https://github.com/asiazhang/ledger/issues/647
[#662]: https://github.com/asiazhang/ledger/issues/662
[#687]: https://github.com/asiazhang/ledger/issues/687
[#651]: https://github.com/asiazhang/ledger/issues/651
[#653]: https://github.com/asiazhang/ledger/issues/653
