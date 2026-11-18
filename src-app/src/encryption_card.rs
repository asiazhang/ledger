//! 加密卡（数据页签卡组；旧壳 `EncryptionSettings` 的重生，#2088 写面平价，
//! #2167 自 [`crate::settings_data`] 按职责拆出）：整库加密三形态写面——
//! 明文库呈现开启流（warning 提示 + 主口令两框 + ≥8 位 / 一致守卫 + error
//! 级无后门确认）；密文会话呈现修改主口令（error 级确认，旧壳同分级）/
//! 关闭加密（warning 级确认——破坏性但有兑底，旧壳同分级）与自动解锁平
//! 台注记（Linux-only 在屏静态注记，共用解锁屏单点常量）。三形态转换完成
//! 后原位换连本会话继续可用（旧壳「应用自动重启」的本壳替代口径，无重启
//! 丢写窗口）；口令强度实时条豁免留痕（zxcvbn Rust 依赖的字典体积对单二
//! 进制的增量，守卫闭集 ≥8 位 / 两次一致保留，CHANGELOG 偏差条目）。
//!
//! 密文输入行走数据页签共用呈现件 [`crate::settings_data::pass_input`]；
//! 危险确认弹窗经 [`crate::danger`] 封装族；阻塞 IO（状态探测、整库转换）
//! 一律 `blocking` 后台执行，信号读写都在引擎 UI 线程。弹窗条件段机制选型
//! 布尔有无 → `when`（#2146 定案，模块族注记见 [`crate::settings_data`]）。

use nsh_base::button;
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::danger::{DangerConfirmSpec, DangerLevel, DangerOutcome, danger_confirm};
use crate::design::font_size;
use crate::settings;
use crate::views::view_palette;
use crate::settings_data::pass_input;
use crate::shell_msgs::{
    MSG_ENCRYPTION_STATUS_FAILED_PREFIX, MSG_READ_TASK_FAILED, MSG_WRITE_TASK_FAILED,
};
use crate::write_load::session_missing_err;
use crate::{boot, unlock, views};

const CARD_ENCRYPTION: &str = "加密";
const ENCRYPTION_HINT: &str = "为账本数据库开启文件级加密：磁盘上的库文件与备份不再能被任意工具直接阅读。开启是一次自动完成的整库转换，失败时原库原样保留。";
const ENCRYPTION_WARN_TITLE: &str = "开启前请确认";
const ENCRYPTION_WARN_BODY: &str = "开启后每次启动都需输入主口令解锁；主口令不会被保存、也没有找回手段——遗忘将无法读取数据（可重置为全新空库，旧库保留为密文副本）。转换完成后本会话继续可用（下次启动需输入主口令解锁）。";
const PASS_LABEL: &str = "主口令";
const PASS_PLACEHOLDER: &str = "设置主口令";
const PASS_CONFIRM_LABEL: &str = "确认主口令";
const PASS_CONFIRM_PLACEHOLDER: &str = "再次输入主口令";
const PASS_TOO_SHORT_PREFIX: &str = "主口令至少需要 ";
const PASS_TOO_SHORT_SUFFIX: &str = " 位";
const PASS_MISMATCH: &str = "两次输入不一致";
const ENCRYPT_BUTTON: &str = "开启加密";
const ENCRYPT_CONFIRM_TITLE: &str = "开启加密？";
const ENCRYPT_CONFIRM_LEAD: &str = "开启后，每次启动都需输入主口令。";
const ENCRYPT_CONFIRM_STRONG: &str =
    "主口令不会被保存、也无法找回——一旦忘记，这份账本的数据将无法再打开。";
const ENCRYPT_CONFIRM_REST: &str =
    "将把整个账本库转换为加密库（转换期间请勿关闭应用），完成后本会话继续可用。";
const ENCRYPT_CONFIRM_OK: &str = "继续开启加密";
const ENCRYPT_CONFIRM_CANCEL: &str = "取消";
const ENCRYPT_OK_HOST_HINT: &str =
    "加密已开启（整库转换完成），本会话已切换到加密库；下次启动需输入主口令解锁。";
const ENCRYPT_FAILED_PREFIX: &str = "开启加密失败: ";
const ENCRYPT_RETRY: &str = "重试";
/// 密文会话形态（#2088 平价旧壳 EncryptionSettings：已开启标识 + 修改主
/// 口令 / 关闭加密 / 自动解锁三件写面；转换完成语义 = 原位换连本会话继续
/// 可用，旧壳「应用自动重启」的本壳替代口径）。
const ENCRYPT_ENABLED_LOCAL_TITLE: &str = "已开启加密";
const ENCRYPT_ENABLED_LOCAL_BODY: &str = "当前账本库为加密库，每次启动需输入主口令解锁。";
const CHANGE_HINT: &str = "怀疑泄露或定期轮换时可修改主口令：旧口令验证通过后转入新口令的新库，完成后本会话切换到新口令的加密库（下次启动凭新口令解锁）。";
const CHANGE_OLD_LABEL: &str = "当前主口令";
const CHANGE_OLD_PLACEHOLDER: &str = "输入当前主口令";
const CHANGE_NEW_LABEL: &str = "新主口令";
const CHANGE_NEW_PLACEHOLDER: &str = "设置新主口令";
const CHANGE_CONFIRM_LABEL: &str = "确认新主口令";
const CHANGE_CONFIRM_PLACEHOLDER: &str = "再次输入新主口令";
const CHANGE_UNCHANGED: &str = "新主口令与当前主口令相同";
const CHANGE_BUTTON: &str = "修改主口令";
const CHANGE_CONFIRM_TITLE: &str = "修改主口令？";
const CHANGE_CONFIRM_LEAD: &str = "修改后，每次启动都需凭新主口令解锁。";
const CHANGE_CONFIRM_STRONG: &str =
    "新主口令不会被保存、也无法找回——一旦忘记，这份账本的数据将无法再打开。";
const CHANGE_CONFIRM_REST: &str =
    "将把整个账本库转换为新主口令的加密库（转换期间请勿关闭应用），完成后本会话继续可用。";
const CHANGE_CONFIRM_OK: &str = "继续修改主口令";
const CHANGE_CONFIRM_CANCEL: &str = "取消";
const CHANGE_OK_HOST_HINT: &str = "主口令已修改，本会话已切换（下次启动请使用新口令解锁）。";
const CHANGE_FAILED_PREFIX: &str = "修改主口令失败: ";
const DISABLE_WARN_TITLE: &str = "关闭前请确认";
const DISABLE_WARN_BODY: &str = "关闭后账本库与今后的备份将以明文存放，启动不再需要主口令（既有密文备份不随之改变）。转换需输入当前主口令，完成后本会话切换到明文库。";
const DISABLE_PASS_LABEL: &str = "当前主口令";
const DISABLE_PASS_PLACEHOLDER: &str = "输入当前主口令";
const DISABLE_BUTTON: &str = "关闭加密";
const DISABLE_CONFIRM_TITLE: &str = "关闭加密？";
const DISABLE_CONFIRM_LEAD: &str =
    "将把整个账本库转换回明文库，此后数据以明文存放、启动不再需要主口令。";
const DISABLE_CONFIRM_STRONG: &str =
    "既有密文备份不随之改变，仍需当时的主口令读取；日后可随时重新开启加密。";
const DISABLE_CONFIRM_REST: &str = "转换期间请勿关闭应用，完成后本会话继续可用。";
const DISABLE_CONFIRM_OK: &str = "继续关闭加密";
const DISABLE_CONFIRM_CANCEL: &str = "取消";
const DISABLE_OK_HOST_HINT: &str = "加密已关闭，本会话已切换到明文库。";
const DISABLE_FAILED_PREFIX: &str = "关闭加密失败: ";
// 自动解锁平价注记共用解锁屏单点常量（[`crate::unlock::AUTO_UNLOCK_UNSUPPORTED`]；
// issue #2088 裁决：`passphrase_cache::supported()` 仅 macOS，本壳
// Linux-only——旧壳非 macOS 隐藏选项回退手输的同款平价，在屏静态注记不做写面）。

/// 主口令最小长度（旧壳 `PASSPHRASE_MIN_LENGTH` 同值：≥8 仅界面判定，后端
/// 契约不动；短口令即时红显 + 提交禁用，不拦截键入，ADR-0058 口径）。
const PASSPHRASE_MIN_LENGTH: usize = 8;


// ---------------------------------------------------------------------------
// 加密卡（整库加密三形态写面，#2088 平价交付）：明文库呈现开启流；密文
// 会话呈现修改主口令 / 关闭加密与自动解锁平台注记，开启流仅属明文会话。
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct EncryptionState {
    /// 库文件密文形态（文件即真相；明文会话 = false，密文会话 = true）。
    file_encrypted: Signal<bool>,
    /// 状态读取失败（错误位 + 重试）。
    status_error: Signal<Option<String>>,
    /// 主口令与确认（开启流）。
    pass: Signal<String>,
    confirm: Signal<String>,
    /// 开启加密确认弹窗开着。
    confirm_open: Signal<bool>,
    /// 修改主口令三框（当前 / 新 / 确认新）与确认弹窗。
    change_current: Signal<String>,
    change_new: Signal<String>,
    change_confirm: Signal<String>,
    change_open: Signal<bool>,
    /// 关闭加密当前口令框与确认弹窗。
    disable_current: Signal<String>,
    disable_open: Signal<bool>,
    /// 执行中（单飞，卡级三动作共用）。
    busy: Signal<bool>,
    /// 状态条位（成功 / 失败反馈）。
    status: Signal<Option<String>>,
}

/// 开启表单可提交（两框非空、一致、≥8 位、非执行中——旧壳同守卫闭集）。
fn enable_ready(state: EncryptionState) -> bool {
    // 全部用跟踪读（disabled 绑定的响应式再评估依赖跟踪读；untracked 一次性
    // 读会让按钮停留在初次评估态）。
    let pass = state.pass.get();
    !state.busy.get()
        && !pass.is_empty()
        && pass.chars().count() >= PASSPHRASE_MIN_LENGTH
        && state.confirm.get() == pass
}

/// 修改主口令表单可提交（当前口令非空、新口令 ≥8 位、两次一致、与当前不
/// 同、非执行中——旧壳同守卫闭集加未变守卫）。
fn change_ready(state: EncryptionState) -> bool {
    let current = state.change_current.get();
    let new = state.change_new.get();
    !state.busy.get()
        && !current.is_empty()
        && new.chars().count() >= PASSPHRASE_MIN_LENGTH
        && state.change_confirm.get() == new
        && new != current
}

/// 关闭加密表单可提交（当前口令非空、非执行中——旧壳同守卫闭集）。
fn disable_ready(state: EncryptionState) -> bool {
    !state.busy.get() && !state.disable_current.get().trim().is_empty()
}

/// 加密状态首刷 / 重试（文件头探测阻塞面）。删首刷接线 → 卡面恒「主口令」
/// 开启表单（加密库也不呈现已开启态）→ 密文形态用例红。
fn refresh_encryption_status(owner: Owner, state: EncryptionState) {
    owner.run(move || {
        spawn(async move {
            let outcome = blocking(boot::encryption_status).await;
            match outcome {
                Ok(Ok(file_encrypted)) => {
                    state.file_encrypted.set(file_encrypted);
                    state.status_error.set(None);
                }
                Ok(Err(e)) => state
                    .status_error
                    .set(Some(format!("{MSG_ENCRYPTION_STATUS_FAILED_PREFIX}{e}"))),
                Err(_) => state
                    .status_error
                    .set(Some(MSG_READ_TASK_FAILED.to_owned())),
            }
        })
    });
}

/// 加密卡三动作的共享任务编排（busy 单飞 → 根 owner blocking 任务直调
/// boot 写面，db 槽缺失补 MSG_SESSION_MISSING 码化错误——旧壳命令层同守卫闭
/// 集）：终态三分支收口状态条——成功先走 `on_success` 清表单并按动作续接
/// （刷新状态读取 / 换连后续接），失败携动作前缀，任务异常携统一文案。
fn run_encryption_action(
    owner: Owner,
    state: EncryptionState,
    ok_hint: &'static str,
    failed_prefix: &'static str,
    on_success: impl Fn(Owner, EncryptionState) + Copy + Send + 'static,
    invoke: impl Fn(&ledger_infra::db::DbState) -> ledger_infra::error::Result<()> + Send + 'static,
) {
    state.busy.set(true);
    owner.run(move || {
        spawn(async move {
            let db = crate::app::db_slot().clone();
            let outcome = blocking(move || match db.as_ref() {
                Some(db) => invoke(db),
                None => Err(session_missing_err()),
            })
            .await;
            state.busy.set(false);
            match outcome {
                Ok(Ok(())) => {
                    on_success(owner, state);
                    state.status.set(Some(ok_hint.to_owned()));
                }
                Ok(Err(e)) => state.status.set(Some(format!("{failed_prefix}{e}"))),
                Err(_) => state
                    .status
                    .set(Some(format!("{failed_prefix}{MSG_WRITE_TASK_FAILED}"))),
            }
        })
    });
}

/// 开启加密执行任务（确认弹窗先关再执行——确认弹窗不是重试现场）：成功
/// 后原位换连本会话继续可用（旧壳自动重启的本壳替代口径，见
/// boot::enable_encryption）。
fn run_enable_encryption(owner: Owner, state: EncryptionState) {
    let passphrase = state.pass.get_untracked();
    run_encryption_action(
        owner,
        state,
        ENCRYPT_OK_HOST_HINT,
        ENCRYPT_FAILED_PREFIX,
        |owner, state| {
            state.pass.set(String::new());
            state.confirm.set(String::new());
            refresh_encryption_status(owner, state);
        },
        move |db| boot::enable_encryption(db, &passphrase),
    );
}

/// 修改主口令执行任务（旧壳 `change_encryption_passphrase` 消费面；成功后
/// 凭新口令原位换连本会话继续可用）。
fn run_change_encryption(owner: Owner, state: EncryptionState) {
    let current = state.change_current.get_untracked();
    let new = state.change_new.get_untracked();
    run_encryption_action(
        owner,
        state,
        CHANGE_OK_HOST_HINT,
        CHANGE_FAILED_PREFIX,
        |_, state| {
            state.change_current.set(String::new());
            state.change_new.set(String::new());
            state.change_confirm.set(String::new());
        },
        move |db| boot::change_encryption_passphrase(db, &current, &new),
    );
}

/// 关闭加密执行任务（旧壳 `disable_encryption` 消费面；成功后原位换连到
/// 明文新世界本会话继续可用）。
fn run_disable_encryption(owner: Owner, state: EncryptionState) {
    let current = state.disable_current.get_untracked();
    run_encryption_action(
        owner,
        state,
        DISABLE_OK_HOST_HINT,
        DISABLE_FAILED_PREFIX,
        |owner, state| {
            state.disable_current.set(String::new());
            refresh_encryption_status(owner, state);
        },
        move |db| boot::disable_encryption(db, &current),
    );
}

pub(crate) fn encryption_card(palette: views::ViewPalette) -> View {
    let owner = Owner::current();
    let state = EncryptionState {
        file_encrypted: signal(false),
        status_error: signal(None),
        pass: signal(String::new()),
        confirm: signal(String::new()),
        confirm_open: signal(false),
        change_current: signal(String::new()),
        change_new: signal(String::new()),
        change_confirm: signal(String::new()),
        change_open: signal(false),
        disable_current: signal(String::new()),
        disable_open: signal(false),
        busy: signal(false),
        status: signal(None),
    };

    refresh_encryption_status(owner, state);

    let confirm_dialog = danger_confirm(
        move || state.confirm_open.get(),
        DangerConfirmSpec {
            level: DangerLevel::Error,
            title: ENCRYPT_CONFIRM_TITLE.to_owned(),
            lead: Some(ENCRYPT_CONFIRM_LEAD.to_owned()),
            strong_warning: Some(ENCRYPT_CONFIRM_STRONG.to_owned()),
            detail: Some(ENCRYPT_CONFIRM_REST.to_owned()),
            confirm_label: ENCRYPT_CONFIRM_OK.to_owned(),
            cancel_label: ENCRYPT_CONFIRM_CANCEL.to_owned(),
        },
        move |outcome: DangerOutcome| {
            state.confirm_open.set(false);
            if outcome == DangerOutcome::Confirmed {
                run_enable_encryption(owner, state);
            }
        },
    );

    let card_body = view()
        .flex_col()
        .app_class("gap-2")
        .child(
            text(ENCRYPTION_HINT)
                .text_size(px(font_size::CAPTION))
                .text_color(palette.muted),
        )
        .child(
            when(
                move || state.status_error.get().is_some(),
                move || view().child(encryption_error_view(state)),
            )
            .otherwise(move || {
                view().child(
                    when(
                        move || state.file_encrypted.get(),
                        move || view().child(encrypted_form(state, palette)),
                    )
                    .otherwise(move || view().child(enable_form(state, palette))),
                )
            }),
        )
        .child(settings::status_line("enc-status", state.status, palette));

    settings::card(CARD_ENCRYPTION, palette, vec![card_body]).child(confirm_dialog)
}

/// 状态读取错误位（错误文案 + 重试按钮）。
fn encryption_error_view(state: EncryptionState) -> impl IntoElement {
    let retry_owner = Owner::current();
    view()
        .flex_row()
        .app_class("items-center gap-2")
        .child(
            text(move || state.status_error.get().unwrap_or_default())
                .text_size(px(font_size::CAPTION))
                .text_color(views::view_palette().danger_fg)
                .test_id("enc-status-error"),
        )
        .child(
            button()
                .app_class(
                    "px-2 py-0.5 rounded-sm border border-border text-foreground hover:bg-accent",
                )
                .child(
                    text(ENCRYPT_RETRY)
                        .text_size(px(font_size::CAPTION))
                        .test_id("enc-retry"),
                )
                .on_press(move |_: &()| refresh_encryption_status(retry_owner, state)),
        )
}

/// 密文会话形态（#2088 平价旧壳 EncryptionSettings 已开启视图）：已开启
/// 警示块 + 自动解锁平台注记 + 修改主口令流（error 级确认，旧壳同分级）+
/// 关闭加密流（warning 级确认——破坏性但有兑底：.bak 密文副本保留可再
/// 开启，旧壳同分级）。删任一写面接线 → 对应 headless 旅程红。
fn encrypted_form(state: EncryptionState, palette: views::ViewPalette) -> impl IntoElement {
    let owner = Owner::current();
    // 修改主口令确认（error 级：新口令无后门，与开启加密同级，旧壳同分级）。
    let change_dialog = danger_confirm(
        move || state.change_open.get(),
        DangerConfirmSpec {
            level: DangerLevel::Error,
            title: CHANGE_CONFIRM_TITLE.to_owned(),
            lead: Some(CHANGE_CONFIRM_LEAD.to_owned()),
            strong_warning: Some(CHANGE_CONFIRM_STRONG.to_owned()),
            detail: Some(CHANGE_CONFIRM_REST.to_owned()),
            confirm_label: CHANGE_CONFIRM_OK.to_owned(),
            cancel_label: CHANGE_CONFIRM_CANCEL.to_owned(),
        },
        move |outcome: DangerOutcome| {
            state.change_open.set(false);
            if outcome == DangerOutcome::Confirmed {
                run_change_encryption(owner, state);
            }
        },
    );
    // 关闭加密确认（warning 级：破坏性但有兑底，旧壳同分级）。
    let disable_dialog = danger_confirm(
        move || state.disable_open.get(),
        DangerConfirmSpec {
            level: DangerLevel::Warning,
            title: DISABLE_CONFIRM_TITLE.to_owned(),
            lead: Some(DISABLE_CONFIRM_LEAD.to_owned()),
            strong_warning: Some(DISABLE_CONFIRM_STRONG.to_owned()),
            detail: Some(DISABLE_CONFIRM_REST.to_owned()),
            confirm_label: DISABLE_CONFIRM_OK.to_owned(),
            cancel_label: DISABLE_CONFIRM_CANCEL.to_owned(),
        },
        move |outcome: DangerOutcome| {
            state.disable_open.set(false);
            if outcome == DangerOutcome::Confirmed {
                run_disable_encryption(owner, state);
            }
        },
    );
    view()
        .flex_col()
        .app_class("gap-2")
        .test_id("enc-enabled-form")
        .child(
            view()
                .flex_col()
                .app_class("rounded-sm border border-destructive/30 bg-destructive/10 p-2 gap-1")
                .child(
                    text(ENCRYPT_ENABLED_LOCAL_TITLE)
                        .text_size(px(font_size::CAPTION))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(views::view_palette().fg)
                        .test_id("enc-enabled-title"),
                )
                .child(
                    text(ENCRYPT_ENABLED_LOCAL_BODY)
                        .text_size(px(font_size::CAPTION))
                        .text_color(views::view_palette().fg),
                ),
        )
        .child(
            text(unlock::AUTO_UNLOCK_UNSUPPORTED)
                .text_size(px(font_size::CAPTION))
                .text_color(views::view_palette().muted)
                .test_id("enc-auto-unlock-note"),
        )
        // —— 修改主口令流（旧壳 changeHint + 三框 + 未变/长度/一致守卫） ——
        .child(
            view()
                .flex_col()
                .app_class("gap-2")
                .test_id("enc-change-form")
                .child(
                    text(CHANGE_HINT)
                        .text_size(px(font_size::CAPTION))
                        .text_color(palette.muted),
                )
                .child(pass_input(
                    state.change_current,
                    palette,
                    "enc-change-current",
                    CHANGE_OLD_LABEL,
                    CHANGE_OLD_PLACEHOLDER,
                ))
                .child(pass_input(
                    state.change_new,
                    palette,
                    "enc-change-new",
                    CHANGE_NEW_LABEL,
                    CHANGE_NEW_PLACEHOLDER,
                ))
                .child(when(
                    move || {
                        let new = state.change_new.get();
                        !new.is_empty() && new.chars().count() < PASSPHRASE_MIN_LENGTH
                    },
                    move || {
                        text(format!(
                            "{PASS_TOO_SHORT_PREFIX}{PASSPHRASE_MIN_LENGTH}{PASS_TOO_SHORT_SUFFIX}"
                        ))
                        .text_size(px(font_size::CAPTION))
                        .text_color(view_palette().danger_fg)
                        .test_id("enc-change-too-short")
                    },
                ))
                .child(pass_input(
                    state.change_confirm,
                    palette,
                    "enc-change-confirm",
                    CHANGE_CONFIRM_LABEL,
                    CHANGE_CONFIRM_PLACEHOLDER,
                ))
                .child(when(
                    move || {
                        let confirm = state.change_confirm.get();
                        let new = state.change_new.get();
                        !confirm.is_empty() && confirm != new
                    },
                    move || {
                        text(PASS_MISMATCH)
                            .text_size(px(font_size::CAPTION))
                            .text_color(view_palette().danger_fg)
                            .test_id("enc-change-mismatch")
                    },
                ))
                .child(when(
                    move || {
                        let confirm = state.change_confirm.get();
                        let new = state.change_new.get();
                        !confirm.is_empty()
                            && !new.is_empty()
                            && new == state.change_current.get_untracked()
                            && confirm == new
                    },
                    move || {
                        text(CHANGE_UNCHANGED)
                            .text_size(px(font_size::CAPTION))
                            .text_color(view_palette().danger_fg)
                            .test_id("enc-change-unchanged")
                    },
                ))
                .child(
                    button()
                        .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90 disabled:opacity-40")
                        .disabled(move || !change_ready(state))
                        .child(
                            text(CHANGE_BUTTON)
                                .text_size(px(font_size::SECONDARY))
                                .text_color(rgb(0xffffff))
                                .test_id("enc-change"),
                        )
                        .on_press(move |_: &()| {
                            if change_ready(state) {
                                state.change_open.set(true);
                            }
                        }),
                ),
        )
        // —— 关闭加密流（旧壳 disableWarn + 当前口令框） ——
        .child(
            view()
                .flex_col()
                .app_class("gap-2")
                .test_id("enc-disable-form")
                .child(
                    view()
                        .flex_col()
                        .app_class("rounded-md border border-amber-500/30 bg-amber-500/10 p-2 gap-1")
                        .child(
                            text(DISABLE_WARN_TITLE)
                                .text_size(px(font_size::CAPTION))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(view_palette().danger_fg)
                                .test_id("enc-disable-warn-title"),
                        )
                        .child(
                            text(DISABLE_WARN_BODY)
                                .text_size(px(font_size::CAPTION))
                                .text_color(palette.fg),
                        ),
                )
                .child(pass_input(
                    state.disable_current,
                    palette,
                    "enc-disable-current",
                    DISABLE_PASS_LABEL,
                    DISABLE_PASS_PLACEHOLDER,
                ))
                .child(
                    button()
                        .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90 disabled:opacity-40")
                        .disabled(move || !disable_ready(state))
                        .child(
                            text(DISABLE_BUTTON)
                                .text_size(px(font_size::SECONDARY))
                                .text_color(rgb(0xffffff))
                                .test_id("enc-disable"),
                        )
                        .on_press(move |_: &()| {
                            if disable_ready(state) {
                                state.disable_open.set(true);
                            }
                        }),
                ),
        )
        .child(change_dialog)
        .child(disable_dialog)
}

/// 明文库开启表单（开启前警示 + 主口令两框 + 即时字段错误 + 提交按钮；
/// 删除守卫接线（不经确认直接转换）→ 守卫用例红。
fn enable_form(state: EncryptionState, palette: views::ViewPalette) -> impl IntoElement {
    let request_enable = move || {
        if !enable_ready(state) {
            return;
        }
        state.confirm_open.set(true);
    };
    view()
        .flex_col()
        .app_class("gap-2")
        .test_id("enc-enable-form")
        .child(
            view()
                .flex_col()
                .app_class("rounded-sm border border-destructive/30 bg-destructive/10 p-2 gap-1")
                .child(
                    text(ENCRYPTION_WARN_TITLE)
                        .text_size(px(font_size::CAPTION))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(views::view_palette().danger_fg)
                        .test_id("enc-warn-title"),
                )
                .child(
                    text(ENCRYPTION_WARN_BODY)
                        .text_size(px(font_size::CAPTION))
                        .text_color(palette.fg),
                ),
        )
        .child(pass_input(
            state.pass,
            palette,
            "enc-passphrase",
            PASS_LABEL,
            PASS_PLACEHOLDER,
        ))
        .child(when(
            move || {
                let pass = state.pass.get();
                !pass.is_empty() && pass.chars().count() < PASSPHRASE_MIN_LENGTH
            },
            move || {
                text(format!(
                    "{PASS_TOO_SHORT_PREFIX}{PASSPHRASE_MIN_LENGTH}{PASS_TOO_SHORT_SUFFIX}"
                ))
                .text_size(px(font_size::CAPTION))
                .text_color(views::view_palette().danger_fg)
                .test_id("enc-too-short")
            },
        ))
        .child(pass_input(
            state.confirm,
            palette,
            "enc-confirm",
            PASS_CONFIRM_LABEL,
            PASS_CONFIRM_PLACEHOLDER,
        ))
        .child(when(
            move || {
                let confirm = state.confirm.get();
                !confirm.is_empty() && confirm != state.pass.get_untracked()
            },
            move || {
                text(PASS_MISMATCH)
                    .text_size(px(font_size::CAPTION))
                    .text_color(views::view_palette().danger_fg)
                    .test_id("enc-mismatch")
            },
        ))
        .child(
            button()
                .app_class("px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90 disabled:opacity-40")
                .disabled(move || !enable_ready(state))
                .child(
                    text(ENCRYPT_BUTTON)
                        .text_size(px(font_size::SECONDARY))
                        .text_color(palette.primary_fg)
                        .test_id("enc-enable"),
                )
                .on_press(move |_: &()| request_enable()),
        )
}

