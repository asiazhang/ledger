//! 密文库锁定视图（解锁面板 + 忘口令重置，issue #2088；旧壳
//! `UnlockScreen.vue` 的 NSH 对应面，旧壳 `unlock.*` zh-CN 词条原样承载）。
//!
//! 呈现面只放解锁 / 状态 / 恢复动作（旧壳 `LOCKED_ALLOWED_COMMANDS` 白名单
//! 语义的本壳对应闭集）：主口令输入 + 解锁、错误行、忘记口令重置入口。会话
//! 不存在的锁定现场无业务读写面（业务视图未挂载、[`crate::app::db_slot`]
//! 恒空），白名单不变量结构性成立；「从备份文件恢复」入口不在本屏（豁免
//! 留痕：恢复通道 = 忘口令重置进明文会话 → 设置卡恢复备份文件，与 NSH
//! 失败态无恢复入口的既有形态一致，#2088 裁决）。
//!
//! 自动解锁平价（#2088 裁决）：[`ledger_infra::db::passphrase_cache::supported`]
//! 仅 macOS——本壳 Linux-only（ADR-0145），解锁屏不做自动解锁尝试（旧壳
//! 非 macOS 前端隐藏「记住」选项回退手输的同款平价），在屏注记说明。
//!
//! **owner 纪律**（视图装载竞态先例，app.rs 引导确认同款）：信号组与任务
//! 的 owner 挂根组件体（[`LockedSurface`] 在 App 装载期单次构造，任务经根
//! owner `run` + `spawn`）——解锁成功翻转 [`StartupView::Ready`] 时本面板
//! 子树卸载，直呼 `spawn` 会把任务挂到子树 owner 上随卸载被取消。面板视图
//! 是纯投影（信号驱动的 `when`/`text` 闭包），分支构建期零信号创建。
//!
//! 解锁 / 重置成功后的续接走业务可用起点单点
//! [`crate::app::enter_business_surface`]（旧壳 `resume_business_surface`
//! 同位：日志接管 → 会话槽换入 → 门开门 → 备份宿主同点补拉起 → HTTP 壳 /
//! 同步触发 / 行情车道），不重走启动竖井（#2088 裁决：重启后密文库引导会
//! 再落锁定态，原位续接是唯一正确形态）；HTTP 泵接线在 await 回引擎线程后
//! 执行（`spawn_pump` 需引擎线程，与启动竖井调用方同形）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{button, input};
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::app::StartupView;
use crate::danger::{DangerConfirmSpec, DangerLevel, DangerOutcome, danger_confirm};
use crate::design::font_size;
use crate::{app, boot, http_host, views};

// ---------------------------------------------------------------------------
// 词表（旧壳 unlock.* zh-CN 词条原样；标题/副题在 app.rs headline_text /
// secondary_text 同源承载，i18n 框架级方案另票收敛）。
// ---------------------------------------------------------------------------

const UNLOCK_PASS_PLACEHOLDER: &str = "主口令";
const UNLOCK_BUTTON: &str = "解锁";
const UNLOCK_TASK_FAILED: &str = "内部错误：解锁任务异常退出";
const FORGOT_ENTRY: &str = "忘记口令？";
const RESET_CONFIRM_TITLE: &str = "重置为全新空库？";
const RESET_CONFIRM_STRONG: &str =
    "主口令没有任何后门或找回手段：遗忘后，当前账本数据在本应用中不可恢复。";
const RESET_CONFIRM_DETAIL: &str = "继续将把账本重置为全新的明文空库，应用随即进入全新账本（重置后可再次开启加密）。当前账本会保留为密文副本（无口令不可读）——日后想起口令时，数据仍可救回。";
const RESET_CONFIRM_OK: &str = "确认重置";
const RESET_CONFIRM_CANCEL: &str = "取消";
const RESET_TASK_FAILED: &str = "内部错误：重置任务异常退出";
/// 自动解锁平价注记（Linux 平台不支持，见模块文档）。设置卡加密卡的
/// 同一注记共用本常量（单点，避免双点措辞漂移）。
pub(crate) const AUTO_UNLOCK_UNSUPPORTED: &str =
    "自动解锁：本机（Linux 桌面）不支持——主口令的本机缓存仅 macOS 可用，启动解锁请手动输入主口令。";

/// 锁定面板的信号组（根组件体装载期单次构造，见模块文档 owner 纪律；
/// `Copy` 供任务闭包与 `danger_confirm` 的 `Copy` 约束捕获——信号字段全部
/// `Copy`）。
#[derive(Clone, Copy)]
pub(crate) struct LockedSurface {
    startup: Signal<StartupView>,
    pass: Signal<String>,
    busy: Signal<bool>,
    error: Signal<Option<String>>,
    reset_open: Signal<bool>,
}

/// 构造锁定面板信号组（App 组件体内调用一次；任务经 `Owner::current()` =
/// 根组件 owner 挂根）。
pub(crate) fn locked_surface(startup: Signal<StartupView>) -> LockedSurface {
    LockedSurface {
        startup,
        pass: signal(String::new()),
        busy: signal(false),
        error: signal(None),
        reset_open: signal(false),
    }
}

impl LockedSurface {
    /// 解锁 / 重置成功后的业务可用起点续接（blocking 任务内调用）：编排见
    /// 模块文档；HTTP 泵在 await 回引擎线程后的调用方段接线（本函数随任务
    /// 闭包回引擎线程执行，`spawn_pump` 直接调用同形）。
    fn enter_ready(&self, db: ledger_infra::db::DbState) {
        let view = app::enter_business_surface(db, ledger_sync_engine::TriggerTimings::default());
        if view == StartupView::Ready {
            http_host::spawn_pump();
        }
        self.startup.set(view);
    }

    fn fail(&self, message: String) {
        self.busy.set(false);
        self.error.set(Some(message));
    }

    /// 解锁任务（阻塞 IO 在 blocking 任务，完成后回引擎线程翻转呈现；旧壳
    /// `unlock_encryption` → `do_unlock` + `resume_business_surface` 编排的
    /// 本壳对应面）。
    fn run_unlock(&self, owner: Owner) {
        if self.busy.get_untracked() {
            return; // 在途防重入
        }
        let passphrase = self.pass.get_untracked();
        if passphrase.is_empty() {
            return; // 空口令不发起（按钮 disabled 同守卫）
        }
        self.busy.set(true);
        self.error.set(None);
        let surface = *self;
        owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || boot::unlock_encrypted_book(&passphrase)).await;
                match outcome {
                    Ok(Ok(db)) => surface.enter_ready(db),
                    Ok(Err(e)) => surface.fail(e.to_string()),
                    Err(_) => surface.fail(UNLOCK_TASK_FAILED.to_owned()),
                }
            });
        });
    }

    /// 忘口令重置任务（旧壳 `reset_after_forgotten_passphrase` + resume 的
    /// 本壳对应面；重置成功直接进全新明文会话，无需重启——旧壳同语义）。
    fn run_reset(&self, owner: Owner) {
        if self.busy.get_untracked() {
            return; // 解锁在途 / 重置在途防重入
        }
        self.busy.set(true);
        self.error.set(None);
        let surface = *self;
        owner.run(move || {
            spawn(async move {
                let outcome = blocking(boot::reset_locked_book).await;
                match outcome {
                    Ok(Ok(db)) => surface.enter_ready(db),
                    Ok(Err(e)) => surface.fail(e.to_string()),
                    Err(_) => surface.fail(RESET_TASK_FAILED.to_owned()),
                }
            });
        });
    }
}

/// 锁定面板（App 启动形态互斥呈现的 Locked 分支；`surface` 与主行/次行前
/// 景色由 App 组件体传入）。删本分支的挂载接线 → 锁定场景呈现缺参/空白两
/// 行态、解锁入口缺席 → headless 解锁旅程红。
pub(crate) fn locked_panel(
    surface: LockedSurface,
    fg: AdaptiveColor,
    muted: AdaptiveColor,
) -> impl IntoElement {
    let owner = Owner::current();
    // 忘口令重置确认（旧壳 unlock.reset* 词表；error 级——不可逆（清空当前
    // 数据面），ADR-0078 分级基准与旧壳 UnlockScreen 同级）。
    let reset_surface = surface;
    let reset_dialog = danger_confirm(
        move || surface.reset_open.get(),
        DangerConfirmSpec {
            level: DangerLevel::Error,
            title: RESET_CONFIRM_TITLE.to_owned(),
            lead: None,
            strong_warning: Some(RESET_CONFIRM_STRONG.to_owned()),
            detail: Some(RESET_CONFIRM_DETAIL.to_owned()),
            confirm_label: RESET_CONFIRM_OK.to_owned(),
            cancel_label: RESET_CONFIRM_CANCEL.to_owned(),
        },
        move |outcome: DangerOutcome| {
            reset_surface.reset_open.set(false);
            if outcome == DangerOutcome::Confirmed {
                reset_surface.run_reset(owner);
            }
        },
    );
    let unlock_surface = surface;
    view()
        .flex_1()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .test_id("locked-panel")
        .child(
            // 主行/副行与 [`crate::app::headline_text`] / [`secondary_text`]
            // 的 Locked 臂同源（lib 文案断言同源单点，首启引导面板同形）。
            text(app::headline_text(&StartupView::Locked))
                .text_size(px(font_size::TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg)
                .test_id("startup-headline"),
        )
        .child(
            text(app::secondary_text(&StartupView::Locked))
                .text_size(px(font_size::CAPTION))
                .text_color(muted),
        )
        .child(
            input()
                .test_id("unlock-passphrase")
                .value(move || surface.pass.get())
                .masked(true)
                .placeholder(UNLOCK_PASS_PLACEHOLDER)
                .optional_label(UNLOCK_PASS_PLACEHOLDER)
                .text_color(fg)
                .w(px(260.0))
                .app_class(
                    "bg-background rounded-md border border-border px-2 py-1 placeholder:text-muted-foreground",
                )
                .on_change(move |value: &str| surface.pass.set(value.to_owned())),
        )
        .child(
            // 解锁失败行（「口令错误或文件损坏」合并口径随 infra 错误文案
            // 原样呈现，可无限重试——旧壳 errorMessage 同位）。
            when(
                move || surface.error.get().is_some(),
                move || {
                    text(move || surface.error.get().unwrap_or_default())
                        .text_size(px(font_size::CAPTION))
                        // 破坏色单一呈现令牌（ViewPalette::danger_fg 成员，
                        // 与 settings_data / backup_entry 同口径）。
                        .text_color(views::view_palette().danger_fg)
                        .test_id("unlock-error")
                },
            )
            .otherwise(view),
        )
        .child(
            view()
                .flex_row()
                .gap_2()
                .child(
                    button()
                        .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90 disabled:opacity-40")
                        .child(
                            text(UNLOCK_BUTTON)
                                .text_size(px(font_size::SECONDARY))
                                .text_color(rgb(0xffffff))
                                .test_id("unlock-confirm"),
                        )
                        // 空口令 / 在途不可按（disabled 契约投影，headless
                        // press_target 判定同款）。
                        .disabled(move || {
                            surface.busy.get()
                                || surface.pass.get().trim().is_empty()
                        })
                        .on_press(move |_: &()| unlock_surface.run_unlock(owner)),
                )
                .child(
                    // 忘记口令常驻入口（旧壳 UnlockScreen 同位；无后门后果
                    // 说明 + error 级二次确认，见 reset_dialog）。
                    button()
                        .app_class("px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent")
                        .child(
                            text(FORGOT_ENTRY)
                                .text_size(px(font_size::SECONDARY))
                                .text_color(fg)
                                .test_id("unlock-forgot"),
                        )
                        .on_press(move |_: &()| surface.reset_open.set(true)),
                ),
        )
        .child(
            text(AUTO_UNLOCK_UNSUPPORTED)
                .text_size(px(font_size::CAPTION))
                .text_color(muted)
                .test_id("unlock-auto-unlock-note"),
        )
        .child(reset_dialog)
}
