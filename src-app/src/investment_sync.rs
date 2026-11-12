//! 投资同步接缝与价格过期提示（issue #2046；spec #2031 系列 15/26，投资 2/3）。
//!
//! **同步接缝**（旧壳 `useInstrumentInfoSync` 的重生，模块级共享单例）：
//! 「同步标的信息」进行中 / 进度 / 结果 / 降级四件状态住进程级信号单例——
//! 持仓卡同步钮（本票落位）与「去同步」入口共用同一份状态，任一入口在途
//! 时其余入口的触发一律短路（不并发第二场同步，旧壳在途承诺同语义）；标的
//! 库页签（#2047 ⑯）消费同单点。同步全程发确定进度（issue #897）：编排
//! 进度回调直达本接缝信号（非阻塞逐次覆写，终态收起清空；迟到事件不复活
//! 进度条）；进度不是失效信号——价格/名称实际写入的投资失效广播按写入
//! 见证由壳侧裁决（见 [`crate::boot::sync_instrument_info`] 收尾单点注释
//! 与 [`trigger_sync`]）。
//!
//! **价格过期提示**（旧壳 `usePriceStaleness` + staleness 词表的重生）：打开
//! 投资视图时一次本地水位检查（零网络请求，判定口径单点在投资域
//! `instrument_price_staleness`，本面只搬运计数与阈值），有过期（或持仓缺现价）
//! 才出提示与「去同步」动作；投资失效信号后重查——同步实际写价后提示随水位
//! 变新自动消失。检查失败静默降级为「不提示」（背景提示，宁可不提示也不误报，
//! 旧壳 silent Loadable 同口径）。
//!
//! **「去同步」语义（旧壳同口径）**：按钮切到标的库页签——标的页签
//!（#2047 ⑯）就是既有同步入口（按钮 + 进度条 + 结果消息复用同一接缝），
//! 不新增第二套同步触发（#2046 ⑮ 的「动作直达同步接缝」临时形态已随 ⑯
//! 落位退役，回到旧壳页签跳转同口径；《父 spec 对表手册》偏差已随 ⑯ 交付
//! 回写）。
//!
//! 界面文案仅 zh-CN（旧壳 investments.json staleness/sync 段词表直取）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_core::prelude::*;

use crate::design::font_size;
use crate::{app, boot, investments};

// ---------------------------------------------------------------------------
// 词表（旧壳 investments.json staleness / sync 段逐条平价）
// ---------------------------------------------------------------------------

/// 提示主文案（staleness.message；{count} / {days} 由装配闭集填充）。
pub(crate) const STALENESS_MESSAGE_FMT: &str =
    "有 {count} 个标的的行情 / 净值可能已过期（超过 {days} 天未更新，或持仓缺现价）。";
/// 「去同步」动作文案（staleness.goSync）。
pub(crate) const STALENESS_GO_SYNC: &str = "去同步标的信息";
/// 提示言外之意（staleness.hint）。
pub(crate) const STALENESS_HINT: &str = "同步只刷新现价；净值披露常滞后 1~2 个交易日、长假顺延，数据源暂无新数据时提示不会消失；走势历史由后台自动补全。";
/// 同步失败文案（investments.sync.failed：{message} 装配闭集填充）。
pub(crate) const SYNC_FAILED_FMT: &str = "同步失败：{message}";
/// 降级标注（investments.sync.degraded）。
pub(crate) const SYNC_DEGRADED: &str = "已降级、本次较慢";
/// 同步按钮文案（investments.holdings.sync）。
pub(crate) const SYNC_LABEL: &str = "同步标的信息";
/// 同步进行中按钮文案（旧壳按钮 loading 的本壳文字态）。
pub(crate) const SYNCING_LABEL: &str = "同步中…";
/// 同ST 进度行文案（investments.sync.progress：{done}/{total} 装配）。
fn sync_progress_text_of(done: u32, total: u32) -> String {
    format!("同步标的信息 {done}/{total}")
}

/// 会话缺失 / 任务异常退出的内部错误（boot 读面同词）。
const MSG_INTERNAL_SESSION: &str = "内部错误：账本库会话缺失";

/// 过期计数快照（staleness 检查结果投影）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Staleness {
    pub(crate) count: u64,
    pub(crate) threshold_days: i64,
}

/// 提示快照 → 主文案（装配闭集：{count} / {days} 一次填充，渲染期零格式化）。
pub(crate) fn staleness_message(staleness: Staleness) -> String {
    STALENESS_MESSAGE_FMT
        .replace("{count}", &staleness.count.to_string())
        .replace("{days}", &staleness.threshold_days.to_string())
}

// ---------------------------------------------------------------------------
// 同步接缝（模块级共享单例；旧壳 useInstrumentInfoSync 状态的重生）
// ---------------------------------------------------------------------------

/// 同步接缝状态束（进程级单例的信号面：持仓卡 / 「去同步」/ ⑯ 标的页签任一
/// 入口消费同份——进行中 / 结果消息 / 降级位 / 确定进度四件随终态覆写；
/// 句柄随首读组件实例建立，切页不丢状态）。
#[derive(Clone, Copy)]
pub(crate) struct SyncSeam {
    /// 同步进行中（按钮 loading 的唯一来源）。
    pub(crate) syncing: Signal<bool>,
    /// 结果反馈消息（成功 = 域结果统计 message；失败 = SYNC_FAILED 装配；
    /// None = 无反馈）。旧壳 resultMessage 同射程。
    pub(crate) message: Signal<Option<String>>,
    /// 反馈着色（true = 成功 / false = 失败；与 message 同生命周期）。
    pub(crate) success: Signal<bool>,
    /// 降级事实位（本次回退逐标的通道较慢，issue #1376；缺失字段（旧后端
    /// 形状）按未降级处置同口径）。
    pub(crate) degraded: Signal<bool>,
    /// 确定进度（issue #897 同在途约定：终态收起清空）。
    pub(crate) progress_done: Signal<u32>,
    pub(crate) progress_total: Signal<u32>,
}

static SEAM: std::sync::Mutex<Option<SyncSeam>> = std::sync::Mutex::new(None);

fn seam_slot() -> std::sync::MutexGuard<'static, Option<SyncSeam>> {
    SEAM.lock().unwrap_or_else(|p| p.into_inner())
}

/// 同步接缝状态单例（惰性初始化；旧壳「全部接缝状态为模块级共享单例」同
/// 源——切页不丢状态，短路入口与发起入口零分叉）。创建面走
/// `Owner::detached`（无属主 = 图级寿命）：接缝句柄被任意视角的闭包
/// 消费（按钮禁用态 / 迚度条 / 结果行 / 触发面），归属给首个建设者子树会
/// 在其卸载后失效（headless 空库入口用例钉住该创建面）。
pub(crate) fn seam() -> SyncSeam {
    if let Some(s) = *seam_slot() {
        return s;
    }
    let s = Owner::detached().run(|| SyncSeam {
        syncing: signal(false),
        message: signal(None),
        success: signal(false),
        degraded: signal(false),
        progress_done: signal(0),
        progress_total: signal(0),
    });
    *seam_slot() = Some(s);
    s
}

/// 复位接缝单例（headless 测试隔离；[`crate::app::reset_book_assembly`]
/// 复位点消费，生产 bin 不消费）。
#[doc(hidden)]
pub fn reset_for_tests() {
    *seam_slot() = None;
}

// ---------------------------------------------------------------------------
// 同步触发（在途短路 + 终态回写 + 成败同判的失效广播）
// ---------------------------------------------------------------------------

/// 发起「同步标的信息」（旧壳 useInstrumentInfoSync.sync() 同名接缝）：
/// 在途短路（任一入口在途时其余触发一致静返——不并发第二场同步，表现面 =
/// 共享状态）；不在途时清终态、置进行中、编排任务随视图实例（失败后宿主
/// 子树卸载不取消，读/写任务先例同形），终态回写接缝状态（进度收起清空 /
/// 结果反馈 / 降级位），实际写过（成败同判——失败 ≠ 未写过，中途失败已
/// 落库的写入仍生效）广播投资失效信号：持仓卡与提示条重拉（ADR-0031 后端
/// 写价 → 失效信号 → 消费方自行重拉、不手动刷新——删除接线即红断言钉在
/// lib 探针）。
pub(crate) fn trigger_sync() {
    let seam = seam();
    // 在途短路：进行中即返回（无第二场并发同步；表现面 = 共享单例）。
    if seam.syncing.get_untracked() {
        return;
    }
    seam.message.set(None);
    seam.success.set(false);
    seam.degraded.set(false);
    seam.progress_done.set(0);
    seam.progress_total.set(0);
    seam.syncing.set(true);
    let db = app::db_slot().clone();
    spawn(async move {
        let Some(db) = db.as_ref() else {
            finish_err(MSG_INTERNAL_SESSION.to_owned());
            return;
        };
        // 进度回调（issue #897）：编排逐次直达接缝信号（非阻塞覆写；迟到
        // 事件由终态收起收口——终态一次清零，其后回调不复活进度条）。
        let progress_done = seam.progress_done;
        let progress_total = seam.progress_total;
        let outcome = boot::sync_instrument_info(db, move |p: ledger_market_sync::SyncProgress| {
            progress_done.set(p.done as u32);
            progress_total.set(p.total as u32);
        })
        .await;
        match outcome {
            boot::SyncInstrumentInfoOutcome::Success(result, evidence) => {
                finish_success(seam, result.message, result.bulk_degraded, evidence);
            }
            boot::SyncInstrumentInfoOutcome::Failed(error, evidence) => finish_failed(
                seam,
                SYNC_FAILED_FMT.replace("{message}", &error.to_string()),
                evidence,
            ),
        }
    });
}

/// 成功终态：结果反馈（成功着色）+ 域消息 + 降级位 + 进度收起；实际写过
/// （成败同判）广播投资失效信号。
fn finish_success(seam: SyncSeam, message: String, degraded: bool, evidence: bool) {
    seam.syncing.set(false);
    seam.success.set(true);
    seam.degraded.set(degraded);
    seam.progress_done.set(0);
    seam.progress_total.set(0);
    seam.message.set(Some(message));
    if evidence {
        crate::signals::notify_investments_invalidated();
    }
}

/// 失败终态：失败反馈 + 进度收起；写入证据（成败同判）仍广播——失败 ≠
/// 未写过，界面照收尾裁决自动刷新一次（旧壳 issue #1277 同语义）。
fn finish_failed(seam: SyncSeam, message: String, evidence: bool) {
    seam.syncing.set(false);
    seam.success.set(false);
    seam.degraded.set(false);
    seam.progress_done.set(0);
    seam.progress_total.set(0);
    seam.message.set(Some(message));
    if evidence {
        crate::signals::notify_investments_invalidated();
    }
}

/// 会话缺失之类的失败收尾（无编排证据路径，库未变——不广播）。
fn finish_err(message: String) {
    let seam = seam();
    seam.syncing.set(false);
    seam.success.set(false);
    seam.degraded.set(false);
    seam.progress_done.set(0);
    seam.progress_total.set(0);
    seam.message.set(Some(message));
}

// ---------------------------------------------------------------------------
// 价格过期检查（旧壳 usePriceStaleness 的重生；InvestmentsView 告警位消费）
// ---------------------------------------------------------------------------

/// staleness 状态槽（进程级单例：快照句柄 + 请求序号守卫——视图落位与失效
/// 信号两处并发装载，迟到前发检查不覆写新检查结果，旧壳 Loadable 竞态裁决
/// 同口径）。
#[derive(Clone, Copy)]
pub(crate) struct StalenessState {
    /// 检查快照（None = 未装载 / 失败降级——不提示）。
    pub current: Signal<Option<Staleness>>,
    /// 请求序号（递增；进度守卫同形——序号失配的自弃）。
    pub seq: Signal<u64>,
}

static STALENESS: std::sync::Mutex<Option<StalenessState>> = std::sync::Mutex::new(None);

fn staleness_slot() -> std::sync::MutexGuard<'static, Option<StalenessState>> {
    STALENESS.lock().unwrap_or_else(|p| p.into_inner())
}

/// staleness 状态单例（惰性初始化，形态同 [`crate::investments::store`]；
/// 创建面走 `Owner::detached`——同 [`seam`] 的图级寿命纪律，提示条的渲染
/// effect 随提示条存亡而接缝状态横于页签存亡之上）。
pub(crate) fn staleness_store() -> StalenessState {
    if let Some(s) = *staleness_slot() {
        return s;
    }
    let s = Owner::detached().run(|| StalenessState {
        current: signal(None),
        seq: signal(0),
    });
    *staleness_slot() = Some(s);
    s
}

/// 复位 staleness 单例（headless 测试隔离；复位点消费）。
#[doc(hidden)]
pub fn reset_staleness_for_tests() {
    *staleness_slot() = None;
}

/// 复位全部本模块单例（headless 复位点单入口：同步接缝 + staleness 两槽）。
#[doc(hidden)]
pub fn reset_all_for_tests() {
    reset_for_tests();
    reset_staleness_for_tests();
}

/// staleness 装载（视图落位 + 投资失效信号两处共用；阻塞读走后台，
/// SQLite IO 不占引擎线程；失败 = None 静默降级为「不提示」——背景提示
/// 不是阻塞路径，宁可不提示也不误报，旧壳 silent Loadable 同口径）。
pub(crate) fn load_staleness() {
    let state = staleness_store();
    state.seq.update(|seq| *seq += 1);
    let seq = state.seq.get();
    let db = app::db_slot().clone();
    spawn(async move {
        // 会话缺失 = 静默降级「不提示」（背景提示不是阻塞路径）；DbState
        // 克隆件拥有地进阻塞闭包（`blocking` 要求 'static——借用面溢出即
        // 编译红，此处不留引用形式的过路写入）。
        let Some(db) = db else {
            return;
        };
        let read = blocking(move || {
            boot::instrument_price_staleness(&db)
                .map(|direct| Staleness {
                    count: direct.stale_count as u64,
                    threshold_days: direct.threshold_days,
                })
                .ok()
        })
        .await;
        // 序号守卫：期间又有新装载（失效信号重查）即这次结果作废。
        if state.seq.get() != seq {
            return;
        }
        state.current.set(read.unwrap_or(None));
    });
}

/// 价格过期提示条（旧壳 InvestmentsView 告警位同位）：有过期（或持仓缺现价）
/// 才出场——主文案 + 「去同步」（动作 = 切到标的库页签，旧壳
/// goSyncInstrumentInfo 同口径）+ 言外
/// 之意小字。落位装载一次 + 订阅投资失效信号重查（信号删除即红断言钉在
/// lib 探针：删接线后同步写价提示不再收起）。（组件函数名 PascalCase——
/// 宏按 snake 进程生成 `staleness_alert()` 构造件，原名会与原函数相撞。）
#[component]
pub(crate) fn StalenessAlert() -> impl IntoElement {
    let palette = investments::pane_palette();
    let state = staleness_store();
    // 落位装载 + 失效信号重查（句柄随本提示条实例存亡）。
    effect(move || {
        let _ = crate::nav::store().current.get();
        load_staleness();
    });
    let invest_sub = crate::signals::subscribe_investments_changed(load_staleness);
    effect(move || {
        let held: &crate::signals::InvestmentsChangedSubscription = &invest_sub;
        let _ = held;
    });
    when(
        move || state.current.get().is_some_and(|s| s.count > 0),
        move || {
            view()
                .flex_col()
                .gap_1()
                .mx_4()
                .my_2()
                .p_3()
                .rounded_lg()
                .test_id("staleness-alert")
                .app_class("border border-border bg-background")
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            text(move || {
                                state
                                    .current
                                    .get()
                                    .map(staleness_message)
                                    .unwrap_or_default()
                            })
                            .text_size(px(font_size::CAPTION))
                            .text_color(palette.danger)
                            .test_id("staleness-message"),
                        )
                        .child(
                            // 「去同步」（旧壳 goSyncInstrumentInfo 同口径）：
                            // 切到标的页签——页签的同步钮 + 进度条 + 结果行
                            // 就是同一接缝的呈现位，不新增第二套同步触发。
                            view()
                                .on_press(move |_: &PressEvent| {
                                    investments::store()
                                        .tab
                                        .set(investments::InvestmentsTab::Instruments);
                                })
                                .rounded_lg()
                                .px_2()
                                .py_1()
                                .app_class("hover:bg-accent")
                                .child(
                                    text(STALENESS_GO_SYNC)
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(palette.info)
                                        .test_id("staleness-go-sync"),
                                ),
                        ),
                )
                .child(
                    text(STALENESS_HINT)
                        .text_size(px(font_size::CAPTION))
                        .text_color(palette.muted)
                        .test_id("staleness-hint"),
                )
        },
    )
    .otherwise(view)
}

// ---------------------------------------------------------------------------
// 同步进度 / 反馈呈现位（持仓卡 / 「去同步」提示条通用消费面）
// ---------------------------------------------------------------------------

/// 确定进度条（issue #897）：进行中才呈现，槽道全宽、内条按 done/total
/// 宽度推进 + 比率行；终态收起清空（迟到事件不复活进度条——进行中拦截）。
pub(crate) fn sync_progress_bar(palette: investments::PanePalette) -> impl IntoElement {
    let seam = seam();
    when(
        move || seam.syncing.get(),
        move || {
            let pct = move || {
                let total = seam.progress_total.get();
                if total == 0 {
                    0.0
                } else {
                    ((seam.progress_done.get() as f32 / total as f32) * 100.0).min(100.0)
                }
            };
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .test_id("investment-sync-progress")
                .child(
                    view()
                        .w(px(240.0))
                        .h(px(6.0))
                        .rounded_full()
                        .app_class("bg-accent overflow-hidden")
                        .child(
                            view()
                                .h(px(6.0))
                                .rounded_full()
                                .bg(palette.fg)
                                .w(move || px(240.0 * pct() / 100.0)),
                        ),
                )
                .child(
                    text(move || {
                        sync_progress_text_of(seam.progress_done.get(), seam.progress_total.get())
                    })
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.muted),
                )
        },
    )
    .otherwise(view)
}

/// 同步反馈行（终态后结果消息着色呈现 + 降级标注；成功/失败两态同一行；
/// 旧壳 resultMessage + SyncDegradedNotice 的同位合并呈现）。
pub(crate) fn sync_result_row(palette: investments::PanePalette) -> impl IntoElement {
    let seam = seam();
    when(
        move || seam.message.get().is_some(),
        move || {
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    text(move || seam.message.get().unwrap_or_default())
                        .text_size(px(font_size::CAPTION))
                        .text_color(move || {
                            if seam.success.get() {
                                palette.fg
                            } else {
                                palette.danger
                            }
                        })
                        .test_id("investment-sync-result"),
                )
                .child(
                    when(
                        move || seam.degraded.get(),
                        move || {
                            // 降级可见（issue #1376 / ADR-0121 决策 4）：本次
                            // 回退逐标的通道时明示本次较慢。
                            text(SYNC_DEGRADED)
                                .text_size(px(font_size::CAPTION))
                                .text_color(palette.muted)
                                .test_id("investment-sync-degraded")
                        },
                    )
                    .otherwise(view),
                )
        },
    )
    .otherwise(view)
}
