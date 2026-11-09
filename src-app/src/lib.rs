// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块与外挂 tests.rs）经 crate 根 cfg(test) 整体
// 放行，生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

//! NSH 原生壳（issue #2017 骨架 / issue #2018 交易表格首屏，ADR-0145 拆票①，spec #2015；
//! #2083 切轨后为唯一产品壳与 workspace 根包）。
//!
//! 以 NSH（Native Shell）Rust 模式承载的壳——启动参数给账本目录，引导域栈
//! 打开**未加密**账本库（无解锁向导），就绪窗口以 DataTable 渲染真实交易数据
//!（100 行/页服务端 offset 分页走域读路径、分页条点击翻页、搜索框接入服务端
//! 统一模糊搜索、忙碌指示与空态）。读路径直调域服务（`ledger-transaction`
//! 读路径），不经任何 IPC 命令边界——ADR-0145
//! 决策 4 的接缝形态立在此处；Tauri 旧壳零改动、保持唯一发版产品。
//!
//! 依赖方向沿用壳 → 域 → 基础设施 → 协议（ADR-0112）：本 crate 是壳层 bin
//! crate（结构守门政策核 CRATES 登记在册），只消费域 crate 与基础设施，不定义
//! 账本语义；金额与折算口径以核心交易域为唯一真源，本壳只消费不另立。
//!
//! 模块地图：
//! - [`insurers`]：保司管理视图（#2051）——保司字典清单双分区（在用 / 已删
//!   「显示已删」只读切换）、新增卡（名称）、统一模糊搜索语义本地过滤、单字
//!   段编辑弹窗与软删轻量确认气泡；写路径直调域公开写入口，字典完整性守卫
//!   住域单正文；
//! - [`items`]：物品视图（#2044）——清单平表（列格装配产物渲染、零新增列）、
//!   编辑（换关带出覆盖）/ 详情（自选参考日重算、本位币折算）/ 处置 / 软删
//!   轻量确认、创建唯一入口提示与交易行菜单「加入物品」确认弹窗（ADR-0025）、
//!   来源跳转行高亮；写路径直调域公开写入口 + 交易失效信号广播；
//! - [`logging`]：域栈日志面（#2053；文件面随 #2077 平价）——stderr + 日滚
//!   文件双输出（数据根下 logs 目录，按天滚动 7 天保留，WorkerGuard 进程级
//!   持有）+ 档位闭集的运行期滤镜接管（值域闭集单点在基建 settings 接缝）；
//! - [`redact`]：载荷脱敏（#2077，根包 `shell_support::redact` 的等价落点）
//!   ——敏感键闭集递归遮蔽；本壳现无 IPC 序列化边界，生产消费面暂缺，任
//!   何会记录调用载荷/敏感字段的日志面必须经本模块遮蔽后落盘；
//! - [`settings`]：设置视图（#2053）——页签容器（通用/分类/币种/数据/定时/
//!   功能/关于，visited-once keep-mounted 状态保留）、外观卡（显示主题三态）、
//!   日志档位卡、数据存储位置卡（查看 / 更改 / 恢复默认 + 二选一接管守卫）、
//!   功能开关管理面（写路径归 [`nav::set_feature_closed`] 单点共源）；
//!   备份 / 加密 / 同步 / 账本切换卡已随设置②（#2054）落位（[`settings_data`]）；
//! - [`app`]：NSH 根组件与启动结果视图模型（装配入口可注入，headless 可测）；
//! - [`nav`]：进程内导航 API 与导航状态（#2032 壳导航骨架）——单点入口
//!   `open`（守卫裁决 → 落位 → 持久化）、下钻意图读一次消费、键位带派发、
//!   侧栏序/收纳清单/关闭集合/上次视图的存储读写（`nsh_core::storage::Storage`
//!   平台文件侧 KV，键值形状镜像旧壳 `view_state:*` 族、不与旧壳共享、不进
//!   SQLite）；
//! - [`nav_model`]：导航纯模型（#2032）——视图/分组闭集与词表、侧栏出厂定义、
//!   持久化载荷解析防御（旧壳 parse 家族重生）、键位带推导（ADR-0065 固定组
//!   带）、守卫落位裁决（旧壳路由守卫族重生）、跳转目标计算（旧壳 source-jump
//!   重生）与移动纯函数；
//! - [`sidebar`]：侧栏（#2032）——品牌行、固定项、三组导航、组「更多」链接、
//!   键位提示与右键排序菜单；
//! - [`dashboard`]：概览视图（#2039）——净资产总览 / 本月收支 / 投资概览 /
//!   财务自由度 / 预算年度摘要五卡（取数口径单点在域，装配层只消费不另立）、
//!   口径说明弹层、净资产卡下钻与按卡片消费面分的失效信号重读；本票落地即
//!   功能冻结点达成（ADR-0145 修订注记）；
//! - [`investments`]：投资视图·概览与盈亏两页签（#2045，spec #2031 系列
//!   14/26）——visited-once keep-mounted 页签容器（旧壳 display-directive=
//!   "show" + 页签 key 纪律的本壳对应物）、会话槽、概览页签、盈亏页签与
//!   #2046 ⑮ 扩入的持仓页签容器位；读路径直调域读入口；
//! - [`investments_holdings`]：投资持仓页签本体（#2046，⑮/投资 2/3）——
//!   持仓卡（首个持仓行 + 搜索/账户/排序三维过滤 + 合计三卡（过滤子集）+
//!   缺价行同步引导）、客户端切片分页与旧壳 useHoldingsFilter/usePortfolioOverview
//!   的重生；同步入口、进度与结果消费 [`investment_sync`] 接缝；
//! - [`investments_form`]：投资记账表单买卖族（#2046 ⑮）——旧壳
//!   InvestmentForm 族的重生：buy/sell 创建与编辑同表单语义（场外基金
//!   金额权威/其余数量+单价权威分流、字段红框+提交禁用两件同发、先取
//!   明细再开窗）；创建入口随⑯ 明细页签落位、编辑入口随交易列表右键接
//!   线开放；
//! - [`investments_ledger`]：投资明细页签本体（#2047 ⑯/投资 3/3）——五种
//!   投资 kind 投影行（kind 形态格装配，渲染期零格式化）、服务端 offset 分页、
//!   类型子集多选/涉及账户单选/标的深链维（旧壳 ?instrument= 同语义）与清除
//!   筛选，行右键编辑（买卖表单族复用）/只读详情/删除与「记买入/记卖出」
//!   创建入口落位；
//! - [`investments_instruments`]：投资标的库页签本体（#2047 ⑯/投资 3/3）——
//!   服务端检索浏览（统一模糊语义 + 市场折叠 + 只看持仓）、anchor 分页定位
//!   （focus 下钻落点 + 行高亮）、行内录价（手动报价弹窗 + 两落点写路径 +
//!   证据门广播）、添加投资标的六通道弹窗与自建标的删除；
//! - [`investment_sync`]：投资同步接缝与价格过期提示（#2046 ⑮）——「同步
//!   标的信息」接缝单例（进行中/进度/结果/降级四件状态，在途短路、成败
//!   同判失效广播）与 staleness 本地水位提示条旧壳 usePriceStaleness
//!   的重生，「去同步」入口语义偏差见模块头；
//! - [`plot_line`]：line 图封装件（`App*` 封装族，#2045 走势卡首建，ADR-0145
//!   决策 5 的封装层落点——NSH 内建 plot 的直消费单点，时间序列单线 + 缺值
//!   点跨越，本件零统计语义零金额换算）；
//! - [`views`]：壳内视图层（#2032）——未迁移视图空态占位（下钻接缝示范消费
//!   面）与组「更多」容器页签壳；
//! - [`assembly`]：装配层——行集进入表格前产出每行每格最终展示文本与列定义
//!   （纯 Rust、无 UI 依赖；#2019 全家桶：数字分组、金额语义色、隐私遮蔽、
//!   分类路径、备注列、通用 kind 标签；#2020 派生行形态：购买项逐项行与订单级
//!   格首行语义、双账户/出资分解标注、来源名与状态、借贷派生标签、商户名；
//!   #2022 删后页码决策）；
//! - [`bar_chart`]：bar 图自绘周长件（`App*` 封装族，#2048 首建，ADR-0145
//!   决策 5 的封装层落点）——纵向分组柱（月度收支）与横向单柱行（分类构成）
//!   两形态，逐根说明气泡（旧壳 tooltip 的点按形态）自含抑制上报，本件零
//!   统计语义；
//! - [`reports`]：报表视图（#2048）——期间口径三卡（月度收支 / 分类构成两段
//!   下钻 / 商户排行 TopN），会话态进程级单例、「期间切换复位下钻」与 ESC
//!   复位出口内化，跳转下钻消费导航单点接缝不自建；
//! - [`period`]：时间维度行——期间游标/步进/预设/边界钳制的纯决策逻辑
//!   （旧壳 time-period.ts 的重生，#2021 / ADR-0141）；
//! - [`period_row`]：共用期间行（#2040 起自交易视图抽出）——步进器与期间
//!   选择弹层的呈现与交互桥，交易列表与搜索视图共消费；
//! - [`search`]：交易搜索视图（#2040）——关键词防抖自动搜索、期间快捷
//!   筛选、金额区间、只读结果分页与占位/无命中两态空态；延迟出场加载指示
//!   整流（#2010 收尾件语义）住本模块；
//! - [`overlay`]：Overlay Suppression 注册表——弹层开合显式上报、快捷键
//!   派发前的共用闸门（旧壳 ADR-0035 语义的原生壳重建，#2021；#2022 上下文
//!   菜单与危险确认对话框消费）；
//! - [`context_menu`]：上下文菜单封装（`App*` 封装族首批件，#2022）——原生
//!   弹出 + 存续期间抑制上报；
//! - [`danger`]：危险确认对话框封装（`App*` 封装族首批件，#2022）——
//!   ADR-0078 error/warning 两级形态、遮罩不关/ESC 可关、抑制上报自含；
//! - [`pinyin_select`]：拼音可搜下拉封装（`App*` 封装族，#2033 首建）——
//!   触发件 + 搜索弹层 + 统一模糊搜索语义过滤（消费交易域 ADR-0027 唯一
//!   定义点）+ 抑制上报自含，可清空形态随 #2034 消费面开门；
//! - [`select`]：普通下拉选择封装（`App*` 封装族，#2034 首建）——触发件 +
//!   选项列表弹层（无搜索小闭集直选）+ 可清空 + 抑制上报自含；
//! - [`multi_select`]：多选下拉选择封装（`App*` 封装族，#2037 首建）——旧壳
//!   `NSelect multiple` 对应件：勾选切换不关弹层 + 可清空 + 抑制上报自含，
//!   选择态归调用方受控（归一规则归筛选层）；
//! - [`combo_select`]：自由文本可搜输入封装（`App*` 封装族，#2034 首建）——
//!   旧壳 PinyinSelect tag 形态对应件：文本输入 + 建议弹层，选定落 id、
//!   键入退回自由文本（商户「选择或即建」双态值语义）；
//! - [`date_select`]：日期选择器封装（`App*` 封装族，#2034 首建）——可键入
//!   触发件 + 日历弹层（消费 nsh-base DatePicker 行为件，ADR-0145 决策 5 的
//!   封装层落点）+ 抑制上报自含；
//! - [`modal`]：模态弹窗封装（`App*` 封装族，#2033 首建）——遮罩不关/ESC
//!   可关、抑制上报自含的弹层容器；
//! - [`popconfirm`]：轻量确认气泡封装（`App*` 封装族，#2033 首建）——锚定
//!   触发件的确认/取消气泡、ESC/点外关闭、抑制上报自含；
//! - [`popover`]：口径说明气泡封装（`App*` 封装族，#2039 首建）——触发件
//!   点按开合的多行说明气泡（旧壳口径 tooltip 的点按形态）、抑制上报自含；
//! - [`signals`]：进程内失效信号——交易数据失效的订阅/广播单点，写路径
//!   成功后广播、列表视图订阅重读（#2022；词汇表失效信号语义的壳内通道）；
//! - [`txn_form`]：交易表单弹窗（`App*` 封装族消费面，#2034）——基础收支
//!   新增/编辑（日期、类型、金额、分类、账户、备注与来源关联语义），编辑
//!   复用同表单（回填初值）；写路径经域公开写入口直调、失效信号驱动刷新；
//! - [`tx_filter`]：交易筛选会话态（#2037）——筛选维度闭集/类型归一/下钻
//!   全量替换的纯决策半边 + 会话内保留的进程级信号单例（旧壳
//!   `transactions-session` 的重生，零写盘）；
//! - [`tx_view_preferences`]：交易页视图偏好（#2062 / ADR-0136 偏好与筛选
//!   分家）——设备级持久行集裁决（「隐藏投资相关流水」开关）的唯一读写方
//!   （旧壳 `transaction-view-preferences` store 的重生，平台文件侧 KV 同
//!   键同值形态、不与旧壳共享）；
//! - [`backup_entry`]：备份最小入口（#2038）——交易列表工具区一键备份：
//!   文件保存对话框（NSH 壳能力直用）+ 域 backup 直调 + 成功/失败状态条
//!   反馈；默认文件名生成面（旧壳 `defaultBackupFileName` 同格式）；
//! - [`boot`]：域栈引导——库文件处置判定（复用基础设施引导层）、成对建连、
//!   交易列表页与币种字典读取、交易删除（#2022）、手动备份与账本指纹
//!   （#2038）；
//! - [`scheduled`]：定时视图壳（#2042 / #2043）——订阅 / 分期 / 定时转账三
//!   形态页签容器（会话槽保留语义）、计划来源落点意图消费与订阅计划清单装
//!   配（状态 / 期次状态 / 周期标签词表、行操作矩阵、行格文本）；
//! - [`scheduled_subscription`]：订阅页签本体（#2042）——订阅清单、新建 / 编辑
//!   计划表单、行操作（暂停 / 恢复 / 取消带确认气泡）与花费面板 / 详情弹层的
//!   薄适配；商户「输入即建」走按名 find-or-create 接缝；#2043 定时②起兼兄
//!   弟页签共享表单元语言（账户/商户仅选项面、币种随账户推导、表单行形态）
//!   的落点；
//! - [`scheduled_installment`]：分期页签本体（#2043）——分期清单与进度呈现
//!   （已还汇总 + 期数百分比条，渲染期零格式化）、新建分期（总金额/期数/每
//!   期金额预览，floor 均分口径单点）、行操作与详情弹层薄适配；
//! - [`scheduled_transfer`]：定时转账页签本体（#2043）——转账清单（下期转账
//!   / × N期尾饰）、新建转账（转入候选同币种过滤、总期数三态）、行操作与
//!   详情弹层薄适配；
//! - [`scheduled_plan_detail`]：期次详情弹层（#2042）——三页签通用的期次历
//!   史呈现、failed 期次重试与「展开更多期次」；
//! - [`scheduled_spend`]：订阅花费面板（#2042）——双口径呈现（实际花费 + 推
//!   算成本）与 12 月自绘趋势柱，花费口径归域、装配层只消费；
//! - [`accounts`]：账户视图（#2041）——余额清单三分区（攒钱/主表/已归档）、
//!   新建表单卡、编辑与余额调整弹窗、行操作（右键 / 「⋯」同位）与账户名下钻；
//!   余额读 V017 持久化缓存（ADR-0067）只消费不另立算术；
//! - [`merchants`]：商户管理视图（#2052）——字典 + 新增表单卡 + 清单卡（统一
//!   模糊搜索 / 显示已删 / 条数排序 / 条数下钻 / 客户端分页）+ 编辑弹窗与软删
//!   轻量确认；写路径直调域参考数据接缝，成功后广播交易失效信号（交易列表
//!   来源列显示商户名的壳内对应面）；
//! - [`savings`]：储蓄目标视图（#2056）——目标清单与已归档双分区卡、蓄水
//!   进度呈现（已存/还差/达成态与双向推算，进度算术归域读数）、新建/编辑
//!   共表单弹层、生命周期动作（归档/取消归档/删除——删除经 danger 二确认，
//!   守卫语义住域写入口单正文）与达成态计划提示；
//! - [`cross_book`]：跨账本投资汇总（#2055）——旧壳壳层编排（根包
//!   cross_book_summary，ADR-0114）的重生：逐本只读探测分派（四态闭集）、
//!   内存合并与当期汇率折算（域读函数/折算入口单点消费）+ 只读合计视图
//!   （四口径卡带概念说明、口径标注、逐本计入状态；随投资开关联动关闭）；
//! - [`ai_prompt`]：AI 提示词页（#2055）——AI 导入入口提示词全文展示与
//!   复制（提示词模板与旧壳同一契约文件的编译期嵌入，两壳同源零副本）；
//! - [`policies`]：保单视图（#2050）——保单清单（保司显示映射含软删、到期
//!   态、保额纯展示与保单视角统计同源快照）、新建/编辑共表单弹层（保司 tag
//!   即建走按名 find-or-create 接缝、保额币种成对清联）、缴费协议（新建可
//!   折叠可选、编辑模式分段史与改价——订阅既有语义直调计划域接缝）、软删
//!   轻量确认与来源跳转行高亮；交易失效订阅/广播（旧壳 ledger:changed 族
//!   语义的壳内对应物）；
//! - [`market_lanes`]：行情车道宿主面（#2079）——三条行情后台车道（价格历史
//!   补全 / 每日现价刷新 / 每日汇率增量同步）的启动接线（`run_startup` 成功
//!   分支单点，旧壳 `start_background_services` 挂载闭集平价）：域公开宿主面
//!   注册点（`LaneHost`）装配 + 跨线程反馈桥（进度进接缝信号、实际写入广播
//!   投资失效）；
//! - [`wiring`]：域接缝接线——读路径全部读时投影接缝 + 交易写入协议的余额刷新
//!   接缝，与旧壳启动接线同形（ADR-0112 决策 5）。
//!
//! 测试权威（ADR-0087 断言归属在本壳的落点）：装配层格文本/列定义/分页边界归
//! [`assembly`] 单测；壳行为（引导与读接线）归本 crate 的 lib 单测；视图树接线
//! （窗口呈现）归 headless 组件测试（`tests/app_headless.rs`,
//! docs/research/nsh-testing-capabilities.md §3.1）；域语义归各域单测，不在本壳
//! 重复。
//!
//! 域 crate 测试的实例纪律（自旧壳根包 lib.rs 延续，#2083 切轨后根包仍是
//! 域 crate 测试图外的第三方）：被测域 crate 在自身测试图内双实例——本地
//! 实例（cfg(test) 构建）与根包图实例（根包依赖携带）——接缝注册静态由测试
//! 工厂（test_support::open）接在根包图实例上，走接缝的行为路径测试必须经
//! `nsh_app::ledger_*` 驱动同一实例；生产路径不经本面（壳层直呼 crate 名）。
//! 本处仅保留 crate 本名再导出，服务于上述纪律。
//!
//! 包内第二 bin（#2082）：`src/bin/ledger-perf`（性能基准工具，ADR-0062 唯一
//! 生成器，spec #458）随本包构建——headless 集成测试经 `CARGO_BIN_EXE_ledger-perf`
//! 消费，工具单测归 perf-bench.yml 每日基准承接（test-exec.ts EXEMPT_BINS 登记）。

pub mod accounts;
pub mod adaptive;
pub mod ai_prompt;
// AI 导入 HTTP 壳（axum api_server，#2081 随宿主迁居）：路由 / 契约 / handler
// 与其集成测试套件的本壳正住址，tauri 依赖经 EmitterSlot 抽象反转（ADR-0112
// 决策 5），生产发射器由 http_host 接线；契约（路由、载荷、错误码）零演进。
pub mod api_server;
pub mod app;
pub mod backup_entry;
pub mod backup_host;
pub mod bar_chart;
pub mod boot;
pub mod combo_select;
pub mod context_menu;
pub mod cross_book;
pub mod danger;
pub mod dashboard;
pub mod date_select;
// AI 导入 HTTP 壳的生产挂载与失效信号泵（#2081）：api_server 迁居后的启动
// 接线面——旧壳 lib.rs setup 内挂载的本壳对应物（ADR-0112 决策 5 壳层接线）。
pub mod http_host;
pub mod insurers;
pub mod investment_sync;
pub mod investments;
pub mod investments_form;
pub mod investments_holdings;
pub mod investments_instruments;
pub mod investments_ledger;
pub mod items;
pub mod logging;
pub mod market_lanes;
pub mod merchants;
pub mod modal;
pub mod multi_select;
pub mod nav;
pub mod nav_model;
pub mod overlay;
pub mod period;
pub mod period_row;
pub mod physical_assets;
pub mod pinyin_select;
pub mod plot_line;
pub mod policies;
pub mod popconfirm;
pub mod popover;
pub mod redact;
pub mod reports;
pub mod savings;
pub mod scheduled;
pub mod scheduled_installment;
pub mod scheduled_plan_detail;
pub mod scheduled_spend;
pub mod scheduled_subscription;
pub mod scheduled_transfer;
pub mod search;
pub mod select;
pub mod settings;
pub mod settings_data;
pub mod shell_support;
pub mod sidebar;
pub mod signals;
pub mod sync_triggers;
pub mod tx_cards;
pub mod tx_filter;
pub mod tx_view_preferences;
pub mod txn_form;
pub mod views;
pub mod wiring;

mod assembly;

// 域 crate 全家实例纪律的再导出面（ADR-0084 测试工厂接线，见 crate 头注释）：
// 仅 crate 本名，生产路径不经本面。
pub use ledger_accounts;
pub use ledger_backup;
pub use ledger_budget;
pub use ledger_categories;
pub use ledger_currencies;
pub use ledger_dashboard;
pub use ledger_investment;
pub use ledger_item;
pub use ledger_market_sync;
pub use ledger_merchants;
pub use ledger_physical_asset;
pub use ledger_policy;
pub use ledger_reports;
pub use ledger_scheduled;
pub use ledger_sync_engine;
pub use ledger_transaction;

// NSH 测试器具（#2081 随 api_server 迁居；ADR-0111 决策 5 / issue #1132 门，
// 根包同款）：集成测试链接非 cfg(test) 构建的 lib，故以 cfg(any(test,
// feature = "test-utils")) 声明，默认不进生产编译；#[doc(hidden)] 不进文档。
// #2077/#2080 的词法器具与 S3 桩 facade（s3_stub）随迁并入本模块
// （crate::test_support::{ScratchDir, s3, scan, scratch} 引用面不变）。
#[cfg(any(test, feature = "test-utils"))]
#[doc(hidden)]
pub mod test_support;

#[cfg(test)]
mod tests;
