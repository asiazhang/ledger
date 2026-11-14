//! NSH 根组件与启动结果视图模型（issue #2017 骨架；#2073 起视图本体归位——交易表格住 [`crate::transactions`]、预算视图住 [`crate::budget`]，本模块只持根组件、启动竖井与启动形态呈现（含密文库锁定态，#2088））。
//!
//! 装配入口可注入（docs/research/nsh-testing-capabilities.md §3.1：lib + bin
//! 双靶，bin 薄启动）：bin（`main.rs`）解析启动参数后经 [`set_book_dir`] 注入
//! 账本目录；headless 组件测试以同一入口注入测试库路径后起真实视图树
//! （`Root::of::<App>`），生产与测试同一条装配路径。
//!
//! 引导在组件任务的后台线程执行（`spawn` + `blocking`：SQLite 是阻塞 IO，不占
//! 引擎线程）；就绪态经 [`view_host`] 分派各视图模块（交易表格
//! [`crate::transactions`] 等 ~30 视图各持独立模块）。会话句柄（`DbState`）住
//! 进程级单例 [`DB`]，页读复用成对建连的只读侧。
//!
//! 显示主题跟随存储外观设置（registry 缺省带主题双态令牌，ADR-0145 立桩验收
//! 项；#2053 起启动经 [`crate::settings::apply_startup_appearance`] 按存储
//! 外观应用显色——出厂跟随系统，深浅色强指；文字色取令牌（裸 text 的默认前
//! 景是黑——暗色桌面不可见，先例 #2017 修复），表格部件经 Tailwind 类取主
//! 题令牌。文案仅 zh-CN（spec #2015 实施决策）。

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use ledger_infra::db::DbState;
use ledger_infra::error::AppError;
use nsh_base::button;
use nsh_core::element::FontWeight;
use nsh_core::file_dialog::OpenDialog;
use nsh_core::prelude::*;

use crate::boot;
use crate::nav_model::ViewName;
use crate::{
    accounts, backup_host, budget, danger, dashboard, investments, items, logging, nav, nav_model,
    reports, scheduled, search, settings, sidebar, signals, sync_triggers, transactions, tx_filter,
    tx_view_preferences, unlock, views,
};

/// 启动装配入口：账本目录（bin 注入一次；headless 测试按场景注入）。槽位
/// 形态的进程级单例（headless 多场景串行测试需可复位，见
/// [`reset_book_assembly`]）。
static BOOK_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// 就绪后的账本库会话（成对建连，读页复用只读侧；进程级单例与 [`BOOK_DIR`]
/// 同形——boot 成功先于 `StartupView::Ready` 呈现，视图读取时恒已就位）。
static DB: Mutex<Option<std::sync::Arc<DbState>>> = Mutex::new(None);

/// 装配槽位守卫（毒化时取回内值：槽位本身不含不变量，锁毒化不影响正确性）。
fn book_dir_slot() -> MutexGuard<'static, Option<PathBuf>> {
    BOOK_DIR.lock().unwrap_or_else(|p| p.into_inner())
}

/// 会话句柄槽位守卫（crate 内消费：交易表单写任务与 lib 测试经它取句柄）。
pub(crate) fn db_slot() -> MutexGuard<'static, Option<std::sync::Arc<DbState>>> {
    DB.lock().unwrap_or_else(|p| p.into_inner())
}

/// 会话句柄只读出口（crate 内消费：lib 测试的接线断言经它取句柄直读首页）。
#[cfg(test)]
pub(crate) fn session_handle() -> Option<std::sync::Arc<DbState>> {
    db_slot().clone()
}

/// 注入账本目录（幂等：先注入者优先，与接缝注册点同形）。bin 启动时调用一次；
/// 未注入时 [`App`] 呈现缺参失败态（不静默退窗，窗口即错误呈现面）。
pub fn set_book_dir(dir: PathBuf) {
    let mut slot = book_dir_slot();
    if slot.is_none() {
        *slot = Some(dir);
    }
}

/// 启动账本目录会话快照（设置卡数据位置面的默认目录单点——身份即目录，
/// 立桩启动契约的读侧；未注入 = None，卡的读写路径各按会话缺失分支呈现）。
pub(crate) fn book_dir() -> Option<PathBuf> {
    book_dir_slot().clone()
}

/// lib 测试的装配串行锁（跨模块共享）：装配入口、会话句柄、导航状态与失效
/// 信号订阅者都是进程级单例——引擎类 lib 单测（tests / nav::tests /
/// dashboard::tests）跨模块互斥防串扰（#2039 概览探针长用例与既有探针并行
/// 即互踩装配单例，收口单锁）。毒化取回内值（槽位本身不含不变量，测试
/// panic 后续用例照常串行）。headless 集成测试目标有独立 SERIAL，两个测试
/// 二进制互不串扰。
#[doc(hidden)]
#[cfg(test)]
pub(crate) fn test_assembly_guard() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    // 车道测试现场同批复位（#2079）：武装位 / 注入槽 / 反馈对不跨用例残留——
    // 守卫获取即解除武装，业务用例的 run_startup 成功分支零车道零网络；车道
    // 接线用例在守卫内显式重装（enable_for_tests + 注入桩）。
    crate::market_lanes::reset_for_tests();
    guard
}

/// 重置装配入口与会话句柄（headless 测试接缝）：同一进程内的多场景串行测试
/// 先复位再注入各自的测试库路径（生产 bin 只注入一次，不消费本函数；串行
/// 由 [`test_assembly_guard`] 保证，复位时上一场景的引擎已撤销）。失效信号
/// 订阅者与导航状态属同一批复位点（上一场景视图的订阅者不得响应下一场景
/// 的广播）。
#[doc(hidden)]
pub fn reset_book_assembly() {
    *book_dir_slot() = None;
    *db_slot() = None;
    // 引导记录槽同批复位（#2091）：上一场景的回退原因 / 引导目录不得泄入
    // 下一场景——解析链现场 D 的回退告示读本槽，残留即污染后续场景呈现。
    boot::reset_boot_record_for_tests();
    signals::reset_for_tests();
    nav::reset_for_tests();
    tx_filter::reset_for_tests();
    tx_view_preferences::reset_for_tests();
    reports::reset_for_tests();
    scheduled::reset_for_tests();
    investments::reset_for_tests();
    // #2088：加密锁定门槽同批复位（上一场景的锁定态不得泄入下一场景——
    // 门槽复位后下个取用重新 seed 恒开实例；锁定面板信号组随各场景 App
    // 实例存亡，无进程级复位面）。
    boot::reset_encryption_gate_for_tests();
    // #2046：投资同步接缝与 staleness 单例（进程级信号束属同批复位点——
    // 上一场景的接缝状态不得响应下一场景）。
    crate::investment_sync::reset_all_for_tests();
    // #2080：同步触发失效广播的投递通道属同批复位点（上一场景的排空任务随
    // 其引擎撤销，通道退役——下一场景的 App 重装 fresh 通道与排空任务）。
    crate::sync_triggers::reset_for_tests();
    crate::cross_book::reset_default_data_dir();
    // #2057：移动档导航壳态同批复位（抽屉开态不跨场景残留）。
    crate::adaptive::reset_for_tests();
    // #2079：行情车道测试现场同批复位（武装位 / 注入槽 / 反馈对——上一场景
    // 的车道武装与桩束不得响应下一场景）。
    crate::market_lanes::reset_for_tests();
    // #2078：自动备份宿主的进程级单例态同批复位（目录镜像 / 首次兜底认领
    // 与待跑标志——上一场景的备份目录不得泄入下一场景）。
    backup_host::reset_shell_side_for_tests();
}

/// 启动结果闭集（窗口呈现的全部状态）。
#[derive(Clone, Debug, PartialEq)]
pub enum StartupView {
    /// 未指定账本目录且数据根未注入（headless 测试态）：呈现用法。
    MissingArg,
    /// 引导中（域栈在后台线程打开账本库）。
    Loading,
    /// 就绪：交易表格首屏（读路径直调域服务的立桩靶标，issue #2018）。
    Ready,
    /// 首启引导（ADR-0146 决策 2）：注册表未配置（含损坏回退）且解析落点
    ///（默认数据目录）无账本库。`fallback_reason` = 注册表损坏回退原因
    ///（`Some` 时引导界面带损坏告示；`None` = 未配置的干净出厂现场）。
    Onboarding { fallback_reason: Option<String> },
    /// 密文库锁定态（issue #2088）：引导已识别密文库（引导记录落槽、锁定门
    /// 置锁），等待主口令解锁或忘口令重置；成功后经业务可用起点单点进入
    /// [`StartupView::Ready`]（原位换连，不重走启动竖井）。
    Locked,
    /// 引导失败：码化错误的用户可见文案（zh-CN 原样呈现）。
    Failed {
        message: String,
        /// 失败态是否挂「选择账本库目录…」出口（ADR-0146 决策 4：密文库与
        /// 损坏残留失败态缺的是解锁与诊断，不是目录——不挂；其余失败态保持
        /// 出口在场，用户可重试或另指目录）。派生单点见 [`failed_view`]。
        can_pick_book_dir: bool,
    },
}

/// 执行启动竖井（引导域栈 → 会话就位），返回窗口呈现态。
///
/// 阻塞（SQLite IO）；组件内在 `blocking` 任务里调用。就绪时会话句柄进 [`DB`]
/// 单例，列表页读经它复用只读连接（不随启动竖井释放）。
pub fn run_startup() -> StartupView {
    run_startup_with_trigger_timings(ledger_sync_engine::TriggerTimings::default())
}

/// 启动竖井（显式触发时机版，[`run_startup`] 的接缝本体，issue #2080）：引导
/// 与会话装配完全同形，只有同步触发接线的时机参数不同——生产走默认常量，
/// 触发接线测试注入「短去抖 + 超长轮询」（写后触发断言只等去抖窗，不被低频
/// 轮询救活，issue #959 显式参数接缝先例；调度线程的正常输入，不是测试后门）。
#[doc(hidden)]
pub fn run_startup_with_trigger_timings(
    timings: ledger_sync_engine::TriggerTimings,
) -> StartupView {
    // 启动账本目录解析链（ADR-0146 决策 1）：argv 显式 > 账本注册表活动指针
    // > 默认数据目录。argv 臂行为原样（身份即目录）；缺参臂经数据根消费
    // 注册表（发布形态 .desktop 直跑无 argv 的产品主路径）；`Ok(None)` = 未
    // 配置（含损坏回退）且无库——首启引导现场（决策 2，T2 #2092）。数据根
    // 未注入（headless 测试态）保持缺参提示。删缺参臂的解析链接线（恢复缺
    // 参无条件 MissingArg）即 headless 现场 A/B 与引导态用例红（ADR-0087）。
    // 数据根取一次（缺参臂判定与引导臂共用同一份，F5：不重复解析）。
    let argv_arm = book_dir_slot().is_some();
    let data_root = if argv_arm {
        None
    } else {
        crate::cross_book::default_data_dir()
    };
    let booted = match book_dir_slot().clone() {
        Some(book_dir) => boot::boot_ledger(&book_dir).map(Some),
        None => match data_root.clone() {
            Some(data_root) => boot::boot_ledger_via_registry(&data_root),
            None => Ok(None),
        },
    };
    match booted {
        Ok(Some(db)) => enter_business_surface(db, timings),
        // 首启引导现场（ADR-0146 决策 2，T2 #2092）：数据根已注入且注册表
        // 未配置（含损坏回退）且解析落点无库——分流信号在
        // boot_ledger_via_registry 内按注册表状态（RegistryBootState）判定，
        // 已解析活动条目缺库在彼处 fail loud（决策 4），不经本臂。回退原因随
        // 引导记录落槽（boot_ledger_via_registry），呈现面带损坏告示；干净
        // 出厂现场无告示。数据根未注入（headless 测试态）保持缺参用法提示。
        // 删本臂引导映射（回退无条件 MissingArg）→ app_headless 引导态用例
        // 全红（「删除即变红」已实证，ADR-0087）。
        Ok(None) => match data_root {
            Some(_) => StartupView::Onboarding {
                fallback_reason: boot::boot_record().and_then(|record| record.fallback_reason),
            },
            None => StartupView::MissingArg,
        },
        // 密文库等待解锁（issue #2088）：引导层产出稳定的等待解锁码，呈现
        // 层映射为锁定视图（解锁 / 忘口令重置入口），不是失败态。
        Err(e) if e.is_code(boot::CODE_AWAITING_UNLOCK) => StartupView::Locked,
        Err(e) => failed_view(&e),
    }
}

/// 业务可用起点单点（旧壳 `resume_business_surface` 同位，issue #2088）：
/// 启动竖井成功分支与解锁 / 重置成功后的续接共用——日志档位接管 → 会话槽
/// 换入 → 加密锁定门开门（fail-closed 次序：先换入后开门，门在换入前若仍
/// 锁定，调度 / 同步触发的门检自然空转）→ 自动备份调度宿主同点补拉起
///（锁定场景解锁后的唯一拉起时机，backup_host 模块文档预留的编排点）→
/// AI 导入 HTTP 壳挂载 → 同步触发 → 行情车道。调用方（引擎线程）在返回
/// [`StartupView::Ready`] 后接线 HTTP 泵（`http_host::spawn_pump`，泵需引
/// 擎线程）。
pub(crate) fn enter_business_surface(
    db: DbState,
    timings: ledger_sync_engine::TriggerTimings,
) -> StartupView {
    // 日志档位接管（旧壳 boot_sequence 启动接线语义保持：RUST_LOG 显
    // 式优先；接管失败仅告警不阻启动——排障档位不是启动位面，锁异常
    // 按读路径容错同口径）。接管在 logging 单点内化，读路径消费面。
    logging::apply_persisted(&db);
    // 自动备份调度宿主接线（issue #2078）：会话首次兜底补跑 + 调度线程
    // 挂载（旧壳 `start_background_services` 的本壳对应编排点；装载期
    // 的偏好推送与深路径发射器在 [`backup_host::install_shell_side`]）。
    // 密文库锁定场景：调度线程挂载在本步发生（线程每轮门检在锁定期间
    // 空转，见 [`backup_host::business_availability`]），首次兜底在解锁后
    // 的本次调用补跑（issue #2088 同点补拉起）。
    backup_host::start_session_host(&db);
    // AI 导入 HTTP 壳挂载（#2081）：库就绪即挂，旧壳 setup 内
    // start_http_server 挂载语义保持（端口 127.0.0.1:9527、进程级
    // 一次）；生产发射器与门注入在 http_host 内化（ADR-0112 决策 5）。
    // 删除本接线即 headless 挂载断言红（tests/app_headless.rs）。
    crate::http_host::mount(&db);
    *db_slot() = Some(std::sync::Arc::new(db));
    // 加密锁定门开门（fail-closed 次序：会话就位后开门；旧壳
    // resume_business_surface 的「先换连再开门」同序）。明文启动门本就
    // 恒开，翻转幂等。
    boot::set_encryption_locked(false);
    // 多端同步触发接线（issue #2080）：旧壳 setup 的 start_triggers 对应
    // 面——写后即时同步钩子随域栈引导接线（wiring），触发闭集（打开即
    // 同步 + 桌面低频轮询）随本调用拉起。旧壳「锁定/失败不拉起」同口径：
    // 只在引导成功分支（占位连接不是业务库）。唯一接线点由源码扫描守门
    // 钉住（sync_triggers guard_tests），删接线即 headless 旅程与触发
    // 接线测试红（ADR-0087）。
    sync_triggers::wire_sync_triggers_with(timings);
    // 行情车道启动接线（issue #2079）：三条行情后台车道（价格历史补
    // 全 / 每日现价刷新 / 每日汇率增量同步）在业务可用起点一并挂载
    //（旧壳 start_background_services 挂载闭集平价，lib.rs:153-155）。
    // 删除本行即车道接线测试红（tests/market_*_lane.rs + 本模块
    // market_lanes::probe 探针）。
    crate::market_lanes::start_market_lanes();
    StartupView::Ready
}

/// 引导失败的失败态构造单点（错误 → 呈现态，含「选择账本库目录…」出口的
/// 挂载判定，ADR-0146 决策 4 的负向闭集）：损坏残留（含无法自动修复的外来
/// 形态）缺的是诊断，不是目录——不挂；其余失败（book-missing 本尊、文件
/// 对话框、注册表族、IO 与建连失败）保持出口在场，可重试或另指目录。
/// 密文库不经本函数（等待解锁码在 run_startup 映射为锁定视图，#2088）。
fn failed_view(error: &AppError) -> StartupView {
    let diagnosis_only = error.is_code(ledger_infra::db::boot::BOOT_DB_UNREADABLE)
        || error.is_code(ledger_infra::db::boot::BOOT_DB_NORMALIZE_FAILED);
    StartupView::Failed {
        message: error.to_string(),
        can_pick_book_dir: !diagnosis_only,
    }
}

/// 回退告示呈现块（单一出处，两处调用，F8 收口）：标题（设置卡回退行同词
/// [`settings::LOCATION_FALLBACK_TITLE`]）+ 回退原因两行的 destructive 款式
/// 条——就绪臂（boot-fallback-*）与引导面板（onboarding-fallback-*）同款
/// 式，test_id 由消费面传入保持各自现值。呈现条件（has_fallback）与数据源
///（boot_record.fallback_reason）归消费面的 when 分支，本函数只承载款式。
fn fallback_notice_view(
    reason: String,
    title_id: &'static str,
    body_id: &'static str,
    fg: AdaptiveColor,
    muted: AdaptiveColor,
) -> View {
    view()
        .flex_col()
        .app_class("rounded-(--radius) border border-destructive/30 bg-destructive/10 p-2 gap-1")
        .child(
            text(settings::LOCATION_FALLBACK_TITLE)
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg)
                .test_id(title_id),
        )
        .child(
            text(move || reason.clone())
                .text_xs()
                .text_color(muted)
                .test_id(body_id),
        )
}

/// 启动任务异常退出（JoinError）的失败态（内部错误；出口保持在场——用户
/// 仍可另指账本库目录）。
fn startup_task_aborted_view() -> StartupView {
    StartupView::Failed {
        message: "内部错误：启动任务异常退出".to_owned(),
        can_pick_book_dir: true,
    }
}

/// 失败态出口与新建意图确认的词表（ADR-0146 决策 3/4；zh-CN 硬编码现状，
/// 框架级 i18n 方案另票收敛）。
const PICK_BOOK_DIR_BUTTON: &str = "选择账本库目录…";
const PICK_BOOK_DIR_TITLE: &str = "选择账本库目录";
const CREATE_BOOK_CONFIRM_TITLE: &str = "新建账本";
const CREATE_BOOK_CONFIRM_LEAD: &str = "将在所选目录新建账本库：";
const CREATE_BOOK_CONFIRM_STRONG: &str = "目录中没有账本库：确认后将新建一本空账并设为当前账本。";
const CREATE_BOOK_CONFIRM_DETAIL: &str =
    "路径有误请取消后重新选择；新建不会改动其他目录中的任何文件。";
const CREATE_BOOK_CONFIRM_CONFIRM: &str = "新建并打开";
const CREATE_BOOK_CONFIRM_CANCEL: &str = "取消";

/// 选择器恢复现场的启动上下文（注册表根与臂判定的具名形态，F8 收口）：
/// 元组 `(bool, PathBuf)` 的语义自明化——argv 臂只保留「打开时重指装配入口」
/// 语义，注册表根统一默认数据根（F5：登记与活动指针只写数据根的注册表）。
#[derive(Clone, Debug)]
struct PickerLaunchContext {
    /// argv 显式臂：装配入口已注入账本目录（打开时重指选定目录）。
    argv_arm: bool,
    /// 注册表根（默认数据根；登记与活动指针的写入位置）。
    registry_root: PathBuf,
}

/// 失败态出口与新建意图确认的选择器启动上下文解析（ADR-0146 决策 1）。数
/// 据根未注入（headless 缺参态）为 None——该现场不走选择器恢复，调用方零
/// 动作兜底。
fn launch_registry_root() -> Option<PickerLaunchContext> {
    crate::cross_book::default_data_dir().map(|registry_root| PickerLaunchContext {
        argv_arm: book_dir_slot().is_some(),
        registry_root,
    })
}

/// 失败态恢复路径的装配入口重指（argv 臂）：先注入者优先只约束幂等注入
///（bin 注入一次），不约束显式重指——用户在失败态明确选定新落点（决策 4
/// 的「打开」），重指后 argv 臂启动即打开选定目录。
fn repoint_book_dir(dir: PathBuf) {
    *book_dir_slot() = Some(dir);
}

/// 失败态出口的恢复打开步骤（直选打开与确认新建打开共用）：argv 臂把装配
/// 入口重指选定目录后重跑启动竖井；注册表臂直接重跑（解析链打开新活动本，
/// 引导记录随重跑落槽）。就绪后的泵接线与启动任务同口径（Ready 臂同形）。
async fn open_picked_book_dir(state: Signal<StartupView>, argv_arm: bool, picked: PathBuf) {
    if argv_arm {
        repoint_book_dir(picked);
    }
    let outcome = blocking(run_startup)
        .await
        .unwrap_or_else(|_| startup_task_aborted_view());
    if outcome == StartupView::Ready {
        crate::http_host::spawn_pump();
    }
    state.set(outcome);
}

/// 启动竖井的呈现文案（缺参 / 引导中 / 首启引导 / 失败四态；就绪态不走此路
/// 径——表格视图承载，见 [`TransactionsView`]）。首启引导的标题/提示单源在
/// 此（引导面板与 lib 文案断言同源）。
pub fn headline_text(view: &StartupView) -> String {
    match view {
        StartupView::MissingArg => "未指定账本库目录".to_owned(),
        StartupView::Loading => "正在打开账本库…".to_owned(),
        StartupView::Ready => String::new(),
        StartupView::Onboarding { .. } => ONBOARDING_HEADLINE.to_owned(),
        // 旧壳 unlock.title 同词（zh-CN 词条原样承载，#2088）。
        StartupView::Locked => "账本已加密".to_owned(),
        StartupView::Failed { message, .. } => format!("启动失败：{message}"),
    }
}

/// 次行文案（用法提示 / 账本目录回显，验证立桩的可见凭据）。
pub fn secondary_text(view: &StartupView) -> String {
    match view {
        StartupView::MissingArg => "用法：nsh-app <账本目录>".to_owned(),
        StartupView::Onboarding { .. } => ONBOARDING_HINT.to_owned(),
        // 旧壳 unlock.hint 同词（#2088）。
        StartupView::Locked => "输入主口令解锁您的数据".to_owned(),
        _ => match book_dir_slot().as_ref() {
            Some(dir) => dir.display().to_string(),
            None => String::new(),
        },
    }
}

/// 首启引导词表（zh-CN 硬编码；ADR-0146 决策 2，T2 #2092）。「打开已有账
/// 本」动作随 T3 #2093 入屏——动作接线在引导面板按钮（onboarding-pick），
/// 现场分流语义零新写（复用失败态选择器同一任务与闭集单点）。
pub(crate) const ONBOARDING_HEADLINE: &str = "欢迎使用 Ledger";
const ONBOARDING_HINT: &str = "尚未找到账本库，新建一本账即可开始记账。";
const ONBOARDING_CREATE_LABEL: &str = "新建账本";
const ONBOARDING_PICK_LABEL: &str = "打开已有账本";

/// 新建确认对话词表（warning 级，ADR-0078 分级基准：新建非破坏但有知情必
/// 要——完整路径明示在警示句内）。既有库目录场景（选择器复用）不动既有库。
const ONBOARDING_CREATE_TITLE: &str = "新建账本";
const ONBOARDING_CREATE_LEAD: &str = "这是一本新账，创建后立即打开。";
const ONBOARDING_CREATE_WARNING_PREFIX: &str = "确认后将在以下位置创建全新的账本库：";
const ONBOARDING_CREATE_DETAIL: &str =
    "账本库是该目录下的 ledger.db 文件；目录中已有账本库时将直接打开，不会覆盖。";
const ONBOARDING_CREATE_CONFIRM: &str = "创建账本";
const ONBOARDING_CREATE_CANCEL: &str = "取消";

fn view_host(nav: nav::NavStore) -> impl IntoElement {
    // 当前视图信号（Copy 句柄移入分支谓词闭包，'static 无借用逃逸）。
    let current = nav.current;
    let is = move |name: ViewName| move || current.get() == name;
    let group_more = |gid: nav_model::GroupId| move || views::group_more_view(gid);
    // 分支序随迁移系列推进：交易列表 / 预算（#2033）/ 账户（#2041）/
    // 定时（#2042）/ 概览（#2039）/ 搜索（#2040）/ 物品（#2044）/ 报表
    //（#2048）/ 储蓄目标（#2056）/ 设置（#2053）/ 保单（#2050）/ 实物资产
    //（#2049）/ 商户（#2052）/ 跨账本汇总与 AI 提示词（#2055）/ 投资视图
    //（概览/盈亏 #2045、持仓 #2046 ⑮、明细/标的 #2047 ⑯，五位页签齐备）
    // 挂真实组件，三容器挂「更多」页签壳，其余未迁移视图共享占位。
    when(
    is(ViewName::Transactions),
    transactions::transactions_view,
)
.otherwise(move || {
    when(
        is(ViewName::Budget),
        budget::budget_view,
    )
    .otherwise(move || {
        when(
            is(ViewName::Accounts),
            accounts::accounts_view,
        )
        .otherwise(move || {
            when(
                is(ViewName::Scheduled),
                scheduled::scheduled_view,
            )
            .otherwise(move || {
                when(
                    is(ViewName::Items),
                    items::items_view,
                )
                .otherwise(move || {
                    when(
                        is(ViewName::Dashboard),
                        dashboard::dashboard_view,
                    )
                    .otherwise(move || {
                        when(
                            is(ViewName::Search),
                            search::search_view,
                        )
                        .otherwise(move || {
                            when(
                                is(ViewName::Reports),
                                reports::reports_view,
                            )
                            .otherwise(move || {
                                when(
                                    is(ViewName::SavingsGoals),
                                    crate::savings::savings_goals_view,
                                )
                                .otherwise(move || {
                                    when(
                                        is(ViewName::Settings),
                                        settings::settings_view,
                                    )
                                    .otherwise(move || {
                                        when(
                                            is(ViewName::Policies),
                                            crate::policies::policies_view,
                                        )
                                        .otherwise(move || {
                                            when(
                                                is(ViewName::PhysicalAssets),
                                                crate::physical_assets::physical_assets_view,
                                            )
                                            .otherwise(move || {
                                                when(
                                                    is(ViewName::Merchants),
                                                    crate::merchants::merchants_view,
                                                )
                                                .otherwise(move || {
                                                when(
                                                    is(ViewName::Ai),
                                                    crate::ai_prompt::ai_prompt_view,
                                                )
                                                .otherwise(move || {
                                                    when(
                                                        is(ViewName::CrossBookSummary),
                                                        crate::cross_book::cross_book_summary_view,
                                                    )
                                                    .otherwise(move || {
                                                        when(
                                                            is(ViewName::Investments),
                                                            investments::investments_view,
                                                        )
                                                        .otherwise(move || {
                                                            when(
                                                            is(ViewName::BookkeepingMore),
                                                            group_more(nav_model::GroupId::Bookkeeping),
                                                        )
                                                        .otherwise(move || {
                                                            when(
                                                                is(ViewName::AssetsMore),
                                                                group_more(nav_model::GroupId::Assets),
                                                            )
                                                            .otherwise(move || {
                                                                when(
                                                                    is(ViewName::InsightsMore),
                                                                    group_more(nav_model::GroupId::Insights),
                                                                )
                                                                .otherwise(move || {
                                                                        // 未迁移视图反应式占位壳：标题随当前视图；意图消费时机 = 落位
                                                                        //（effect 键 current，同轮落地）。
                                                                        let focused = signal(None::<nav::NavIntent>);
                                                                        effect(move || {
                                                                        let _current = nav.current.get();
                                                                        focused.set(nav::take_intent());
                                                                        });
                                                                        views::placeholder_shell_current(focused)
                                                                    })
                                                                })
                                                            })
                                                        })
                                                    })
                                                })
                                            })
                                        })
                                    })
                                })
                            })
                        })
                    })
                })
            })
        })
    })
})
})
}

/// 根组件构造单点（生产 `main.rs` 与 headless 测试同一条装配路径）：主题
/// 令牌随根挂载（清单挂载解析，[`crate::theme::tokens`]——nsh.json
/// `"theme"` → theme.json 令牌闭集，ADR-0147 决策 1）。拆除 `.tokens(..)`
/// 或清单挂载即回退 UI 集缺省 neutral——出厂基线负向判据的接线点。
pub fn root() -> nsh_core::element::Root {
    nsh_core::element::Root::of::<App>().tokens(crate::theme::tokens())
}

/// 立桩根组件：就绪前居中呈现启动形态（缺参/引导中/首启引导/失败/密文锁定，#2088 起五态），就绪后承载导航骨架（侧栏 + 内容区）。
#[component]
pub fn App() -> impl IntoElement {
    // 导航状态与筛选会话态挂根组件实例（根永生，信号存活与进程同步；后续消费
    // 经同一句柄；筛选会话态「视图卸载不丢」依赖根实例所有权，#2037），键位带
    // 绑定随根实例存亡（引擎线程派发，含抑制闸门）。
    let nav = nav::store();
    tx_filter::store();
    // 交易页视图偏好（#2062）信号同挂根实例：首调若落在交易视图体，断点翻转
    // 重挂视图（owner 释放）后装载读信号即崩——与筛选会话态同一条所有权契约。
    tx_view_preferences::store();
    // 报表页会话态（#2048）同样挂根实例（首调初始化；离开报表视图重挂不丢
    // ——旧壳 reports-session 存活期 = 应用会话同语义）。
    reports::store();
    nav::bind_shortcuts();
    // 显示主题启动钩（#2053）：按存储外观应用全局显色——三环接缝（存储读 /
    // 解析 / 应用）在 settings::apply_startup_appearance 单点内的接线，拆除
    // 即红（headless 外观探针）。
    settings::apply_startup_appearance();
    // 多端同步轮次的失效广播排空任务（issue #2080）：挂根组件实例（根永生）
    // ——调度线程的轮次回调只投递，回引擎线程才广播（signals 线程契约）。唯一
    // 装载点由源码扫描守门钉住（sync_triggers guard_tests）。
    sync_triggers::install_invalidation_drain();
    // 车道反馈泵（#2079）：引擎线程任务、根实例所有 = 进程寿命——行情车道
    // 线程把进度与实际写入投进程级队列，本任务排空（进度进接缝信号、写入
    // 广播投资失效）。删除本行即车道反馈探针红（market_lanes::probe）。
    crate::market_lanes::install_feedback_pump();
    // 自动备份宿主装载期接线（#2078）：设备偏好推送（备份目录镜像）+ 深路径
    // 发射器注入 + 失效信号泵（进程级发射器先装者优先；目录镜像就位早于任何
    // 调度轮）。调度线程挂载在启动竖井成功分支（backup_host::start_session_host）。
    backup_host::install_shell_side();
    let state = signal(StartupView::Loading);
    // 密文库锁定面板信号组（#2088）：根组件体单次构造（owner 纪律见 unlock
    // 模块文档——解锁/重置任务挂根 owner，面板子树卸载不取消在途任务）。
    let locked = unlock::locked_surface(state);
    // 首启引导的新建确认（ADR-0146 决策 2 / ADR-0078 warning 级，T2 #2092）：
    // 开着 = 待确认新建在场；结局回写关窗先行（settings.rs 二选一确认先例）。
    // 确认后任务挂根组件 owner（根永生）——状态翻转到就绪时引导子树卸载，
    // 直呼 spawn 会把创建任务挂到子树 owner 上随卸载被取消（视图装载竞态
    // 先例：app.rs 视图装载的 Owner::run 注）。路径明示取构建期数据根
    //（Onboarding 现场数据根恒已注入；槽位先注入者优先，生命周期 = 进程）。
    let app_owner = Owner::current();
    let onboarding_dir = crate::cross_book::default_data_dir();
    let create_open = signal(false);
    let creating = signal(false);
    let create_error = signal(Option::<String>::None);
    let create_dialog = danger::danger_confirm(
        move || create_open.get(),
        danger::DangerConfirmSpec {
            level: danger::DangerLevel::Warning,
            title: ONBOARDING_CREATE_TITLE.to_owned(),
            lead: Some(ONBOARDING_CREATE_LEAD.to_owned()),
            strong_warning: Some(format!(
                "{ONBOARDING_CREATE_WARNING_PREFIX}{}",
                onboarding_dir
                    .as_ref()
                    .map(|dir| dir.display().to_string())
                    .unwrap_or_default()
            )),
            detail: Some(ONBOARDING_CREATE_DETAIL.to_owned()),
            confirm_label: ONBOARDING_CREATE_CONFIRM.to_owned(),
            cancel_label: ONBOARDING_CREATE_CANCEL.to_owned(),
        },
        move |outcome| {
            create_open.set(false);
            match outcome {
                // 取消 / ESC：留在引导，零副作用（不建库、不写注册表）。
                danger::DangerOutcome::Dismissed => {}
                danger::DangerOutcome::Confirmed => {
                    let Some(target) = crate::cross_book::default_data_dir() else {
                        return;
                    };
                    if creating.get_untracked() {
                        return; // 在途防重入：创建中的二次确认不入队
                    }
                    creating.set(true);
                    create_error.set(None);
                    app_owner.run(move || {
                        spawn(async move {
                            // 建库 + 登记 + 置指针（复用面核心，boot::create_default_book）
                            // 成功后重走启动竖井：解析链打开新账本，日志/备份宿主/
                            // HTTP 壳/同步触发/行情车道等就绪分支接线一次性完成——
                            // 本次启动直接进主界面，引导不再回归。
                            let outcome = blocking(move || {
                                match boot::create_default_book(&target) {
                                    Ok(()) => Ok(run_startup()),
                                    Err(e) => Err(e.to_string()),
                                }
                            })
                            .await
                            .unwrap_or_else(|_| Err("内部错误：创建账本任务异常退出".to_owned()));
                            creating.set(false);
                            match outcome {
                                Ok(view) => state.set(view),
                                // 创建失败留在引导（错误随面板呈现，可重试）；
                                // 建库成功但竖井失败走既有失败态呈现。
                                Err(message) => create_error.set(Some(message)),
                            }
                        });
                    });
                }
            }
        },
    );
    // 主行/次行前景取主题令牌（双态自适应）：裸 text 的默认前景是黑
    // （ResolvedTextStyle::default 的 React Native 惯例），暗色桌面上黑字贴
    // 暗底不可见——窗口文字必须显式带色。消费面走 crate::theme 单点（缺失
    // 兜底归 theme.rs）；启动诊断日志另读 registry 原始在册状态。
    let tokens = nsh_core::element::tokens();
    tracing::debug!(
        foreground = ?tokens.color("foreground"),
        muted = ?tokens.color("muted-foreground"),
        "启动视图主题令牌"
    );
    let crate::views::ViewPalette {
        fg,
        muted,
        danger_fg,
    } = crate::views::view_palette();
    // 失败态出口（ADR-0146 决策 4）与新建意图确认（决策 3 分支③）的现场：
    // 挂起新建目录信号、选择器任务与对话结局处理（组件期一次构造，信号驱动
    // 开合）。
    let owner = Owner::current();
    let create_confirm: Signal<Option<PathBuf>> = signal(None);
    let pick_book_dir_task = {
        move |_: &()| {
            owner.run(move || {
                spawn(async move {
                    // 目录选择对话框（#2038 接缝）：Err = 平台能力缺失 → 既有
                    // nsh-shell.file-dialog-failed 码化呈现；Ok(None) = 取消 →
                    // 原失败态原样保留（误触零后果，出口可再触发）。
                    let answer = pick_paths(OpenDialog::folders().title(PICK_BOOK_DIR_TITLE)).await;
                    match answer {
                        Err(err) => state.set(StartupView::Failed {
                            message: AppError::coded(
                                boot::CODE_FILE_DIALOG_FAILED,
                                format!("文件对话框无法打开：{err}"),
                            )
                            .to_string(),
                            can_pick_book_dir: true,
                        }),
                        Ok(None) => {}
                        Ok(Some(paths)) => {
                            let Some(picked) = paths.into_iter().next() else {
                                return;
                            };
                            let Some(ctx) = launch_registry_root() else {
                                return;
                            };
                            // 现场闭集处置单点（决策 3，与首启引导「打开已有
                            // 账本」同函数）；密文/损坏残留错误经 failed_view
                            // 上屏（出口自动消失——缺的是解锁与诊断）。
                            let resolved = {
                                let picked = picked.clone();
                                let registry_root = ctx.registry_root.clone();
                                match blocking(move || {
                                    boot::resolve_picked_book_dir(&registry_root, &picked)
                                })
                                .await
                                {
                                    Ok(resolved) => resolved,
                                    Err(_) => {
                                        state.set(startup_task_aborted_view());
                                        return;
                                    }
                                }
                            };
                            match resolved {
                                Err(e) => state.set(failed_view(&e)),
                                Ok(boot::PickedBookDisposition::Open) => {
                                    open_picked_book_dir(state, ctx.argv_arm, picked).await;
                                }
                                Ok(boot::PickedBookDisposition::ConfirmCreate(dir)) => {
                                    create_confirm.set(Some(dir));
                                }
                            }
                        }
                    }
                })
            });
        }
    };
    let create_book_outcome = {
        move |outcome: danger::DangerOutcome| {
            let picked = create_confirm.get_untracked();
            // 关窗先行（设置面接管确认同形）；取消/ESC 同为放弃——原失败态
            // 原样保留，出口可再触发。
            create_confirm.set(None);
            if outcome == danger::DangerOutcome::Confirmed
                && let Some(dir) = picked
            {
                let Some(ctx) = launch_registry_root() else {
                    return;
                };
                owner.run(move || {
                    spawn(async move {
                        // 建库 + 登记（决策 3 分支③完成动作，F4 换序：先建
                        // 库后登记——建库失败注册表零写入，确认对话可重试）。
                        let created = {
                            let dir = dir.clone();
                            let registry_root = ctx.registry_root.clone();
                            match blocking(move || {
                                boot::confirm_create_picked_book(&registry_root, &dir)
                            })
                            .await
                            {
                                Ok(created) => created,
                                Err(_) => {
                                    state.set(startup_task_aborted_view());
                                    return;
                                }
                            }
                        };
                        match created {
                            Err(e) => state.set(failed_view(&e)),
                            Ok(()) => open_picked_book_dir(state, ctx.argv_arm, dir).await,
                        }
                    })
                });
            }
        }
    };
    let create_book_dialog = when(
        move || create_confirm.get().is_some(),
        move || {
            // 对话框随挂起路径重建（明示路径随选定变化）；开合追踪与抑制上
            // 报由封装按 open 闭包承担。
            let dir = create_confirm.get_untracked().unwrap_or_default();
            danger::danger_confirm(
                move || create_confirm.get().is_some(),
                danger::DangerConfirmSpec {
                    level: danger::DangerLevel::Warning,
                    title: CREATE_BOOK_CONFIRM_TITLE.to_owned(),
                    lead: Some(format!("{CREATE_BOOK_CONFIRM_LEAD}{}", dir.display())),
                    strong_warning: Some(CREATE_BOOK_CONFIRM_STRONG.to_owned()),
                    detail: Some(CREATE_BOOK_CONFIRM_DETAIL.to_owned()),
                    confirm_label: CREATE_BOOK_CONFIRM_CONFIRM.to_owned(),
                    cancel_label: CREATE_BOOK_CONFIRM_CANCEL.to_owned(),
                },
                create_book_outcome,
            )
        },
    );
    spawn(async move {
        // blocking 的 JoinHandle：启动竖井不 panic（失败走 Result）；JoinError 仅在
        // 任务异常退出时出现，归入失败态呈现（不 panic 引擎线程）。
        let outcome = blocking(run_startup)
            .await
            .unwrap_or_else(|_| startup_task_aborted_view());
        // AI 导入 HTTP 壳的泵任务接线（#2081）：挂载在 run_startup 竖井内
        // （blocking 后台线程），泵需引擎线程投递壳内族广播（crate::signals
        // thread_local 订阅者表线程契约），在此竖井 await 回引擎后接线。
        // 删除本接线即 headless 泵断言红（tests/app_headless.rs）。
        if outcome == StartupView::Ready {
            crate::http_host::spawn_pump();
        }
        state.set(outcome);
    });
    view().flex_1().flex_col().app_class("bg-background").child(
        // 启动形态与导航骨架互斥呈现：就绪前居中启动形态（含密文锁定面板），就绪后侧栏 + 内容区。
        // 单一分支承载内容区（侧栏与视图宿主同分支），不双挂。
        when(
            move || matches!(state.get(), StartupView::Ready),
            move || {
                // 引导回退告示（ADR-0146 决策 1 现场 D）：注册表损坏 / 活动
                // 目录不可用回退默认数据目录时，主界面照常打开，顶部告示条
                // 显著提示回退原因（boot 记录先于就绪态落槽，呈现时恒已就位；
                // 无回退零呈现，既有视觉不变）——老壳「失败恢复屏显著提示」
                // 语义的窗口承载；设置卡回退行（loc-fallback）同源同款式。
                let fallback = boot::boot_record().and_then(|record| record.fallback_reason);
                let has_fallback = fallback.is_some();
                let reason = fallback.unwrap_or_default();
                // 窗口分级分支（#2057 / ADR-0088 决策 2/4）：≥ 断点桌面档渲染
                // 既有侧栏布局（宽屏主形态一字不动，迁移系列红线）；< 断点移
                // 动档渲染导航壳（顶栏 + 导航抽屉——抽屉消费同一份导航状态，
                // [`crate::nav`] ViewState 语义与存储零改动）。断点判定单点
                // 住 [`crate::adaptive`]，形态分档只改布局分支、不复位任何
                // 信号（导航/筛选/分页会话态两侧同源——断点切换平滑、状态不丢）。
                let body = when(crate::adaptive::is_narrow, move || {
                    view()
                        .flex_1()
                        .flex_col()
                        .child(crate::adaptive::top_bar(fg, muted))
                        .child(view_host(nav))
                        .child(crate::adaptive::nav_drawer())
                })
                .otherwise(move || {
                    view()
                        .flex_1()
                        .flex_row()
                        .child(sidebar::sidebar_view())
                        .child(view_host(nav))
                });
                view()
                    .flex_1()
                    .flex_col()
                    .child(
                        when(
                            move || has_fallback,
                            move || {
                                fallback_notice_view(reason.clone(), "boot-fallback-title", "boot-fallback", fg, muted)
                            },
                        )
                        .otherwise(view),
                    )
                    .child(body)
            },
        )
        .otherwise(move || {
            // 密文库锁定面板（#2088）优先分流：解锁 + 忘口令重置入口；文案
            // 与 headline_text / secondary_text 的 Locked 臂同源。其余非就
            // 绪态（首启引导 / 缺参 / 引导中 / 失败）保持既有呈现——首启
            // 引导（ADR-0146 决策 2，T2 #2092）呈现引导面板（欢迎 + 新建
            // 动作；损坏告示与确认对话见下），失败态另挂「选择账本库
            // 目录…」出口（ADR-0146 决策 4，T4 #2094）。
            when(
                move || state.get() == StartupView::Locked,
                move || unlock::locked_panel(locked, fg, muted),
            )
            .otherwise(move || {
                when(
                    move || matches!(state.get(), StartupView::Onboarding { .. }),
                    move || {
                    view()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .test_id("onboarding-panel")
                        .child(
                            text(move || headline_text(&state.get()))
                                .text_xl()
                                .text_color(fg)
                                .test_id("startup-headline"),
                        )
                        .child(
                            text(move || secondary_text(&state.get()))
                                .text_xs()
                                .text_color(muted)
                                .test_id("onboarding-hint"),
                        )
                        // 损坏告示（ADR-0146 决策 2）：注册表损坏回退后无库的
                        // 引导现场，回退原因随引导记录落槽（boot_ledger_via_registry），
                        // 与就绪态回退告示（现场 D）同源同款式、设置卡回退行同词；
                        // 干净出厂现场零呈现。告示知情后可继续完成账本指定（新建
                        // 动作照常可用；确认后 write_registry 覆盖损坏指针文件——
                        // 与出厂逃生舱折叠语义同向）。
                        .child({
                            let fallback = boot::boot_record().and_then(|record| record.fallback_reason);
                            let has_fallback = fallback.is_some();
                            let reason = fallback.unwrap_or_default();
                            when(
                                move || has_fallback,
                                move || {
                                    fallback_notice_view(reason.clone(), "onboarding-fallback-title", "onboarding-fallback", fg, muted)
                                },
                            )
                            .otherwise(view)
                        })
                        .child(
                            // 「新建账本」动作（决策 2 双动作之一）。按 press
                            // 开确认对话（明示完整路径），确认后建库+登记+置指针
                            // 并重走启动竖井（接线在 create_dialog 结局处）。
                            // 主操作按钮规格对表 docs/DESIGN.md §6（h-8 32pt 默认
                            // 档；#2101 记一笔同款）。
                            button()
                                .app_class("h-8 px-3 rounded-(--radius) bg-primary hover:opacity-90")
                                .child(
                                    text(ONBOARDING_CREATE_LABEL)
                                        .text_sm()
                                        .text_color(crate::theme::color("primary-foreground"))
                                        .test_id("onboarding-create"),
                                )
                                .on_press(move || create_open.set(true)),
                        )
                        .child(
                            // 「打开已有账本」动作（决策 2 双动作之二，T3 #2093）：
                            // 按压开目录选择器，选定目录交 T4 现场闭集处置单点
                            //（resolve_picked_book_dir，与失败态出口同一任务
                            // pick_book_dir_task——现场分流语义只有一份）：①②
                            // 直接打开、③转新建确认对话（create_confirm 信号驱
                            // 动，组件体挂载）、④⑤码化错误呈现；取消零副作用留
                            // 引导。删本接线即 headless 引导选择器用例红（ADR-0087）。
                            button()
                                .app_class(
                                    "h-8 px-3 rounded-(--radius) border border-border text-foreground hover:bg-accent",
                                )
                                .child(
                                    text(ONBOARDING_PICK_LABEL)
                                        .text_sm()
                                        .text_color(fg)
                                        .test_id("onboarding-pick-book-dir"),
                                )
                                .on_press(pick_book_dir_task),
                        )
                        // 创建失败留在引导（错误可读、动作可重试；成功路径不经此）。
                        .child(
                            when(
                                move || create_error.get().is_some(),
                                move || {
                                    text(move || create_error.get().unwrap_or_default())
                                        .text_xs()
                                        .text_color(danger_fg)
                                        .test_id("onboarding-error")
                                },
                            )
                            .otherwise(view),
                        )
                },
            )
            .otherwise(move || {
                view()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(
                        text(move || headline_text(&state.get()))
                            .text_xl()
                            .text_color(fg)
                            .test_id("startup-headline"),
                    )
                    .child(
                        text(move || secondary_text(&state.get()))
                            .text_xs()
                            .text_color(muted),
                    )
                    .child(
                        // book-missing 失败态的显式出口（ADR-0146 决策 4）：仅密文
                        // 与损坏残留失败态不挂（can_pick_book_dir = false，
                        // failed_view 单点派生）。删本接线即 headless 失败态断言红
                        //（ADR-0087 接线断言）。
                        when(
                            move || {
                                matches!(
                                    state.get(),
                                    StartupView::Failed {
                                        can_pick_book_dir: true,
                                        ..
                                    }
                                )
                            },
                            move || {
                                button()
                                    .app_class(
                                        "h-8 px-3 rounded-(--radius) border border-border text-foreground hover:bg-accent",
                                    )
                                    .child(
                                        text(PICK_BOOK_DIR_BUTTON)
                                            .text_sm()
                                            .text_color(fg)
                                            .test_id("startup-pick-book-dir"),
                                    )
                                    .on_press(pick_book_dir_task)
                            },
                        )
                        .otherwise(view),
                    )
                })
            })
        }),
    )
    // 新建确认对话挂在启动容器外层（关窗时零渲染；开时遮罩 inset_0 覆盖全
    // 窗——危险确认封装：遮罩不关、ESC 可关、开合上报 Overlay Suppression，
    // 快捷键抑制经封装族单通道接入）。元素非 Copy，不能移入面板 Fn 闭包，
    // 故挂组件体这一层（OpenDialog 只从引导面板按钮开启）。
    .child(create_dialog)
    .child(create_book_dialog)
}

#[cfg(test)]
mod shortcut_binding_tests {
    use nsh_core::keymap::{KeyBinding, Keystroke, When};

    /// 可编辑焦点闸（ADR-0141 决策 4 三闸门之一）的绑定侧钉子：四个裸键皆为
    /// 文本编辑键，`When::Auto` 缺省绑定的 `in_text = false`——文本输入持有
    /// 键盘焦点期间引擎不派发（keymap.find 的 editing 判定）。本测钉住本壳
    /// 「以 Auto 绑定」的选择；UI 级证明（搜索框焦点期间快捷键静默）随 #2023
    /// 搜索接入落地。裸键语义（拒绝一切修饰键）由引擎 keystroke 精确匹配
    /// （含修饰位）承担，一并钉住。
    #[test]
    fn bare_shortcut_bindings_do_not_fire_while_editing() {
        for text in ["[", "]", ",", "."] {
            let stroke = Keystroke::parse_with(text, false).unwrap();
            assert!(
                stroke.edits_text(),
                "{text} 是文本编辑键（输入焦点期间抑制的前提）"
            );
            let binding = KeyBinding::new(0, stroke, When::Auto, false);
            assert!(!binding.in_text, "When::Auto 绑定不在编辑中派发：{text}");
            let mut with_modifier = stroke;
            with_modifier.modifiers.shift = true;
            assert_ne!(
                with_modifier, stroke,
                "带修饰键的同名键是另一 keystroke，不命中"
            );
        }
    }
}
