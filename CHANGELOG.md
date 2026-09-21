# Changelog

本文件记录开源记账（OpenLedger）各版本对使用者可见的变更，格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/)，版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)规则。

## [Unreleased]

### BREAKING

- **AI 导入 / HTTP API**：`GET /api/v1/transactions` 缺省不再返回全部——HTTP 单请求行数上限 100，缺省等价第一页 × 100（`total` 恒返回，读全部须按 `total` 翻页取齐）；显式 `page_size` 超过 100 报 400 码化错误 `transaction.page-size-over-cap`，`limit` 超过 100 或负值报 400 `transaction.limit-out-of-range`。读回核对教学同步改为分页读回；IPC 通道分页语义不变（前端与多端同步不受影响）（[#1631]）。

### Added

- **交易**：非本位币交易随行留痕折算来源——每笔记录本笔折算使用的汇率值与来源（「来自汇率序列」/「来自调用方显式给定」，后者写入入口由后续票接入），列表与详情可读回解释「这个本位币金额是怎么来的」；与本位币同币种、不折算（份额调整）及存量行留痕为空（[#1548]，只增不改、存量库免重建）。

### Changed

- **性能**：报表商户排名加商户维度覆盖索引（V030）并钉定查询计划，50 万笔库聚合 p95 777ms→148ms（[#1655]）。

## [0.7.0] - 2026-09-19

### Added

- **投资**：货币基金改按「价格恒定标的」处理——单位净值恒 1.0000（收益以份额结转体现），价格来源显示「恒定价格」、走势直出常量线，净值日期留空；同步不再为它请求行情，也不计入缺口统计与进度（[#1450]、[#1451]，ADR-0126）。
- **账户**：信用卡账户新增「信用额度」「账单日 / 还款日」档案——新建与编辑可填，列表显示已用额度百分比，编辑弹窗显示可用额度、使用率与下次账单日；额度仅为档案，不进余额与报表口径（[#1327]，ADR-0119）。
- **投资**：新增资金加权收益率（XIRR）——持仓页每标的、盈亏页账户级与全账级给出年化内部收益率（实际天数 / 365），支持区间选择，按币种分组不折算，缺价持仓不计入；期初存量（补记）持仓给未年化口径，只有存量时整项降级，有了真实流水后起算点顺延到首笔流水（[#1195]、[#1343]、[#1345]、[#1346]，ADR-0115）。
- **投资**：新增份额调整（拆股 / 送股 / 份额折算与结转）记录——份额变多、变少而现金不动，余额与已实现盈亏不变；记录可修改与删除，改删按逐批次重述审计回补（[#1049]、[#1051]）。
- **投资**：新增现金分红记录——归属某标的的现金收入（到账账户不限投资账户），不摊薄成本、不改份额；「累计收益」扩为持仓收益 + 已实现盈亏 + 累计分红三腿（[#1078]，ADR-0109）。
- **投资**：分红按标的计数、按币种分组进累计收益，可修改与软删，累计收益实时回退（[#1078]）。
- **交易**：交易列表类型筛选与只读详情新增「分红」类型，分红行只读呈现、无编辑与软删入口（[#1078]，ADR-0109）。
- **AI 导入**：导入知识新增现金分红教学，`dividend` 可用（[#1078]）。
- **AI 导入**：新增投资知识端点 `GET /api/v1/import/knowledge/investment`——投资交易 / 申赎 / 转换 / 份额调整 / 现金分红五节教学与对账口径；基础知识端点只留索引与指针（[#1123]、[#1124]，ADR-0110）。
- **投资**：「同步标的信息」新增逐标的进度条与计数（[#897]）。
- **账本**：新增多账本——可新建、改名、移除与切换，现有数据自动成为「默认账本」（[#831]、[#834]）。
- **账本**：多账本适配备份与自动解锁缓存；「更改数据位置」改为仅搬移当前账本（[#836]）。
- **账本**：新增跨账本投资汇总——把各本账本的持仓市值、持仓收益、累计收益与可投资资产合并为只读合计，本位币不一致时按当期汇率折算并标注口径，加密未解锁或未初始化的账本逐本标注「未计入」（[#1196]，ADR-0114）。
- **多端同步**：记账动作写入可同步操作日志（OpLog），重复送达不重复记账（[#855]）。
- **多端同步**：双端并发改动按确定性规则合并，定时执行幂等防双扣；无法执行的操作进挂起清单（[#856]）。
- **多端同步**：支持检查点快照，新设备凭快照引导、只重放增量即达一致（[#857]）。
- **多端同步**：内置 S3 兼容对象存储通道——自备存储桶 + 显式凭据，支持自定义 HTTPS 端点与 path-style 寻址，同步包可选加密上云（[#859]、[#1216]）。
- **多端同步**：本位币基准随同步在各设备一致生效；原「默认币种」更名为「展示币种」，仅影响本机展示（[#858]）。
- **多端同步**：定时计划、账户/分类/商户字典、预算、保险、物品与实物资产的变化随同步分发（[#860]）。
- **多端同步**：投资买入/卖出、标的字典、汇率与手动报价及 AI 导入的数据变化随同步分发（[#861]）。
- **多端同步**：设置页新增同步卡片——上次同步时间与挂起操作数、手动同步、S3 通道配置（保存前可「测试连接」并分层报错）；密文库经主口令加密上通道，明文库同步显著提示（[#862]、[#1218]、[#1219]）。
- **多端同步**：打开应用即同步、运行期每 10 分钟低频轮询、记账后约 5 秒上传；挂起明细按原因本地化（[#863]）。
- **多端同步**：Android 手机接入——共享同一套域引擎与界面，配置通道即可加入，空账本经检查点快照引导（[#864]）。
- **多端同步**：通道配置提供厂商预设下拉，选中预填端点、地域与寻址方式，加载 / 保存按端点反查回显（[#1220]）。
- **移动端**：窗口 <840 切换移动档导航壳（顶栏 + 抽屉），适配刘海与手势条安全区（[#842]）。
- **移动端**：触屏交互适配——交易行常显「⋯」菜单，悬停信息点按可达（[#843]）。
- **移动端**：交易页窄窗口收为卡片列表，新增「记一笔」悬浮按钮；搜索结果同构复用（[#846]）。
- **移动端**：Android 返回键 / 手势获得应用内语义——先关最上层弹层、无弹层则路由回退（[#845]）。
- **移动端**：概览与账户页移动档适配——栅格单列、账户列表三分列、表单纵排（[#847]）。
- **移动端**：预算与定时计划页移动档适配——列表三分列，暂停 / 恢复 / 取消以 ≥48px 触控按钮可达（[#848]）。
- **移动端**：报表、投资与收纳页 / 设置移动档适配；桌面专属管理动作在移动档隐藏（[#849]）。
- **AI 导入**：新增商户改名端点 `PUT /api/v1/merchants/{id}`（[#884]）。
- **AI 导入**：导入知识新增查询与分页纪律——服务端过滤子集检查、以 `total` 核对总数、buy/sell 关联认 `source`（[#928]）。
- **交易**：交易页筛选与分页会话内保留，切走再回原样恢复（[#893]）。
- **投资**：买入 / 卖出可选「出资账户」——现金实际流出 / 流入的账户纳入余额与资金流归因（[#935]）。
- **投资**：交易列表买入 / 卖出行展示「出资账户 → 投资账户」双链接，按账户筛选命中其出资的买卖（[#937]）。
- **多端同步**：买入 / 卖出的出资账户随同步一致重放，旧版本操作照常重放（[#939]）。
- **AI 导入**：导入知识同步出资账户教学（[#939]）。
- **AI 导入**：导入知识新增基金转换教学，`convert` 可用（[#981]）。
- **AI 导入**：导入知识新增份额调整教学，`split` 可用（[#1054]）。
- **发布**：Android arm64 APK 进入发布矩阵，随 GitHub Release 发布（[#559]）。
- **发布**：Android APK 发布签名就绪——CI 注入 keystore 签名，tag 构建缺签名 secrets 直接失败（[#560]）。
- **发布**：README「安装」节同步发布签名口径——Android 已签名，桌面三平台维持未签名 / 未公证（[#561]）。
- **报表**：报表页接入 ESC 复位（[#894]）。
- **投资**：持仓页与盈亏页标的代码可点击下钻，跳交易页按该标的过滤（含已清仓标的，是清仓标的卖出流水的唯一入口）（ADR-0107）。
- **交易**：交易列表新增标的过滤维度，支持 URL 参数下钻，基金转换任一腿命中即算（ADR-0107）。

### BREAKING

- **投资 / 交易**：删除语义修订——删除卖出会回补持仓扣减并清空卖出匹配；删除买入改为级联软删其持仓批次的在用卖出并回补持仓，「已有部分卖出的买入禁删」守卫与 `trade.partially-sold-delete` 错误码退役（[#940]，ADR-0097）。
- **数据库 schema**：基金转换（convert）所需的 kind/action 检查约束扩集、4 个转换列与转出消耗表 `security_lot_conversions` 经就地修改 V001/V002 落地。仅全新安装获得新 schema；**存量库不自动升级，需重建库**（[#977]、ADR-0099）。
- **交易**：交易列表类型筛选的单值查询参数移除——统一走集合参数 `kinds`（逗号分隔，单值亦经它）；旧单值参数被忽略、结果变宽、不报错（[#1025]）。

### Changed

- **性能**：transactions 索引清理——删除 6 个零消费者 / 语义重叠索引；账户币种锁守卫改写为双 EXISTS 查询（608MB 基准库实测最坏 ~0.8s → 微秒级）；新增保单统计部分覆盖索引（[#1300]）。
- **AI 导入**：导入知识改为分级自足——基础知识端点返回索引 + 非投资节，投资五节改从新端点按需获取（[#1123]，ADR-0110）。
- **AI 导入**：AI 菜单入口提示词对齐双形态（AI 记账 / 数据迁移），「迁移为主、直录为辅」的说法退役（[#1122]，ADR-0110）。
- **AI 导入**：导入去重内容哈希纳入出资账户；历史行（无出资账户）去重行为不变（[#939]，ADR-0096）。
- **投资**：持仓与盈亏表格数值列右对齐、等宽数字，盈亏数字改为「红涨绿跌」（[#920]）。
- **桌面**：窗口最小宽 900 → 360，窄窗口下自动进入移动档布局（[#842]，ADR-0088）。
- **桌面**：交易列表每行新增常显「⋯」操作按钮（点开与右键同一菜单）（[#843]，ADR-0088）。
- **依赖**：前端引入 oxfmt 格式化器并入质量门；vitest 4→5、oxlint 与 Rust 传递依赖刷新（开发工具变更，无使用者可见变化）（[#1519]、[#953]）。
- **依赖**：同步信封加密基座由 ring 换为 aws-lc-rs——信封格式与派生参数不变（[#1215]）。
- **交易**：交易页类型筛选改为多选——空选即全部类型，原单选形态退役（[#1025]）。
- **投资**：盈亏页收敛——移除「卖出明细」卡，总盈亏卡单统计布局同步收敛（ADR-0107）。
- **投资**：标的类型无效的报错补充合法值清单（stock/fund/bond/etf/other）（[#1022]）。
- **投资**：盈亏页「按年 / 按账户」两表新增「已实现收益」列（= 已实现盈亏 + 现金分红），单元格副行拆出两腿；只有分红、没有卖出的年份 / 账户不再整行缺席（[#1533]，ADR-0129）。

### Removed

- **投资**：盈亏页「卖出明细」卡退役——逐匹配的卖出批次成本 / 盈亏不再逐笔展示（ADR-0107）。

### Fixed

- **备份**：修复自动备份连续失败静默——连续失败达阈值（3 次，约 30 分钟）后设置页「自动备份」卡片提示失败次数与排查指引，一次成功备份后提示消失；单次失败不上浮界面（[#1456]）。
- **实物资产**：修复同日连续新增估值可能读回陈旧值——`new_uuid` 改为进程内严格单调的 UUID v7，所有按主键时间破平的读路径不再因同毫秒随机低位而回退（[#1489]）。
- **投资**：修复标的远程搜索与标的列表的在途请求乱序覆盖——新查询发起即作废在途，迟到旧结果不落位（投资录入表单、盈亏页标的筛选、标的页列表三处）（[#1401]）。
- **实物资产**：修复切换「在持 / 已处置」筛选后短暂显示另一筛选的资产——筛选切换即作废在途重拉并按新筛选立即重拉（[#1381]，ADR-0123 修订注）。
- **投资**：修复货币基金市值少计约七成——货基现价曾被按「万份收益」收录，改为按响应自报口径识别、现价恒 1.0000；已写错的现价自下次同步起自愈（[#1342]，ADR-0038 修订）。
- **分类**：修复编辑分类时清空父分类或图标静默不生效——更新入参支持「给 `null` 即清空」（[#1327] 范围外修复）。
- **投资**：修复已终止（清盘）基金被误报「查无基金代码」——基金查询与标的创建改按「搜索索引 ∪ 档案通道」判定存在；同批修复这类基金中断整次名称刷新（[#1212]，ADR-0039 修订）。
- **投资**：修复盈亏汇总多币种混算——总盈亏与三张汇总改为按币种分组展示（ADR-0107）。
- **投资**：修复删除卖出后对应买入被幽灵占用永久锁死、无法删除或修改（[#940]）。
- **投资**：修复投资走势对场外基金与手动报价标的不生效——单标的走势放行改由后端价格通道判定驱动（[#1060]）。
- **投资**：修复添加 / 导入的场外基金「同步标的信息」不回填历史净值——首刷判据改看有无历史序列，首次同步即补齐近两年周线（[#1059]）。
- **投资**：「同步标的信息」首刷 / 深回填单只基金期间新增页级进度明细（如「回填 110022：第 3/25 页」）（[#1061]）。
- **发布**：修复 Windows 发布 pnpm install 因补丁文件 CRLF 行尾失败（[#917]）。

## [0.6.0] - 2026-09-08

### Added

- **保险**：保单与商户分家——保险公司改从保司字典选择，保费流水归保单引用，商户排行回归纯消费口径（[#713]）。
- **保险**：新增保司字典（全新库内置 30 家常用国内保司）与管理页（[#712]、[#714]）。
- **设置**：路径与备份文件新增「复制路径」「在访达中显示」，备份列表新增「刷新」（[#653]、[#651]）。
- **备份**：备份与恢复跟随加密语义——列表新增「加密」列，密文备份恢复需输主口令（[#572]）。
- **加密**：新增整库加密闭环——开启加密与主口令、关闭加密、修改主口令、忘记口令重置、本机记住主口令（Touch ID 自动解锁）（[#570]、[#571]、[#573]、[#574]）。
- **加密**：开发/未签名构建的自动解锁免 Touch ID 门（[#662]）。
- **界面**：新增金额隐私模式，一键掩码全应用金额（[#566]）。
- **报表**：商户消费排行每行新增交易笔数（[#617]）。
- **设置**：About 页新增日志等级配置（[#611]）。
- **备份**：启动失败改由恢复屏接管，可重置空库或从备份恢复（[#601]、[#602]、[#603]）。
- **投资**：打通美股记账——新增股票实时查询端点 `GET /api/v1/stocks/{code}`，查询、建标的、刷价与净资产口径覆盖美股（[#693]、[#696]）。
- **投资**：标的创建对 stock 类型开启东财增强，校验代码并回填名称与现价（[#694]）。
- **AI 导入**：新增紧凑契约方言端点 `GET /api/v1/contract`，契约体积 ~46KB → ~17KB（[#839]）。
- **投资**：新增「添加投资标的」对话框，成为标的创建唯一入口（[#697]、[#826]）。

### BREAKING

- **数据库 schema**：保单表保司字段由商户引用换为保司引用。仅全新安装获得新形状；**存量库不自动升级，需重建库**（[#713]）。
- **数据库 schema**：V001 新增导入去重兜底索引，仅全新安装生效（[#701]）。
- **数据库 schema**：标的 market 检查约束扩展至美股三市场。仅全新安装生效；**存量库不提供自动修复**（[#692]）。

### Changed

- **投资**：「同步持仓价格」升级为「同步标的信息」，覆盖库内全部标的并刷新权威名称（[#827]）。
- **加密/备份**：转换、恢复、搬迁后原位重引导；自动解锁等待有界，受阻超时回退手输（[#644]）。
- **应用更名**：应用更名为「开源记账」（OpenLedger），数据与升级路径不变（[#584]）。
- **交易**：转账/买入/卖出携带分类改为报错（[#582]）。
- **报表**：商户消费排行改为表格，可点击下钻（[#618]）。
- **设置**：备份页签卡片重排，一键备份直达（[#651]）。
- **加密**：危险确认统一为应用内弹窗；主口令最短 8 位（[#650]）。

### Removed

- **投资**：标的全量同步退役，权威名称改由按代码查询/创建带回（[#698]）。

### Fixed

- **投资**：场内 ETF 持仓纳入行情通道，照常刷现价（[#695]）。
- **界面**：修复九处弹窗表单行距贴死（[#804]）。
- **界面**：修复「移回侧栏」菜单提示文案溢出（[#647]）。
- **加密**：开发/未签名构建的自动解锁文案按运行形态区分（[#687]）。

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

[#1548]: https://github.com/asiazhang/ledger/issues/1548
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
[#559]: https://github.com/asiazhang/ledger/issues/559
[#560]: https://github.com/asiazhang/ledger/issues/560
[#561]: https://github.com/asiazhang/ledger/issues/561
[#566]: https://github.com/asiazhang/ledger/issues/566
[#582]: https://github.com/asiazhang/ledger/issues/582
[#584]: https://github.com/asiazhang/ledger/issues/584
[#570]: https://github.com/asiazhang/ledger/issues/570
[#571]: https://github.com/asiazhang/ledger/issues/571
[#572]: https://github.com/asiazhang/ledger/issues/572
[#573]: https://github.com/asiazhang/ledger/issues/573
[#574]: https://github.com/asiazhang/ledger/issues/574
[#601]: https://github.com/asiazhang/ledger/issues/601
[#602]: https://github.com/asiazhang/ledger/issues/602
[#603]: https://github.com/asiazhang/ledger/issues/603
[#611]: https://github.com/asiazhang/ledger/issues/611
[#617]: https://github.com/asiazhang/ledger/issues/617
[#618]: https://github.com/asiazhang/ledger/issues/618
[#650]: https://github.com/asiazhang/ledger/issues/650
[#647]: https://github.com/asiazhang/ledger/issues/647
[#662]: https://github.com/asiazhang/ledger/issues/662
[#687]: https://github.com/asiazhang/ledger/issues/687
[#651]: https://github.com/asiazhang/ledger/issues/651
[#653]: https://github.com/asiazhang/ledger/issues/653
[#692]: https://github.com/asiazhang/ledger/issues/692
[#693]: https://github.com/asiazhang/ledger/issues/693
[#694]: https://github.com/asiazhang/ledger/issues/694
[#695]: https://github.com/asiazhang/ledger/issues/695
[#696]: https://github.com/asiazhang/ledger/issues/696
[#697]: https://github.com/asiazhang/ledger/issues/697
[#698]: https://github.com/asiazhang/ledger/issues/698
[#826]: https://github.com/asiazhang/ledger/issues/826
[#644]: https://github.com/asiazhang/ledger/issues/644
[#701]: https://github.com/asiazhang/ledger/issues/701
[#712]: https://github.com/asiazhang/ledger/issues/712
[#713]: https://github.com/asiazhang/ledger/issues/713
[#714]: https://github.com/asiazhang/ledger/issues/714
[#804]: https://github.com/asiazhang/ledger/issues/804
[#827]: https://github.com/asiazhang/ledger/issues/827
[#839]: https://github.com/asiazhang/ledger/issues/839
[#842]: https://github.com/asiazhang/ledger/issues/842
[#843]: https://github.com/asiazhang/ledger/issues/843
[#846]: https://github.com/asiazhang/ledger/issues/846
[#845]: https://github.com/asiazhang/ledger/issues/845
[#847]: https://github.com/asiazhang/ledger/issues/847
[#848]: https://github.com/asiazhang/ledger/issues/848
[#855]: https://github.com/asiazhang/ledger/issues/855
[#856]: https://github.com/asiazhang/ledger/issues/856
[#857]: https://github.com/asiazhang/ledger/issues/857
[#859]: https://github.com/asiazhang/ledger/issues/859
[#858]: https://github.com/asiazhang/ledger/issues/858
[#860]: https://github.com/asiazhang/ledger/issues/860
[#861]: https://github.com/asiazhang/ledger/issues/861
[#862]: https://github.com/asiazhang/ledger/issues/862
[#863]: https://github.com/asiazhang/ledger/issues/863
[#864]: https://github.com/asiazhang/ledger/issues/864
[#834]: https://github.com/asiazhang/ledger/issues/834
[#836]: https://github.com/asiazhang/ledger/issues/836
[#831]: https://github.com/asiazhang/ledger/issues/831
[#884]: https://github.com/asiazhang/ledger/issues/884
[#893]: https://github.com/asiazhang/ledger/issues/893
[#894]: https://github.com/asiazhang/ledger/issues/894
[#849]: https://github.com/asiazhang/ledger/issues/849
[#1049]: https://github.com/asiazhang/ledger/issues/1049
[#1051]: https://github.com/asiazhang/ledger/issues/1051
[#897]: https://github.com/asiazhang/ledger/issues/897
[#917]: https://github.com/asiazhang/ledger/issues/917
[#920]: https://github.com/asiazhang/ledger/issues/920
[#928]: https://github.com/asiazhang/ledger/issues/928
[#935]: https://github.com/asiazhang/ledger/issues/935
[#937]: https://github.com/asiazhang/ledger/issues/937
[#939]: https://github.com/asiazhang/ledger/issues/939
[#940]: https://github.com/asiazhang/ledger/issues/940
[#977]: https://github.com/asiazhang/ledger/issues/977
[#981]: https://github.com/asiazhang/ledger/issues/981
[#1025]: https://github.com/asiazhang/ledger/issues/1025
[#1022]: https://github.com/asiazhang/ledger/issues/1022
[#1059]: https://github.com/asiazhang/ledger/issues/1059
[#1060]: https://github.com/asiazhang/ledger/issues/1060
[#1061]: https://github.com/asiazhang/ledger/issues/1061
[#1054]: https://github.com/asiazhang/ledger/issues/1054
[#1078]: https://github.com/asiazhang/ledger/issues/1078
[#1122]: https://github.com/asiazhang/ledger/issues/1122
[#1123]: https://github.com/asiazhang/ledger/issues/1123
[#1300]: https://github.com/asiazhang/ledger/issues/1300
[#1124]: https://github.com/asiazhang/ledger/issues/1124
[#1215]: https://github.com/asiazhang/ledger/issues/1215
[#1216]: https://github.com/asiazhang/ledger/issues/1216
[#1218]: https://github.com/asiazhang/ledger/issues/1218
[#1219]: https://github.com/asiazhang/ledger/issues/1219
[#1220]: https://github.com/asiazhang/ledger/issues/1220
[#1196]: https://github.com/asiazhang/ledger/issues/1196
[#1195]: https://github.com/asiazhang/ledger/issues/1195
[#1327]: https://github.com/asiazhang/ledger/issues/1327
[#1342]: https://github.com/asiazhang/ledger/issues/1342
[#1343]: https://github.com/asiazhang/ledger/issues/1343
[#1345]: https://github.com/asiazhang/ledger/issues/1345
[#1346]: https://github.com/asiazhang/ledger/issues/1346
[#1381]: https://github.com/asiazhang/ledger/issues/1381
[#1450]: https://github.com/asiazhang/ledger/issues/1450
[#1451]: https://github.com/asiazhang/ledger/issues/1451
[#1456]: https://github.com/asiazhang/ledger/issues/1456
[#1519]: https://github.com/asiazhang/ledger/issues/1519
[#1631]: https://github.com/asiazhang/ledger/issues/1631
[#1655]: https://github.com/asiazhang/ledger/issues/1655
