//! 备份最小入口（issue #2038；spec #2031「备份最小入口」纵切）：交易列表
//! 工具区一键备份——文件保存对话框选择目标路径 + 域 backup 直调产出备份
//! 文件 + 成功/失败反馈。
//!
//! 语义对齐旧壳备份卡（设置 → 数据 → 备份，`useBackup`）：
//! - 入口是「另存为…」路径的最小化（旧壳一键备份在有备份目录配置时直写受管
//!   命名；NSH 本票无设置面、无备份目录来源，保存对话框是唯一落点选择方式，
//!   与旧壳未配置目录时的一键备份同形）。默认文件名与旧壳
//!   `defaultBackupFileName` 同格式（`ledger-backup-YYYYMMDD-HHMMSS[-<指纹>]
//!   .db.zip`，本地时间；前缀常量前后端各持一份是 ADR-0016 已接受的取舍，
//!   本壳生成面持第三份同值——全量备份管理面归设置②时再收敛）。
//! - 文件保存对话框是 NSH 壳能力（spec #2031 NSH 版本条款）：上游 0.4.0 已
//!   具备（[`nsh_core::element::pick_save_path`]，Linux 宿主 GTK
//!   `GtkFileDialog`），直用不另建；对话框原生观感归人工对表子集。
//! - 反馈形态：NSH 无 toast（前票裁决），成功/失败经入口下方状态条呈现
//!   （旧壳 message.success/error 的条位对应物，交易删除错误条位先例）；
//!   失败文案 = 码化错误的中文原样（[`crate::boot`] 消息随码走，本壳无错误
//!   模板面）或对话框层失败的码化包装。
//!
//! 覆盖确认守卫与备份执行见本模块下半（[`backup_entry_overlays`] /
//! [`perform_backup`]）。
//!
//! 线程契约：按钮处理器与对话框回答都在引擎 UI 线程；指纹读与备份写是
//! SQLite/文件阻塞 IO，经 `blocking` 在后台线程执行。任务挂视图实例 owner
//! （交易删除先例同形——状态条与对话框随视图存亡，任务存活到备份完成）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::path::PathBuf;

use nsh_base::button;
use nsh_core::element::pick_save_path;
use nsh_core::file_dialog::{FileFilter, SaveDialog};
use nsh_core::prelude::*;

use crate::{boot, danger};

/// 应用版本（备份元数据的应用版本标记，旧壳 `app.package_info().version` 的
/// 对应物：workspace 各成员版本与根包锁步，Cargo.toml 头注）。pub(crate)：
/// 设置②备份管理面（`settings_data`）同一标记的消费面。
pub(crate) const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 手动备份默认文件名的前缀（旧壳 `MANUAL_BACKUP_PREFIX` 同值：命名规则前后
/// 端各持一份并保持一致，ADR-0016）。
const MANUAL_BACKUP_PREFIX: &str = "ledger-backup-";

/// 手动备份默认文件名的后缀（旧壳 `MANAGED_BACKUP_SUFFIX` 同值）。
const MANAGED_BACKUP_SUFFIX: &str = ".db.zip";

/// 手动备份默认文件名（旧壳 `defaultBackupFileName` 同格式的生成面单点）：
/// `ledger-backup-YYYYMMDD-HHMMSS[-<账本指纹>].db.zip`，本地时间；指纹未固化
/// （空库）时退化为无标识旧命名（[`crate::boot::book_fingerprint`] 缺失同
/// 口径，产物归属由内层元数据水印承担）。纯函数，格式闭集单测钉住。
pub(crate) fn default_backup_file_name(
    now: chrono::DateTime<chrono::Local>,
    book_fingerprint: Option<&str>,
) -> String {
    let base = format!("{MANUAL_BACKUP_PREFIX}{}", now.format("%Y%m%d-%H%M%S"));
    match book_fingerprint {
        Some(fingerprint) => format!("{base}-{fingerprint}{MANAGED_BACKUP_SUFFIX}"),
        None => format!("{base}{MANAGED_BACKUP_SUFFIX}"),
    }
}

/// 备份入口状态（信号挂调用方视图实例——组件体内 [`BackupEntryState::new`]
/// 首次调用，交易表单信号束同形）。
#[derive(Clone, Copy)]
pub(crate) struct BackupEntryState {
    /// 备份执行中（单飞：按钮禁用，进行中不可重复发起）。
    backing_up: Signal<bool>,
    /// 覆盖确认待决目标（Some = 确认对话框开；确认/取消后归 None）。
    pending_target: Signal<Option<PathBuf>>,
    /// 备份结果反馈（None = 无呈现；Some = 状态条内容）。
    status: Signal<Option<BackupStatus>>,
}

/// 备份反馈三态（状态条的唯一事实源：进行中 / 成功〔含产物路径〕/ 失败
/// 〔完整呈现文案，含「备份失败: 」前缀〕）。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BackupStatus {
    /// 备份执行中（大库数秒级，忙碌反馈不可省——体验取舍「等待成功优于即刻
    /// 失败」的呈现面）。
    InProgress,
    /// 备份成功（旧壳「备份成功」toast + lastBackup 路径展示的合流形态）。
    Success {
        /// 备份产物完整路径（域 [`ledger_backup::BackupResult::path`]）。
        path: String,
    },
    /// 备份失败（完整呈现文案：码化错误中文原样或对话框层失败包装）。
    Failed { message: String },
}

impl BackupEntryState {
    /// 组件体内构造（信号挂当前组件实例）。
    pub(crate) fn new() -> Self {
        Self {
            backing_up: signal(false),
            pending_target: signal(None),
            status: signal(None),
        }
    }
}

/// 按钮按下 → 备份流程入口（工具区按钮的处理器）：指纹读（后台线程）→
/// 保存对话框（默认名带指纹）→ 按结果分流（取消零副作用 / 失败反馈 /
/// 备份执行）。任务挂 `owner`（视图实例）——对话框回答晚于步骤返回，任务
/// 不得随触发子树卸载被取消。
pub(crate) fn begin_backup(state: BackupEntryState, owner: Owner) {
    // 单飞守卫：备份进行中不重入（按钮禁用是第一道防线，本判定兜底）。
    if state.backing_up.get_untracked() {
        return;
    }
    owner.run(move || {
        spawn(async move {
            // 指纹读（SQLite 只读，阻塞 IO 不占引擎线程）；失败归 None 退化
            // 命名（[boot::book_fingerprint] 容错同口径）。
            let db = crate::app::db_slot().clone();
            let fingerprint =
                blocking(move || db.as_ref().and_then(|db| boot::book_fingerprint(db)))
                    .await
                    .ok()
                    .flatten();
            // 文件保存对话框（NSH 壳能力直用；参数与旧壳另存为同词表）。
            let dialog = SaveDialog::new()
                .file_name(default_backup_file_name(
                    chrono::Local::now(),
                    fingerprint.as_deref(),
                ))
                .title("备份到…")
                .filter(FileFilter::new("开源记账备份", ["zip"]));
            match pick_save_path(dialog).await {
                // 对话框无法打开（Linux 无文件选择 portal 等）：码化包装后
                // 呈现失败（平台错误原文随文案透传，不静默）。
                Err(err) => state.status.set(Some(BackupStatus::Failed {
                    message: format!(
                        "备份失败: {}",
                        ledger_infra::error::AppError::coded(
                            boot::CODE_FILE_DIALOG_FAILED,
                            format!("文件对话框无法打开：{err}"),
                        )
                    ),
                })),
                // 用户取消：零副作用（不开确认、不写文件、不清旧状态——旧壳
                // save 返回空串即返回同语义）。
                Ok(None) => {}
                Ok(Some(target)) => {
                    // 覆盖确认守卫（#2038）：目标已存在 = 破坏性替换，先开
                    // warning 级确认（ADR-0078），确认后才备份；不存在直备。
                    // stat 失败极罕见，按不存在直备（备份本身原子替换，不做
                    // 比确认更重的兜底）。真实桌面 GTK 原生保存对话框自带覆盖
                    // 确认（平台行为），本守卫是应用内第二道，数据安全方向
                    // 偏保守且 headless 可测（守卫接线删除即红钉在
                    // tests/app_headless.rs）。
                    let probe = target.clone();
                    let exists = blocking(move || probe.exists()).await.unwrap_or(false);
                    if exists {
                        state.pending_target.set(Some(target));
                    } else {
                        perform_backup(state, owner, target);
                    }
                }
            }
        });
    });
}

/// 备份执行（对话框选定后的唯一写面，直备与覆盖确认两条路共用）：进行中
/// 状态落位 → 域 backup 直调（后台线程）→ 结果落位（成功含产物路径 /
/// 失败含完整文案）。任务挂 `owner`——覆盖确认关窗即卸载对话框子树，任务
/// 不得随其被取消（交易删除 perform_delete 先例同形）。
pub(crate) fn perform_backup(state: BackupEntryState, owner: Owner, target: PathBuf) {
    state.status.set(Some(BackupStatus::InProgress));
    state.backing_up.set(true);
    owner.run(move || {
        spawn(async move {
            let db = crate::app::db_slot().clone();
            let outcome = blocking(move || match db.as_ref() {
                Some(db) => boot::create_backup(db, &target, APP_VERSION),
                None => Err(ledger_infra::error::AppError::coded(
                    boot::CODE_SESSION_MISSING,
                    "内部错误：账本库会话缺失",
                )),
            })
            .await;
            state.backing_up.set(false);
            match outcome {
                Ok(Ok(result)) => {
                    state
                        .status
                        .set(Some(BackupStatus::Success { path: result.path }));
                }
                Ok(Err(e)) => state.status.set(Some(BackupStatus::Failed {
                    message: format!("备份失败: {e}"),
                })),
                Err(_) => state.status.set(Some(BackupStatus::Failed {
                    message: "备份失败: 内部错误：备份任务异常退出".to_owned(),
                })),
            }
        });
    });
}

/// 覆盖确认对话框（warning 级，ADR-0078 两级形态的 warning 构造面首个壳内
/// 消费面）：目标已存在时开（受控开合读 [`BackupEntryState::pending_target`]），
/// 确认才执行备份；取消/ESC 同为放弃（零写入，旧壳原生对话框取消同语义）。
/// 抑制上报与遮罩/ESC 关闭语义自含于 danger 封装（#2022 AC3 测试钉住封装级
/// 语义，本消费面共享）。
fn backup_confirm_dialog(state: BackupEntryState, owner: Owner) -> impl IntoElement {
    let on_outcome = move |outcome: danger::DangerOutcome| {
        let target = state.pending_target.get_untracked();
        state.pending_target.set(None);
        if outcome == danger::DangerOutcome::Confirmed
            && let Some(target) = target
        {
            perform_backup(state, owner, target);
        }
    };
    danger::danger_confirm(
        move || state.pending_target.get().is_some(),
        danger::DangerConfirmSpec {
            // warning 级：破坏性（旧备份内容被替换）但影响可再操作（备份是
            // 可再生产物，重新备份即可）——判据「错误执行的代价与可回退性」。
            level: danger::DangerLevel::Warning,
            title: "覆盖备份文件".to_owned(),
            lead: Some("目标位置已存在同名文件。".to_owned()),
            strong_warning: Some("覆盖后原文件内容将被替换，不可找回。".to_owned()),
            detail: None,
            confirm_label: "覆盖并备份".to_owned(),
            cancel_label: "取消".to_owned(),
        },
        on_outcome,
    )
}

/// 备份入口的弹层与反馈挂载面（视图树尾部消费，同一状态束）：覆盖确认
/// 对话框 + 反馈状态条。
pub(crate) fn backup_entry_overlays(state: BackupEntryState, owner: Owner) -> impl IntoElement {
    view()
        .flex_col()
        .child(backup_confirm_dialog(state, owner))
        .child(backup_status_line(state))
}

/// 工具区「备份」入口按钮（次级动作形态：描边按钮，「记一笔」主按钮左侧）。
/// 备份进行中禁用（单飞）；test_id 落在按钮内文本上（headless press/enabled
/// 经 id 锚点命中文本、物理命中到内层按钮，清除筛选先例同形）。
pub(crate) fn backup_button(state: BackupEntryState, owner: Owner) -> impl IntoElement {
    let palette = crate::views::view_palette();
    button()
        .disabled(move || state.backing_up.get())
        .accessibility_label("备份账本")
        .app_class(
            "h-8 px-3 rounded-(--radius) border border-border text-foreground hover:bg-accent disabled:opacity-40",
        )
        .child(
            text("备份")
                .text_sm()
                .text_color(palette.fg)
                .test_id("tx-backup"),
        )
        .on_press(move |_: &()| begin_backup(state, owner))
}

/// 备份反馈状态条（入口下方条位，仅在有反馈时呈现）：进行中「正在备份…」/
/// 成功「备份成功」+ 产物路径 / 失败完整文案。test_id 挂条位载体（分支内
/// 断言经文本节点锚点）。
pub(crate) fn backup_status_line(state: BackupEntryState) -> impl IntoElement {
    let status = state.status;
    let palette = crate::views::view_palette();
    let danger_color = palette.danger_fg;
    when(
        move || status.get().is_some(),
        move || {
            view()
                .test_id("tx-backup-status")
                .flex_row()
                .items_center()
                .gap_2()
                .px_4()
                .py_1()
                // 进行中（琥珀句柄归主行内嵌状态——本壳无 warning 令牌，弱化
                // 色呈现；数秒级短暂态不设色别）。
                .child(
                    when(
                        move || matches!(status.get(), Some(BackupStatus::InProgress)),
                        move || {
                            text("正在备份…")
                                .text_xs()
                                .text_color(palette.muted)
                                .test_id("tx-backup-in-progress")
                        },
                    )
                    .otherwise(view),
                )
                // 成功（标签 + 产物路径两节点：断言锚「备份成功」精确命中，
                // 路径供用户核对落点——旧壳 lastBackup 路径展示同语义）。
                .child(
                    when(
                        move || matches!(status.get(), Some(BackupStatus::Success { .. })),
                        move || {
                            view()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(
                                    text("备份成功")
                                        .text_xs()
                                        .text_color(palette.fg)
                                        .test_id("tx-backup-success"),
                                )
                                .child(
                                    text(move || match status.get() {
                                        Some(BackupStatus::Success { path }) => path,
                                        _ => String::new(),
                                    })
                                    .text_xs()
                                    .text_color(palette.muted)
                                    .test_id("tx-backup-success-path"),
                                )
                        },
                    )
                    .otherwise(view),
                )
                // 失败（完整文案，破坏色——删除错误条位先例同形）。
                .child(
                    when(
                        move || matches!(status.get(), Some(BackupStatus::Failed { .. })),
                        move || {
                            text(move || match status.get() {
                                Some(BackupStatus::Failed { message }) => message,
                                _ => String::new(),
                            })
                            .text_xs()
                            .text_color(danger_color)
                            .test_id("tx-backup-failed")
                        },
                    )
                    .otherwise(view),
                )
        },
    )
}
