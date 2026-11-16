//! 侧栏底部账本切换区（issue #2129）：web 参照侧栏左下角账本入口（issue
//! #834 / ADR-0089）的 NSH 重生——展开态形态（当前账本名触发件 → 清单弹层
//! ）；折叠浮标形态不在本票（NSH 侧栏无折叠态，窄窗横切票另裁）。
//!
//! 票面闭集（#2129）：账本清单、活动指针指示（活动行高亮 + 「当前」标注）、
//! 切换动作、跨账本投资汇总置顶入口。新建 / 改名 / 移除不住此——登记变更
//! 是设置面账本切换卡的管辖面，两处各守各的语义边界（#2129 范围条款：与
//! 设置面数据存储位置卡的换本 / 搬迁入口互不重复）；侧栏只消费与设置卡同
//! 一的 boot 写缝（[`boot::switch_book`]），不设第二登记写入口。
//!
//! 换本生效时机（web 参照既有语义开工核对结论）：web `useBookSwitcher` =
//! 轻量确认 → `api.switchBook` 写活动指针 → `restartAppShortly()` 原位重引
//! 导重载进目标账本（确认后**立即生效**）；本壳无原位重引导（既有偏差，
//! [`boot::switch_book`] 与设置面账本切换卡同款口径），确认后写活动指针 +
//! 状态条提示重启生效——指针写链单点归 boot，呈现与确认形态对齐设置卡。
//! ADR-0146 首启现场闭集零接触；搬迁窗口内登记不可变（mutable=false）拒绝
//! 切换，与设置卡同门。清单刷新时机对齐 web `useBookSwitcher`：挂载首刷
//! （登记变更后各面自刷，跨面不追）。
//!
//! 弹层纪律：清单弹层为非模态面板（web AppPopover 同形态）——点外部 / ESC
//! 关闭由 nsh-base popover 宿主承担，开/关上报 Overlay Suppression 注册表
//! （[`crate::overlay`]，popover_tip/popconfirm 同款 token 通道）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use ledger_infra::db::book_registry::Book;
use nsh_base::{PopoverEvent, button, popover};
use nsh_core::prelude::*;

use crate::app::book_dir;
use crate::boot;
use crate::danger::{DangerConfirmSpec, DangerLevel, DangerOutcome, danger_confirm};
use crate::design::font_size;
use crate::nav;
use crate::nav_model::{self, NavRequest};
use crate::overlay;
use crate::settings;
use crate::views;

// ---------------------------------------------------------------------------
// 词表（web packages/i18n books.json zh-CN 词条原样承载；与设置面账本切换
// 卡共享词条字面；i18n 框架级方案不在本系列，字面承载）。
// ---------------------------------------------------------------------------

/// 触发件兜底文案（清单未就绪 / 无活动本时；web `books.entry.label`）。
const ENTRY_LABEL: &str = "账本";
/// 活动指针指示标注（web `books.panel.current`；设置卡同词）。
const PANEL_CURRENT: &str = "当前";
/// 切换确认对话框词表（设置卡同词；web `books.switchDialog.*` 同义）。
const SWITCH_TITLE: &str = "切换账本";
const SWITCH_DETAIL: &str =
    "将写活动账本指针：重启应用后进入目标账本。未保存的界面状态会丢失，账本数据本身不受影响。";
const SWITCH_CONFIRM: &str = "切换并重启后生效";
const SWITCH_CANCEL: &str = "取消";
/// 切换成功反馈（状态条承载重启提示；本壳无原位重引导，web toast「应用即
/// 将重载」的偏差口径在句内如实表达）。
const SWITCHED_PREFIX: &str = "已切换到「";
const SWITCHED_SUFFIX: &str = "」，重启应用后进入新账本";
const ACTION_FAILED_PREFIX: &str = "操作失败: ";
const SESSION_UNAVAILABLE: &str = "账本注册表尚未就绪，请重启应用后再试";
const WRITE_TASK_FAILED: &str = "内部错误：写入任务异常退出";

/// 账本切换区反应式状态（挂 [`book_switch_area`] 所在组件实例）。
#[derive(Clone, Copy)]
struct SwitcherState {
    /// 清单行集（挂载首刷读注册表最新落盘态）。
    books: Signal<Vec<Book>>,
    /// 活动账本 id。
    active: Signal<Option<String>>,
    /// 登记变更可用性（可变 = 注册表可读且无推迟搬迁窗口）。
    mutable: Signal<bool>,
    /// 回退警示（回退原因随行）。
    fallback_reason: Signal<Option<String>>,
    /// 状态条位（切换成功 / 失败反馈）。
    status: Signal<Option<String>>,
    /// 切换确认待决目标（id, name）。
    switch_pending: Signal<Option<(String, String)>>,
    /// 切换执行中（在途拒绝再次发起）。
    switching: Signal<bool>,
    /// 清单弹层开合（受控）。
    open: Signal<bool>,
}

/// 清单装载（聚合读注册表最新落盘态；与设置面 [`crate::settings_data`] 的
/// `refresh_books` 同一 boot 读缝）。删装载接线 → 清单永远空态 → 红。
fn refresh(owner: Owner, state: SwitcherState) {
    owner.run(move || {
        spawn(async move {
            let launch = book_dir();
            let outcome = blocking(move || match launch {
                Some(launch) => Ok(boot::book_list(&launch)),
                None => Err(ledger_infra::error::AppError::coded(
                    "book.registry-unavailable",
                    SESSION_UNAVAILABLE,
                )),
            })
            .await;
            if let Ok(Ok(info)) = outcome {
                state.books.set(info.books.clone());
                state.active.set(info.active_id.clone());
                state.mutable.set(info.mutable);
                state.fallback_reason.set(info.fallback_reason.clone());
            }
        })
    });
}

/// 侧栏底部账本切换区（web 侧栏底部常驻入口展开态同位）。
pub fn book_switch_area() -> impl IntoElement {
    let owner = Owner::current();
    let state = SwitcherState {
        books: signal(Vec::new()),
        active: signal(None),
        mutable: signal(false),
        fallback_reason: signal(None),
        status: signal(None),
        switch_pending: signal(None),
        switching: signal(false),
        open: signal(false),
    };
    refresh(owner, state);

    // 视图层取色统一经 view_palette（#2146：直读 theme::color 属绕过封
    // 装，双态自适应与兑底语义两处自行维护）。
    let palette = views::view_palette();
    let fg = palette.fg;
    let muted = palette.muted;

    // 活动账本展示名（web `activeBook?.name ?? label` 同语义）。
    let active_name = move || {
        let active = state.active.get();
        state
            .books
            .get()
            .iter()
            .find(|book| Some(book.id.as_str()) == active.as_deref())
            .map(|book| book.name.clone())
            .unwrap_or_else(|| ENTRY_LABEL.to_owned())
    };

    // 切换意图（web requestSwitch 同守卫：当前本不重复切换；登记不可变时
    // 后端必拒，入口直接不触发；切换在途拒绝重入）。
    let switch_request = move |book: Book| {
        let is_active = state.active.get_untracked().as_deref() == Some(book.id.as_str());
        if is_active || !state.mutable.get_untracked() || state.switching.get_untracked() {
            return;
        }
        state
            .switch_pending
            .set(Some((book.id.clone(), book.name.clone())));
    };

    // 切换确认对话框（warning 级轻量确认；确认后写活动指针 + 状态条提示
    // 重启生效——本壳无原位重引导，web `restartAppShortly` 原位重引导的既有
    // 偏差，设置面账本切换卡同款。删切换写链 → 指针不写 → 旅程用例红）。
    let switch_dialog = {
        let (state, owner) = (state, owner);
        let on_outcome = move |outcome: DangerOutcome| {
            let Some((id, _name)) = state.switch_pending.get_untracked() else {
                return;
            };
            state.switch_pending.set(None);
            if outcome != DangerOutcome::Confirmed {
                return;
            }
            state.switching.set(true);
            owner.run(move || {
                spawn(async move {
                    let launch = book_dir();
                    let outcome = blocking(move || match launch {
                        Some(launch) => boot::switch_book(&launch, &id),
                        None => Err(ledger_infra::error::AppError::coded(
                            "book.registry-unavailable",
                            SESSION_UNAVAILABLE,
                        )),
                    })
                    .await;
                    state.switching.set(false);
                    match outcome {
                        Ok(Ok(book)) => {
                            state.status.set(Some(format!(
                                "{SWITCHED_PREFIX}{}{SWITCHED_SUFFIX}",
                                book.name
                            )));
                            refresh(owner, state);
                        }
                        Ok(Err(e)) => state.status.set(Some(format!("{ACTION_FAILED_PREFIX}{e}"))),
                        Err(_) => state
                            .status
                            .set(Some(format!("{ACTION_FAILED_PREFIX}{WRITE_TASK_FAILED}"))),
                    }
                })
            });
        };
        danger_confirm(
            move || state.switch_pending.get().is_some(),
            DangerConfirmSpec {
                level: DangerLevel::Warning,
                title: SWITCH_TITLE.to_owned(),
                lead: None,
                strong_warning: None,
                detail: Some(SWITCH_DETAIL.to_owned()),
                confirm_label: SWITCH_CONFIRM.to_owned(),
                cancel_label: SWITCH_CANCEL.to_owned(),
            },
            on_outcome,
        )
    };

    // 开/关上报（Overlay Suppression 注册表单一通道：追踪受控开合，全路径
    // 覆盖）；组件卸载时 Drop 自动撤销（popover_tip 同形）。
    let token = overlay::OverlayToken::new();
    let open_for_effect = state.open.read_only();
    effect(move || {
        token.report(open_for_effect.get());
    });

    // 触发件（侧栏底部常驻：当前账本名 + 尾箭头；web book-entry 同位）。
    let trigger = view()
        .test_id("sidebar-book-entry")
        .flex_row()
        .items_center()
        .justify_between()
        .px_3()
        .py_2()
        .app_class("border-t border-border rounded-(--radius)")
        .on_press(move |_: &PressEvent| state.open.set(!state.open.get_untracked()))
        .child(
            text(active_name)
                .text_sm()
                .text_color(fg)
                .number_of_lines(1),
        )
        .child(text("›").text_xs().text_color(muted));

    // 清单行（活动行高亮 + 「当前」标注 = 活动指针指示；行按压 = 切换意
    // 图请求，守卫在 [`switch_request` 内挡掉）。key = id（侧栏无登记变更，
    // 行身份随注册表 id）。
    let rows = each(
        move || state.books.get(),
        |book: &Book| book.id.clone(),
        move |item: ReadSignal<Book>, index: ReadSignal<usize>| {
            let is_active = move || state.active.get().as_deref() == Some(item.get().id.as_str());
            let index = index.get();
            let row_id = item.get_untracked().id.clone();
            view()
                .test_id(format!("sidebar-book-switch-{row_id}").as_str())
                .role(nsh_core::AccessibilityRole::Button)
                .optional_label(move || Some(SharedString::from(item.get().name.clone())))
                .flex_row()
                .items_center()
                .justify_between()
                .px_2()
                .py_1()
                .mx_1()
                .app_class("rounded-(--radius) min-w-0")
                .on_press(move |_: &PressEvent| switch_request(item.get_untracked().clone()))
                .style_when(is_active, move |s| s.bg(palette.accent))
                .child(
                    text(move || item.get().name.clone())
                        .text_sm()
                        .text_color(fg)
                        .number_of_lines(1)
                        .test_id(format!("sidebar-book-row-{index}-name").as_str()),
                )
                .child(
                    when(is_active, move || {
                        text(PANEL_CURRENT)
                            .text_xs()
                            .text_color(muted)
                            .test_id(format!("sidebar-book-row-{index}-current").as_str())
                    })
                    .otherwise(view),
                )
        },
    );

    // 跨账本汇总置顶入口（非账本条目、仅导航——不动活动指针；消费 nav 单
    // 点接缝不自建。web summaryAvailable 同口径：注册表可读且回退丶投资
    // 功能未关闭时可用；投资关闭时聚合视图本会改道投资页，入口直接不可
    // 按，ADR-0116 决策 4）。
    let summary_available = move || {
        state.mutable.get()
            && state.fallback_reason.get().is_none()
            && !nav::store().is_closed(nav_model::ViewName::Investments)
    };
    let open_summary = move || {
        nav::open(NavRequest::bare(nav_model::ViewName::CrossBookSummary));
    };

    let panel = view()
        .flex_col()
        .gap_1()
        .w(px(220.0))
        // 汇总置顶入口（清单容器外，web 同位）。
        .child(
            button()
                .app_class(
                    "px-2 py-1 rounded-(--radius) border border-border text-foreground hover:bg-accent disabled:opacity-40",
                )
                .disabled(move || !summary_available())
                .child(
                    text("全部账本（投资汇总）")
                        .text_size(px(font_size::CAPTION))
                        .text_color(fg)
                        .test_id("sidebar-book-summary"),
                )
                .on_press(move |_: &()| open_summary()),
        )
        .child(rows);

    // 状态条位在切换区容器（触发件上方）：确认弹窗点击在弹层外，弹层随
    // 「点外关闭」收起，重启提示不能随弹层消失——重启前持续可见（web toast
    // 的 NSH 条位对应物，#2038 裁决；且重启提示恰宜常驻到重启为止）。
    view()
        .flex_col()
        .child(settings::status_line(
            "sidebar-book-status",
            state.status,
            palette,
        ))
        .child(
            popover()
                .open(move || Some(state.open.get()))
                .on_open_change(move |e: &PopoverEvent| {
                    if let PopoverEvent::OpenChange(shown, _) = *e {
                        state.open.set(shown);
                    }
                })
                .popup_class("bg-background border border-border rounded-(--radius) shadow-md p-2")
                .trigger(trigger)
                .child(panel),
        )
        .child(switch_dialog)
}
