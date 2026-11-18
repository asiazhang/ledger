//! 恢复确认弹窗封装（issue #2144；`App*` 封装族成员，modal.rs 同层——旧壳
//! `RestoreConfirmModal` 的 NSH 对应件，锁定屏 [`crate::unlock`] 与设置卡
//! 备份卡 [`crate::settings_data`] 两侧消费同一封装，收敛 #2131 时期的
//! 两侧同构复制）。
//!
//! 形态闭集（两侧并集语义，零业务语义）：备份文件路径行 + 恢复后果强警示
//! 块 + 跨模式警告块 + 密文备份主口令输入 + 弹窗内错误位 + 取消/确认按钮
//! 行。词表单点复用设置卡常量（[`crate::settings_data`]，措辞零漂移，
//! #2131）。口令显出判定收敛两侧并集（[`restore_show_pass`]）：密文备份
//! 且无上下文口令、或已被要求显出——设置卡无上下文口令面（恒 `None`），
//! 退化即其原判定；锁定屏上下文口令自动试开时不显出（#2131）。条件段
//! （口令框 / 错误位）按 #2146 选型标准用 `when`（布尔有无：条件翻转重建
//! 分支 + reactive 绑定逐次求值）；按钮禁用面用响应式闭包（prop 绑定逐次
//! 求值）。
//!
//! 提交回调归调用方（两侧恢复通道不同：锁定屏
//! [`crate::boot::restore_locked_book`] 原位续接 vs 设置卡
//! [`crate::boot::restore_backup`] 会话内替换），意图 / 口令 / 显出 / busy
//! / 错误五信号归调用方持有、任务按各自通道写回；busy 单飞语义随调用方
//!（锁定屏三入口共用单 busy，设置卡恢复独立 busy）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{button, input};
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::design::font_size;
use crate::modal::app_modal;
use crate::settings_data;
use crate::views;

/// 恢复确认意图（旧壳 `RestoreIntent` 的本壳对应面；#2131 两侧意图类型的
/// 收敛面）：弹窗开着 = 意图在场。`context_pass` 是上下文口令快照（锁定屏
/// 解锁框非空即随意图携带，密文备份确认先自动试开、失败就地显出口令框
/// 重输；设置卡无此面，恒 `None`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RestoreIntent {
    /// 所选备份文件路径（弹窗内回显）。
    pub path: String,
    /// 备份是否密文（元数据加密标记，驱动口令框与跨模式警告）。
    pub backup_encrypted: bool,
    /// 当前库是否密文（文件头探测，驱动跨模式警告）。
    pub current_encrypted: bool,
    /// 上下文口令快照（None = 不携带，密文备份直接弹口令框）。
    pub context_pass: Option<String>,
}

/// 口令框显出判定（旧壳 RestoreConfirmModal showPassphrase 同位，#2131 /
/// #2144 两侧并集单点）：密文备份且无上下文口令时确认前显出；上下文试开
/// 失败或后端报需口令后就地显出（revealed 由调用方任务置真）。设置卡恒无
/// 上下文口令，判定退化为「密文备份或已显出」——与其原判定同值。
pub(crate) fn restore_show_pass(intent: &RestoreIntent, revealed: Signal<bool>) -> bool {
    (intent.backup_encrypted && intent.context_pass.is_none()) || revealed.get()
}

/// 弹窗 headless 锚点闭集（两侧同构面各自的 test_id 前缀：锁定屏
/// `unlock-restore-*`、设置卡 `data-restore-*`；封装不拼串，锚点字面量
/// 由调用方常量承载）。
pub(crate) struct RestoreIds {
    /// 备份文件路径行。
    pub path: &'static str,
    /// 跨模式警告标题。
    pub cross_mode_title: &'static str,
    /// 跨模式警告正文。
    pub cross_mode: &'static str,
    /// 主口令输入。
    pub passphrase: &'static str,
    /// 弹窗内错误位。
    pub error: &'static str,
    /// 取消按钮。
    pub cancel: &'static str,
    /// 确认按钮。
    pub confirm: &'static str,
}

/// 恢复确认五信号束（#2164 类型化：意图 / 口令 / 显出 / busy / 错误五信号
/// 归调用方持有、任务按各自通道写回——原七位置参中五信号整束传递，收敛为
/// 命名字段消错序；`Copy` 随 [`Signal`]，闭包各自捕获副本，语义零变化）。
#[derive(Clone, Copy, Debug)]
pub(crate) struct RestoreSignals {
    /// 弹窗意图（开着 = 呈现；取消 / ✕ / ESC 关闭路径由此置空）。
    pub intent: Signal<Option<RestoreIntent>>,
    /// 主口令输入。
    pub pass: Signal<String>,
    /// 口令框显出（判定单点 [`restore_show_pass`]）。
    pub revealed: Signal<bool>,
    /// 执行中（单飞守卫语义随调用方：锁定屏三入口共用、设置卡独立）。
    pub busy: Signal<bool>,
    /// 弹窗内错误位。
    pub error: Signal<Option<String>>,
}

/// 恢复确认弹窗（意图开着 = 呈现；关闭路径 = 取消 / ESC / ✕——执行中
/// 不关；成功后由调用方任务关并走各自续接）。
pub(crate) fn app_restore_confirm_modal(
    signals: RestoreSignals,
    ids: &'static RestoreIds,
    on_confirm: impl Fn(&()) + Copy + 'static,
) -> impl IntoElement {
    app_modal(
        move || signals.intent.get().is_some(),
        || settings_data::RESTORE_MODAL_TITLE.to_owned(),
        move || {
            if !signals.busy.get_untracked() {
                signals.intent.set(None);
            }
        },
        move || restore_body(signals, ids, on_confirm),
    )
}

/// 弹窗内容（两侧同构面的单一封装族成员；交叉模式块随意图固定，静态段
/// 承载；口令框与错误位是「弹窗开着期间就地变化」的条件段，`when` 承载）。
fn restore_body(
    signals: RestoreSignals,
    ids: &'static RestoreIds,
    on_confirm: impl Fn(&()) + Copy + 'static,
) -> View {
    let RestoreSignals {
        intent,
        pass,
        revealed,
        busy,
        error,
    } = signals;
    let palette = views::view_palette();
    let Some(opened) = intent.get() else {
        return view();
    };
    let cross_mode = (opened.backup_encrypted != opened.current_encrypted).then_some({
        if opened.backup_encrypted {
            (
                settings_data::RESTORE_CROSS_MODE_TITLE,
                settings_data::RESTORE_TO_ENCRYPTED_WARN,
            )
        } else {
            (
                settings_data::RESTORE_CROSS_MODE_TITLE,
                settings_data::RESTORE_TO_PLAINTEXT_WARN,
            )
        }
    });
    view()
        .flex_col()
        .app_class("gap-2")
        .child(
            text(format!(
                "{}{}",
                settings_data::RESTORE_BACKUP_FILE_LABEL,
                opened.path
            ))
                .text_size(px(font_size::CAPTION))
                .text_color(palette.fg)
                .test_id(ids.path),
        )
        .child(
            view()
                .flex_col()
                .app_class("rounded-sm border border-destructive/30 bg-destructive/10 p-2 gap-1")
                .child(
                    text(settings_data::RESTORE_CONFIRM_STRONG)
                        .text_size(px(font_size::CAPTION))
                        .font_weight(FontWeight::BOLD)
                        .text_color(palette.danger_fg),
                ),
        )
        .children(cross_mode.map(|(title, warning)| {
            view()
                .flex_col()
                .app_class("rounded-sm border border-destructive/30 bg-destructive/10 p-2 gap-1")
                .child(
                    text(title)
                        .text_size(px(font_size::CAPTION))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.fg)
                        .test_id(ids.cross_mode_title),
                )
                .child(
                    text(warning)
                        .text_size(px(font_size::CAPTION))
                        .text_color(palette.fg)
                        .test_id(ids.cross_mode),
                )
        }))
        // 口令框：显出判定单点 [`restore_show_pass`]（两侧并集）；显出后
        // 口令输入就位重输（masked，同解锁框形态）。
        .child(when(
            move || intent.get().is_some_and(|i| restore_show_pass(&i, revealed)),
            move || {
                view()
                    .flex_col()
                    .app_class("gap-1")
                    .child(
                        text(settings_data::RESTORE_PASS_LABEL)
                            .text_size(px(font_size::CAPTION))
                            .text_color(palette.fg),
                    )
                    .child(
                        input()
                            .test_id(ids.passphrase)
                            .value(move || pass.get())
                            .masked(true)
                            .placeholder(settings_data::RESTORE_PASS_PLACEHOLDER)
                            .optional_label(settings_data::RESTORE_PASS_LABEL)
                            .text_color(palette.fg)
                            .w(px(260.0))
                            .app_class(
                                "bg-background rounded-(--radius) border border-border px-2 py-1 placeholder:text-muted-foreground",
                            )
                            .on_change(move |value: &str| pass.set(value.to_owned())),
                    )
            },
        ))
        .child(when(
            move || error.get().is_some(),
            move || {
                text(move || error.get().unwrap_or_default())
                    .text_size(px(font_size::CAPTION))
                    .text_color(views::view_palette().danger_fg)
                    .test_id(ids.error)
            },
        ))
        .child(
            view()
                .flex_row()
                .justify_end()
                .app_class("gap-2")
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-(--radius) border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(move || busy.get())
                        .child(
                            text(settings_data::RESTORE_CANCEL)
                                .text_size(px(font_size::SECONDARY))
                                .test_id(ids.cancel),
                        )
                        .on_press(move |_: &()| {
                            if !busy.get_untracked() {
                                intent.set(None);
                            }
                        }),
                )
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-(--radius) bg-destructive hover:opacity-90 disabled:opacity-40",
                        )
                        // 口令框显出时须非空（按钮 disabled 同守卫）；显出
                        // 判定与条件段同一单点 [`restore_show_pass`]。
                        .disabled(move || {
                            busy.get() || {
                                intent.get().is_some_and(|i| restore_show_pass(&i, revealed))
                                    && pass.get().is_empty()
                            }
                        })
                        .child(
                            text(settings_data::RESTORE_CONFIRM_ACTION)
                                .text_size(px(font_size::SECONDARY))
                                // 低频专用配对前景保留 theme 单点直读（#2153
                                // 边界：destructive-foreground 不入 palette
                                // 闭集，正当边界见 ViewPalette 文档）。
                                .text_color(crate::theme::color("destructive-foreground"))
                                .test_id(ids.confirm),
                        )
                        .on_press(on_confirm),
                ),
        )
}
