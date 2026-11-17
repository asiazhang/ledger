//! 密文库锁定视图（解锁面板 + 忘口令重置 + 从备份文件恢复，issue #2088 /
//! #2131；旧壳 `UnlockScreen.vue` 的 NSH 对应面，旧壳 `unlock.*` zh-CN 词条
//! 原样承载）。
//!
//! 呈现面只放解锁 / 状态 / 逃生门动作（旧壳 `LOCKED_ALLOWED_COMMANDS` 白名单
//! 语义的本壳对应闭集）：主口令输入 + 解锁、错误行、忘记口令重置入口、从
//! 备份文件恢复入口（#2131 收口 #2088 挂账残留：恢复入口与忘口令重置并列
//! 常驻双逃生门，#603 平价；复用备份域恢复通道与 RestoreSafetyBackup 语义，
//! 词条口径不变——文件选择器 → 元数据校验 → 模式探测 → 确认弹窗〔跨模式
//! 警告 + 密文备份主口令，上下文口令自动试开〕→ [`crate::boot::restore_locked_book`]
//! 恢复通道最小命令面，其余业务读写仍被锁定现场结构性拦截——会话不存在
//!（[`crate::app::db_slot`] 恒空）、业务视图未挂载，白名单不变量成立）。
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

use std::path::Path;

use nsh_base::{button, input};
use nsh_core::element::FontWeight;
use nsh_core::file_dialog::{FileFilter, OpenDialog};
use nsh_core::prelude::*;

use crate::app::StartupView;
use crate::danger::{DangerConfirmSpec, DangerLevel, DangerOutcome, danger_confirm};
use crate::design::font_size;
use crate::restore_confirm::{
    RestoreIds, RestoreIntent, app_restore_confirm_modal, restore_show_pass,
};
use crate::settings_data;
use crate::settings_data::{READ_TASK_FAILED, WRITE_TASK_FAILED};
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
/// 从备份文件恢复入口与选择器标题（旧壳 `unlock.restore` /
/// `unlock.restorePickTitle` zh-CN 词条原样；#2131）。
const RESTORE_ENTRY: &str = "从备份文件恢复…";
const RESTORE_PICK_TITLE: &str = "选择备份文件";
/// 自动解锁平价注记（Linux 平台不支持，见模块文档）。设置卡加密卡的
/// 同一注记共用本常量（单点，避免双点措辞漂移）。
pub(crate) const AUTO_UNLOCK_UNSUPPORTED: &str =
    "自动解锁：本机（Linux 桌面）不支持——主口令的本机缓存仅 macOS 可用，启动解锁请手动输入主口令。";

/// 恢复确认弹窗 headless 锚点（锁定屏前缀，#2131；收敛后经共享封装
/// [`RestoreIds`] 消费，#2144）。
const RESTORE_IDS: RestoreIds = RestoreIds {
    path: "unlock-restore-path",
    cross_mode_title: "unlock-restore-cross-mode-title",
    cross_mode: "unlock-restore-cross-mode",
    passphrase: "unlock-restore-passphrase",
    error: "unlock-restore-error",
    cancel: "unlock-restore-cancel",
    confirm: "unlock-restore-confirm",
};

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
    /// 恢复意图（Some = 确认弹窗开；#2131，意图类型收敛共享
    /// [`RestoreIntent`]，#2144）。
    restore_intent: Signal<Option<RestoreIntent>>,
    /// 恢复弹窗口令输入（上下文口令自动试开失败后就地重输位）。
    restore_pass: Signal<String>,
    /// 口令框就地显出（试开失败 / 后端报需口令后置真）。
    restore_pass_revealed: Signal<bool>,
    /// 恢复弹窗内错误位（失败不关弹窗，就地重试）。
    restore_error: Signal<Option<String>>,
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
        restore_intent: signal(None),
        restore_pass: signal(String::new()),
        restore_pass_revealed: signal(false),
        restore_error: signal(None),
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

    /// 恢复入口任务（旧壳 UnlockScreen pickRestore / 设置卡 pick_restore_task
    /// 的本屏对应面，#2131）：文件选择 → 元数据校验 → 当前模式探测 → 意图
    /// 落位（确认弹窗开）。校验失败即中止（弹窗不开，宁可不弹窗——破坏性
    /// 操作前的安全面同设置卡口径），经解锁屏错误行呈现。两步探测都是纯
    /// 文件面（恢复通道最小命令面，无会话依赖——锁定现场可用）。上下文口
    /// 令在选定备份那一刻快照（解锁框非空即随意图携带，空白不携带）。
    fn run_restore_pick(&self, owner: Owner) {
        if self.busy.get_untracked() {
            return; // 解锁 / 重置 / 恢复在途防重入（busy 三入口共用单飞）
        }
        let surface = *self;
        owner.run(move || {
            spawn(async move {
                let picked = match pick_paths(
                    OpenDialog::files()
                        .multiple(false)
                        .filter(FileFilter::new(settings_data::FILTER_NAME, ["zip", "db"]))
                        .title(RESTORE_PICK_TITLE),
                )
                .await
                {
                    Ok(Some(paths)) => paths,
                    _ => return,
                };
                let Some(path) = picked.into_iter().next() else {
                    return;
                };
                let target: String = String::from(path.to_string_lossy().as_ref());
                let probe_target = target.clone();
                let meta = match blocking(move || boot::backup_meta(Path::new(&probe_target))).await
                {
                    Ok(Ok(meta)) => meta,
                    Ok(Err(e)) => {
                        surface.fail(format!("{}{e}", settings_data::MSG_META_FAILED_PREFIX));
                        return;
                    }
                    Err(_) => {
                        surface.fail(format!(
                            "{}{READ_TASK_FAILED}",
                            settings_data::MSG_META_FAILED_PREFIX
                        ));
                        return;
                    }
                };
                let current = match blocking(boot::encryption_status).await {
                    Ok(Ok(flag)) => flag,
                    Ok(Err(e)) => {
                        surface.fail(format!(
                            "{}{e}",
                            settings_data::MSG_ENCRYPTION_STATUS_FAILED_PREFIX
                        ));
                        return;
                    }
                    Err(_) => {
                        surface.fail(format!(
                            "{}{READ_TASK_FAILED}",
                            settings_data::MSG_ENCRYPTION_STATUS_FAILED_PREFIX
                        ));
                        return;
                    }
                };
                let context = {
                    let pass = surface.pass.get_untracked();
                    (!pass.trim().is_empty()).then_some(pass)
                };
                surface.restore_pass.set(String::new());
                surface.restore_pass_revealed.set(false);
                surface.restore_error.set(None);
                surface.restore_intent.set(Some(RestoreIntent {
                    path: target,
                    backup_encrypted: meta.encrypted,
                    current_encrypted: current,
                    context_pass: context,
                }));
            })
        });
    }

    /// 恢复确认任务（弹窗提交；旧壳 RestoreConfirmModal confirm / 设置卡
    /// confirm_restore_task 的本屏对应面，#2131）：口令框显出时用输入值，
    /// 否则用上下文口令自动试开；失败不关弹窗（口令错误就地重试，解锁同
    /// 语义），密文备份或后端报需口令时就地显出口令框；成功关弹窗并经业务
    /// 可用起点进会话（恢复出的会话已按模式探测结果成对打开，
    /// [`boot::restore_locked_book`]：明文库直达主界面，密文库凭恢复时已验
    /// 证的口令解锁——旧壳「恢复后重启由启动探测接管」的本壳原位形态）。
    fn run_restore_confirm(&self, owner: Owner) {
        let Some(intent) = self.restore_intent.get_untracked() else {
            return;
        };
        if self.busy.get_untracked() {
            return; // 解锁 / 重置 / 恢复在途防重入
        }
        let typed = self.restore_pass.get_untracked().clone();
        let show_pass = restore_show_pass(&intent, self.restore_pass_revealed);
        if show_pass && typed.is_empty() {
            return; // 口令框显出时须非空（按钮 disabled 同守卫）
        }
        let attempt = if show_pass {
            typed
        } else {
            intent.context_pass.clone().unwrap_or_default()
        };
        let passphrase = (!attempt.is_empty()).then_some(attempt);
        self.busy.set(true);
        self.restore_error.set(None);
        let surface = *self;
        owner.run(move || {
            spawn(async move {
                let restore_target = intent.path.clone();
                let outcome = blocking(move || {
                    boot::restore_locked_book(Path::new(&restore_target), passphrase.as_deref())
                })
                .await;
                match outcome {
                    Ok(Ok(db)) => {
                        surface.restore_intent.set(None);
                        surface.restore_pass.set(String::new());
                        surface.busy.set(false);
                        surface.enter_ready(db);
                    }
                    Ok(Err(e)) => {
                        let text = e.to_string();
                        surface.restore_error.set(Some(format!(
                            "{}{text}",
                            settings_data::RESTORE_FAIL_PREFIX
                        )));
                        surface.busy.set(false);
                        // 后端探测实际密文而元数据未标记（异常产物）时，按需显
                        // 出口令输入——以后端探测为准（旧壳
                        // passphraseRevealedByError 同位）。
                        if intent.backup_encrypted || text.contains("backup.passphrase-required") {
                            surface.restore_pass_revealed.set(true);
                        }
                    }
                    Err(_) => {
                        surface.restore_error.set(Some(format!(
                            "{}{WRITE_TASK_FAILED}",
                            settings_data::RESTORE_FAIL_PREFIX
                        )));
                        surface.busy.set(false);
                    }
                }
            })
        });
    }
}

/// 锁定面板（App 启动形态互斥呈现的 Locked 分支；`surface` 与主行/次行前
/// 景色由 App 组件体传入）。删本分支的挂载接线 → 锁定场景呈现缺参/空白两
/// 行态、解锁入口缺席 → headless 解锁旅程红；删恢复入口 / 恢复确认弹窗
/// 接线（`unlock-restore-open` / `restore_modal`）→ 恢复旅程红（#2131）。
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
    // 从备份文件恢复确认弹窗（#2131；#2144 收敛共享封装
    // [`crate::restore_confirm::app_restore_confirm_modal`]，锁定屏与设置
    // 卡两侧消费同一成员；关闭路径 = 取消 / ESC / ✕——执行中不关；成功后
    // 由任务关并进会话）。
    let restore_modal = app_restore_confirm_modal(
        surface.restore_intent,
        surface.restore_pass,
        surface.restore_pass_revealed,
        surface.busy,
        surface.restore_error,
        &RESTORE_IDS,
        move |_: &()| surface.run_restore_confirm(owner),
    );
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
                )
                .child(
                    // 从备份文件恢复常驻入口（#2131，旧壳 UnlockScreen 同位
                    // 双逃生门并列；文件选择 → 探测 → 确认弹窗，见
                    // restore_modal）。
                    button()
                        .app_class("px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent")
                        .child(
                            text(RESTORE_ENTRY)
                                .text_size(px(font_size::SECONDARY))
                                .text_color(fg)
                                .test_id("unlock-restore-open"),
                        )
                        .on_press(move |_: &()| surface.run_restore_pick(owner)),
                ),
        )
        .child(
            text(AUTO_UNLOCK_UNSUPPORTED)
                .text_size(px(font_size::CAPTION))
                .text_color(muted)
                .test_id("unlock-auto-unlock-note"),
        )
        .child(reset_dialog)
        .child(restore_modal)
}
