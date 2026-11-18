//! 备份管理卡组（设置②数据页签；旧壳 `BackupSettings` 的重生，#2054 落
//! 位，#2167 自 [`crate::settings_data`] 按职责拆出）：动作卡（一键备份〔配
//! 置了备份目录时直写受管默认名，否则同另存为对话框〕+ 另存为 + 最近备份
//! 回显与复制）、备份目录卡（目录选择 / 清除 + 保留上限输入）、自动备份卡
//! （开关 + 上次时刻 + 连续失败警示 + 本壳调度宿主缺位注记）、备份文件列表
//! 卡（按当前账本指纹分域的行集 + 手动刷新 + 立即清理〔warning 级数量确
//! 认〕+ 行级「在访达中显示」）与恢复卡（红色警示块 + 恢复确认弹窗——恢复
//! 安全备份 + 替换当前全部数据，issue #572 跨模式警告 + 密文备份主口令，
//! 口令错误不关弹窗就地重试）。
//!
//! 备份偏好镜像（备份目录 / 保留上限）：NSH 自建平台存储 [`Storage`]（键值
//! 形态同旧壳 `backup_dir` / `backup_max_count` 的 JSON 形状，不与旧壳共享、
//! 不进 SQLite、不动 schema——外观卡同款契约）；[`pref_backup_dir`] 同时是
//! [`crate::backup_host`] 设备偏好推送的镜像读面。阻塞 IO（备份写、恢复、
//! 探测）一律 `blocking` 后台执行；信号读写都在引擎 UI 线程。恢复确认弹窗
//! 词表（`RESTORE_*`）与备份文件过滤名（[`FILTER_NAME`]）是
//! [`crate::restore_confirm`] / [`crate::unlock`] 的跨模块消费面（词面零变
//! 化，#2167 拆分只挪地址）。
//!
//! 弹窗条件段机制选型（#2146 定案，行为零变化）：布尔有无 / 分支翻转 →
//! `when`，整段重造 → `dynamic`（备份列表行集段同款）。测试豁免（ADR-0060）：
//! 本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::path::{Path, PathBuf};

use nsh_base::{button, input};
use nsh_core::element::FontWeight;
use nsh_core::file_dialog::{FileFilter, OpenDialog, SaveDialog};
use nsh_core::prelude::*;
use nsh_core::storage::Storage;

use crate::danger::{DangerConfirmSpec, DangerLevel, DangerOutcome, danger_confirm};
use crate::design::font_size;
use crate::restore_confirm::{
    RestoreIds, RestoreIntent, RestoreSignals, app_restore_confirm_modal,
};
use crate::settings_data::{format_backup_size, format_backup_time};
use crate::shell_msgs::{
    MSG_ENCRYPTION_STATUS_FAILED_PREFIX, MSG_META_FAILED_PREFIX, MSG_READ_TASK_FAILED,
    MSG_RESTORE_FAIL_PREFIX, MSG_WRITE_TASK_FAILED,
};
use crate::views;
use crate::write_load::session_missing_err;
use crate::{backup_entry, backup_host, boot, settings, signals};

// ---------------------------------------------------------------------------
// 词表（旧壳 settings.data.* / books.* zh-CN 词条原样；i18n 框架级方案不在
// 本系列，字面承载）。
// ---------------------------------------------------------------------------

const CARD_BACKUP: &str = "备份";
const CARD_BACKUP_DIR: &str = "备份目录";
const CARD_BACKUP_AUTO: &str = "自动备份";
const CARD_BACKUP_LIST: &str = "备份文件列表";
const CARD_BACKUP_RESTORE: &str = "恢复";
const BACKUP_HINT: &str = "将当前账本（账户、交易、预算、投资持仓、定时交易等全部数据）打包备份为一个文件。备份不含主题、默认币种等界面偏好。";
const BACKUP_ONCE: &str = "一键备份";
const BACKUP_AS: &str = "另存为…";
const BACKUP_LAST_LABEL: &str = "最近备份：";
const BUTTON_COPY_PATH: &str = "复制路径";
const MSG_COPIED: &str = "已复制完整路径";
const DIR_HINT: &str = "配置默认备份目录后，“一键备份”将直接写入该目录，无需每次选择位置。";
const DIR_UNSET: &str = "未设置（备份时将弹出位置选择）";
const DIR_CHANGE: &str = "更改目录";
const DIR_CHOOSE: &str = "选择目录";
const DIR_CLEAR: &str = "清除";
const DIR_PICK_TITLE: &str = "选择备份目录";
const MSG_DIR_SET: &str = "备份目录已设置";
const KEEP_LIMIT_LABEL: &str = "备份保留上限";
const KEEP_LIMIT_SUFFIX: &str =
    "个（1–100）。超出上限的最旧备份会在备份后自动清理（按当前账本独立计算）。";
const AUTO_SWITCH_LABEL: &str = "开启后每 24 小时检查一次，数据有变化时自动备份";
const AUTO_LAST_LABEL: &str = "上次自动备份：";
const AUTO_LAST_NEVER: &str = "从未";
const AUTO_NEED_DIR: &str = "设置备份目录后自动备份生效";
const AUTO_FAIL_ALERT_PREFIX: &str = "自动备份已连续失败 ";
const AUTO_FAIL_ALERT_SUFFIX: &str = " 次，数据保护网可能未生效。请检查备份目录是否可用、磁盘空间是否充足；备份成功后本提示自动消失。";
const LIST_HINT: &str = "仅显示当前账本的备份；其他账本的产物互不可见，各自参与滚动清理。";
const BUTTON_REFRESH: &str = "刷新";
const BUTTON_PRUNE_NOW: &str = "立即清理";
const LIST_EMPTY_WITH_DIR: &str = "备份目录中暂无备份文件";
const LIST_EMPTY_NO_DIR: &str = "未设置备份目录";
const LIST_COUNT_PREFIX: &str = "当前共 ";
const LIST_COUNT_MIDDLE: &str = " 个备份，上限 ";
const LIST_COUNT_SUFFIX: &str = " 个。";
const SOURCE_AUTO: &str = "自动";
const SOURCE_MANUAL: &str = "手动";
const DEVICE_UNKNOWN: &str = "未知来源";
const ENCRYPTED_TAG: &str = "密文";
const REVEAL_LABEL: &str = "在访达中显示";
const RESTORE_HINT_STRONG: &str = "替换当前全部数据";
const RESTORE_HINT_BEFORE: &str = "从备份文件恢复将";
const RESTORE_HINT_AFTER: &str =
    "（破坏性操作）。恢复前系统会自动备份当前数据到应用数据目录；恢复成功后应用将自动重启。";
const RESTORE_BUTTON: &str = "从备份恢复…";
const RESTORE_PICK_TITLE: &str = "从备份恢复…";
pub(crate) const RESTORE_MODAL_TITLE: &str = "确认恢复";
pub(crate) const RESTORE_CONFIRM_STRONG: &str =
    "恢复将替换当前全部数据，且不可撤销。系统会在恢复前自动备份当前数据到应用数据目录。";
pub(crate) const RESTORE_CONFIRM_ACTION: &str = "确认恢复";
pub(crate) const RESTORE_CANCEL: &str = "取消";
pub(crate) const RESTORE_PASS_LABEL: &str = "备份的主口令";
pub(crate) const RESTORE_PASS_PLACEHOLDER: &str = "输入该备份的主口令";
pub(crate) const RESTORE_BACKUP_FILE_LABEL: &str = "备份文件：";
pub(crate) const RESTORE_CROSS_MODE_TITLE: &str = "加密模式不一致";
pub(crate) const RESTORE_TO_PLAINTEXT_WARN: &str =
    "当前库为加密库：恢复这份明文备份后，数据将变为未加密，应用重启后以明文库启动。";
pub(crate) const RESTORE_TO_ENCRYPTED_WARN: &str =
    "这份备份为密文备份：恢复后此库将变为加密库，应用重启后需凭该备份的主口令解锁。";
const MSG_RESTORE_OK: &str = "恢复成功，请重启应用进入新库";

const MSG_LIST_FAILED_PREFIX: &str = "读取备份列表失败: ";
const MSG_AUTO_STATE_FAILED_PREFIX: &str = "读取自动备份状态失败: ";
const MSG_AUTO_ON: &str = "自动备份已开启";
const MSG_AUTO_OFF: &str = "自动备份已关闭";
const MSG_AUTO_TOGGLE_FAILED_PREFIX: &str = "更新自动备份开关失败: ";
const MSG_PRUNE_PARTIAL_PREFIX: &str = "清理完成：已删除 ";
const MSG_PRUNE_PARTIAL_MIDDLE: &str = " 个，";
const MSG_PRUNE_PARTIAL_SUFFIX: &str = " 个失败";
const MSG_PRUNE_DONE_PREFIX: &str = "已清理 ";
const MSG_PRUNE_DONE_SUFFIX: &str = " 个旧备份";
const MSG_PRUNE_FAILED_PREFIX: &str = "清理备份失败: ";
const MSG_PRUNE_NOT_NEEDED: &str = "无需清理：备份数量未超过上限";
const OVERWRITE_TITLE: &str = "覆盖备份文件";
const OVERWRITE_LEAD: &str = "目标位置已存在同名文件。";
const OVERWRITE_STRONG: &str = "覆盖后原文件内容将被替换，不可找回。";
const OVERWRITE_OK: &str = "覆盖并备份";
const OVERWRITE_CANCEL: &str = "取消";
const BACKUP_DIALOG_TITLE: &str = "备份到…";
const MSG_BACKUP_IN_PROGRESS: &str = "正在备份…";
const MSG_BACKUP_FAILED_PREFIX: &str = "备份失败: ";
const PRUNE_CONFIRM_TITLE: &str = "确认清理";
const PRUNE_CONFIRM_STRONG_PREFIX: &str = "将删除最旧的 ";
const PRUNE_CONFIRM_STRONG_SUFFIX: &str = " 个备份，删除后不可恢复。";

/// 恢复确认弹窗 headless 锚点（设置卡前缀；#2144 收敛后经共享封装
/// [`RestoreIds`] 消费）。
const DATA_RESTORE_IDS: RestoreIds = RestoreIds {
    path: "data-restore-path",
    cross_mode_title: "data-restore-cross-mode-title",
    cross_mode: "data-restore-cross-mode",
    passphrase: "data-restore-passphrase",
    error: "data-restore-error",
    cancel: "data-restore-cancel",
    confirm: "data-restore-confirm",
};
const PRUNE_CONFIRM_DETAIL: &str =
    "删除的是备份产物，账本数据本身不受影响；自动备份开启时会按保留上限继续生成新备份。";
const PRUNE_CONFIRM_OK: &str = "确认清理";
const PRUNE_CONFIRM_CANCEL: &str = "取消";
pub(crate) const FILTER_NAME: &str = "开源记账备份";

/// 保留上限默认值（旧壳 store `backup_max_count` 出厂值同值；上限面 1–100）。
const KEEP_LIMIT_DEFAULT: u32 = 30;
const KEEP_LIMIT_MIN: u32 = 1;
const KEEP_LIMIT_MAX: u32 = 100;

/// 偏好存储键（旧壳 store `backup_dir` / `backup_max_count` 同键字面；NSH
/// 自建平台存储，不与旧壳共享——外观卡同款契约）。
const BACKUP_DIR_KEY: &str = "backup_dir";
const BACKUP_MAX_COUNT_KEY: &str = "backup_max_count";

// ---------------------------------------------------------------------------
// 备份行投影（行文本在装配期定格：两行文本 = 文件名 / 元件拼接；渲染期零
// 格式化）。
// ---------------------------------------------------------------------------

/// 来源展示文案（旧壳 sourceText 同口径：auto → 自动；manual / pre-update /
/// 旧数据缺字段 → 手动）。
pub(crate) fn source_text(kind: ledger_backup::BackupKind) -> &'static str {
    match kind {
        ledger_backup::BackupKind::Auto => SOURCE_AUTO,
        _ => SOURCE_MANUAL,
    }
}

/// 来源设备展示文案（旧壳 sourceDeviceText 同口径：缺水印 = 未知来源）。
pub(crate) fn device_text(source_device_id: Option<&str>) -> String {
    match source_device_id {
        Some(id) => id.to_owned(),
        None => DEVICE_UNKNOWN.to_owned(),
    }
}

/// 备份行视图（行文本在装配期定格：两行文本 = 文件名 / 元件拼接）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BackupRowView {
    /// 文件名（行首）。
    pub file_name: String,
    /// 元件拼接（来源 / 设备 / 密文标记 / 大小 / 时刻，旧壳列序同序）。
    pub meta: String,
    /// 产物完整路径（「在访达中显示」动作面）。
    pub path: String,
    /// 来源设备标识（立即清理的「本机桶」判定消费面）。
    pub source_device_id: Option<String>,
}

/// 备份行集 → 视图投影（同一笔产物一段装配，无第二份口径）。
pub(crate) fn backup_row_views(rows: &[ledger_backup::BackupFileInfo]) -> Vec<BackupRowView> {
    rows.iter()
        .map(|row| {
            let lock = if row.encrypted {
                format!("[{ENCRYPTED_TAG}] ")
            } else {
                Default::default()
            };
            BackupRowView {
                file_name: row.file_name.clone(),
                meta: format!(
                    "来源 {} · 设备 {} · {}{} · {}",
                    source_text(row.kind),
                    device_text(row.source_device_id.as_deref()),
                    lock,
                    format_backup_size(row.size_bytes),
                    format_backup_time(&row.created_at),
                ),
                path: row.path.clone(),
                source_device_id: row.source_device_id.clone(),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 备份偏好读写（NSH 自建平台存储；键值形态同旧壳 store JSON 序列化形状）。
// ---------------------------------------------------------------------------

/// 读备份目录偏好（缺失 / 损坏 → 空串 = 未设置）。
pub(crate) fn pref_backup_dir() -> String {
    Storage::get_item(BACKUP_DIR_KEY)
        .and_then(|raw| {
            serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
        })
        .unwrap_or_default()
}

/// 写备份目录偏好（写失败告警不静默、会话内仍生效——导航界面状态同取舍）。
pub(crate) fn store_backup_dir(dir: &str) {
    if let Err(e) = Storage::set_item(BACKUP_DIR_KEY, serde_json::json!(dir).to_string()) {
        tracing::warn!(key = BACKUP_DIR_KEY, error = %e, "备份目录偏好写入存储失败");
    }
}

/// 读保留上限偏好（缺失 / 损坏 / 闭集外 → 出厂值）。
pub(crate) fn pref_keep_limit() -> u32 {
    Storage::get_item(BACKUP_MAX_COUNT_KEY)
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.as_u64().map(|n| n as u32))
        .map(|n| n.clamp(KEEP_LIMIT_MIN, KEEP_LIMIT_MAX))
        .unwrap_or(KEEP_LIMIT_DEFAULT)
}

/// 写保留上限偏好（写入前钳闭集 1–100）。
pub(crate) fn store_keep_limit(n: u32) {
    let n = n.clamp(KEEP_LIMIT_MIN, KEEP_LIMIT_MAX);
    if let Err(e) = Storage::set_item(BACKUP_MAX_COUNT_KEY, serde_json::json!(n).to_string()) {
        tracing::warn!(key = BACKUP_MAX_COUNT_KEY, error = %e, "备份保留上限写入存储失败");
    }
}

/// 复制路径按钮（旧壳 copy-text 通道：写剪贴板 + 卡内状态条位反馈）。
fn copy_button(
    id: &'static str,
    value_of: impl Fn() -> Option<String> + Copy + 'static,
    on_copied: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    button()
        .app_class("px-2 py-0.5 rounded-sm border border-border text-foreground hover:bg-accent")
        .child(
            text(BUTTON_COPY_PATH)
                .text_size(px(font_size::CAPTION))
                .text_color(views::view_palette().fg)
                .test_id(id),
        )
        .on_press(move |_: &()| {
            if let Some(value) = value_of() {
                set_clipboard(value);
                on_copied();
            }
        })
}


// ---------------------------------------------------------------------------
// 备份管理（五卡共用同一信号组；任务挂视图实例 owner）。
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct BackupUiState {
    /// 备份执行中（动作两路 + 恢复之外的单飞）。
    backing_up: Signal<bool>,
    /// 覆盖确认待决目标（确认对话框开着时 Some）。
    pending_overwrite: Signal<Option<PathBuf>>,
    /// 最近备份展示文案（「路径（大小）」，旧壳 lastBackup 同形）。
    last_backup: Signal<Option<String>>,
    /// 最近备份完整路径（复制通道消费面）。
    last_backup_path: Signal<Option<String>>,
    /// 动作卡状态条位。
    status: Signal<Option<String>>,
    /// 备份行集（装配投影）。
    rows: Signal<Vec<BackupRowView>>,
    /// 行集读取失败反馈（列表卡内条位）。
    rows_error: Signal<Option<String>>,
    /// 行集装载中。
    rows_loading: Signal<bool>,
    /// 清理执行中。
    pruning: Signal<bool>,
    /// 清理确认待决数量。
    prune_pending: Signal<Option<usize>>,
    /// 备份目录偏好镜像。
    backup_dir: Signal<String>,
    /// 保留上限偏好镜像。
    keep: Signal<u32>,
    /// 保留上限输入缓冲。
    keep_text: Signal<String>,
    /// 自动备份开关。
    auto_enabled: Signal<bool>,
    /// 自动备份状态读取失败反馈。
    auto_error: Signal<Option<String>>,
    /// 上次自动备份时刻。
    auto_last: Signal<Option<String>>,
    /// 连续失败提示态（阈值判定归域单点）。
    auto_failing: Signal<bool>,
    /// 连续失败次数（提示文案插值）。
    auto_failures: Signal<u32>,
    /// 本机设备标识（桶判定基准）。
    device: Signal<Option<String>>,
    /// 恢复意图（弹窗开着 = Some；类型收敛共享 [`RestoreIntent`]，#2144）。
    restore_intent: Signal<Option<RestoreIntent>>,
    /// 恢复口令输入。
    restore_pass: Signal<String>,
    /// 恢复口令框按需显出（后端探测报需口令后解锁）。
    restore_pass_revealed: Signal<bool>,
    /// 恢复执行中。
    restore_busy: Signal<bool>,
    /// 恢复弹窗内错误位（口令错误可就地重试）。
    restore_error: Signal<Option<String>>,
}

/// 备份目录是否已配置（偏好镜像非空 = 已设置）。
fn dir_set(state: BackupUiState) -> bool {
    !state.backup_dir.get_untracked().trim().is_empty()
}

/// 行集装载任务：失败诚实呈现（不静默假空态；旧壳 listFailed 同口径）。
/// 删装载接线 → 列表永远空态 → 红。
fn refresh_rows_task(owner: Owner, state: BackupUiState) {
    owner.run(move || {
        spawn(async move {
            state.rows_error.set(None);
            state.rows_loading.set(true);
            let dir = state.backup_dir.get_untracked();
            let db = crate::app::db_slot().clone();
            let outcome = blocking(move || match db.as_ref() {
                Some(db) if !dir.trim().is_empty() => boot::backup_rows(db, Path::new(dir.trim())),
                Some(_) => Ok(Vec::new()),
                None => Err(session_missing_err()),
            })
            .await;
            state.rows_loading.set(false);
            match outcome {
                Ok(Ok(rows)) => {
                    state.rows.set(backup_row_views(&rows));
                    state.rows_error.set(None);
                }
                Ok(Err(e)) => {
                    state.rows.set(Vec::new());
                    state
                        .rows_error
                        .set(Some(format!("{MSG_LIST_FAILED_PREFIX}{e}")));
                }
                Err(_) => {
                    state.rows.set(Vec::new());
                    state.rows_error.set(Some(MSG_READ_TASK_FAILED.to_owned()));
                }
            }
        })
    });
}

/// 自动备份状态装载（阈值判定在域单点 [`boot::auto_backup_failure_alerting`]，
/// 本壳不复刻）。删此装载接线 → 状态卡恒出厂值 → 红。
fn refresh_auto_backup_state(owner: Owner, state: BackupUiState) {
    owner.run(move || {
        spawn(async move {
            let db = crate::app::db_slot().clone();
            let outcome = blocking(move || match db.as_ref() {
                Some(db) => boot::auto_backup_state(db),
                None => Err(session_missing_err()),
            })
            .await;
            match outcome {
                Ok(Ok(result)) => {
                    state.auto_enabled.set(result.enabled);
                    state.auto_last.set(result.last_backup_at);
                    state.auto_failures.set(result.consecutive_failures);
                    state.auto_failing.set(boot::auto_backup_failure_alerting(
                        result.consecutive_failures,
                    ));
                    state.auto_error.set(None);
                }
                Ok(Err(e)) => state
                    .auto_error
                    .set(Some(format!("{MSG_AUTO_STATE_FAILED_PREFIX}{e}"))),
                Err(_) => state.auto_error.set(Some(MSG_READ_TASK_FAILED.to_owned())),
            }
        })
    });
}

/// 备份执行（对话框选定 / 一键直写的共用执行面）：进行中落位 → 域 backup
/// 直调 → 结果落位（成功向 = 最近备份回显 + 受管产物滚动清理 + 列表刷新 /
/// 非受管产物列表刷新 / 失败完整文案）。删执行接线 → 全旅程用例红。
fn run_backup_task(owner: Owner, state: BackupUiState, target: PathBuf) {
    state.status.set(Some(MSG_BACKUP_IN_PROGRESS.to_owned()));
    state.backing_up.set(true);
    owner.run(move || {
        spawn(async move {
            let db = crate::app::db_slot().clone();
            let outcome = blocking(move || match db.as_ref() {
                Some(db) => boot::create_backup(db, &target, backup_entry::APP_VERSION),
                None => Err(session_missing_err()),
            })
            .await;
            state.backing_up.set(false);
            match outcome {
                Ok(Ok(result)) => {
                    state.last_backup.set(Some(format!(
                        "{}（{}）",
                        result.path,
                        format_backup_size(result.size_bytes)
                    )));
                    state.last_backup_path.set(Some(result.path.clone()));
                    state.status.set(None);
                    // 受管产物（落在配置的备份目录内，域受管改道可能落设备子目录，
                    // 前缀判定一同覆盖）→ 滚动清理到保留上限（旧壳一键 / 另存为
                    // 同规则）；非受管 → 列表刷新。
                    let dir = state.backup_dir.get_untracked();
                    let managed = !dir.trim().is_empty() && result.path.starts_with(dir.trim());
                    if managed {
                        prune_to_limit_task(owner, state);
                    } else {
                        refresh_rows_task(owner, state);
                    }
                }
                Ok(Err(e)) => state
                    .status
                    .set(Some(format!("{MSG_BACKUP_FAILED_PREFIX}{e}"))),
                Err(_) => state.status.set(Some(format!(
                    "{MSG_BACKUP_FAILED_PREFIX}{MSG_WRITE_TASK_FAILED}"
                ))),
            }
        })
    });
}

/// 滚动清理执行（保留上限 → [`boot::prune_backups`] 单点）+ 行集与自动备
/// 份状态刷新（旧壳 `ledger:backups-changed` 信号的壳内对应物）。
fn prune_to_limit_task(owner: Owner, state: BackupUiState) {
    state.pruning.set(true);
    owner.run(move || spawn(async move {
        let dir = state.backup_dir.get_untracked();
        let keep = state.keep.get_untracked();
        let db = crate::app::db_slot().clone();
        let outcome = blocking(move || match db.as_ref() {
            Some(db) if !dir.trim().is_empty() => {
                boot::prune_backups(db, Path::new(dir.trim()), keep as usize)
            }
            Some(_) => Ok(ledger_backup::PruneResult {
                kept: 0,
                deleted: Vec::new(),
                failed: Vec::new(),
            }),
            None => Err(session_missing_err()),
        })
        .await;
        state.pruning.set(false);
        match outcome {
            Ok(Ok(result)) => {
                if !result.failed.is_empty() {
                    state.status.set(Some(format!(
                        "{MSG_PRUNE_PARTIAL_PREFIX}{}{MSG_PRUNE_PARTIAL_MIDDLE}{}{MSG_PRUNE_PARTIAL_SUFFIX}",
                        result.deleted.len(),
                        result.failed.len(),
                    )));
                } else if !result.deleted.is_empty() {
                    state.status.set(Some(format!(
                        "{MSG_PRUNE_DONE_PREFIX}{}{MSG_PRUNE_DONE_SUFFIX}",
                        result.deleted.len()
                    )));
                }
            }
            Ok(Err(e)) => state.status.set(Some(format!("{MSG_PRUNE_FAILED_PREFIX}{e}"))),
            Err(_) => state.status.set(Some(format!("{MSG_PRUNE_FAILED_PREFIX}{MSG_WRITE_TASK_FAILED}"))),
        }
        refresh_rows_task(owner, state);
        refresh_auto_backup_state(owner, state);
    }));
}

/// 自动备份开关动作（失败由状态重载回读旧态）。
fn apply_auto_toggle(owner: Owner, state: BackupUiState, enabled: bool) {
    owner.run(move || {
        spawn(async move {
            let db = crate::app::db_slot().clone();
            let outcome = blocking(move || match db.as_ref() {
                Some(db) => boot::set_auto_backup_enabled(db, enabled),
                None => Err(session_missing_err()),
            })
            .await;
            match outcome {
                Ok(Ok(())) => {
                    state.auto_enabled.set(enabled);
                    state.status.set(Some(if enabled {
                        MSG_AUTO_ON.to_owned()
                    } else {
                        MSG_AUTO_OFF.to_owned()
                    }));
                }
                Ok(Err(e)) => {
                    state
                        .status
                        .set(Some(format!("{MSG_AUTO_TOGGLE_FAILED_PREFIX}{e}")));
                    refresh_auto_backup_state(owner, state);
                }
                Err(_) => {
                    state.status.set(Some(format!(
                        "{MSG_AUTO_TOGGLE_FAILED_PREFIX}{MSG_WRITE_TASK_FAILED}"
                    )));
                    refresh_auto_backup_state(owner, state);
                }
            }
        })
    });
}

/// 共用流程件：指纹读 → 保存对话框（#2038 同词表）→ 覆盖守卫 → 执行。
fn begin_backup_as(owner: Owner, state: BackupUiState) {
    if state.backing_up.get_untracked() {
        return; // 单飞兜底
    }
    owner.run(move || {
        spawn(async move {
            let db = crate::app::db_slot().clone();
            let fingerprint =
                blocking(move || db.as_ref().and_then(|db| boot::book_fingerprint(db)))
                    .await
                    .ok()
                    .flatten();
            let dialog = SaveDialog::new()
                .file_name(backup_entry::default_backup_file_name(
                    chrono::Local::now(),
                    fingerprint.as_deref(),
                ))
                .title(BACKUP_DIALOG_TITLE)
                .filter(FileFilter::new(FILTER_NAME, ["zip"]));
            match pick_save_path(dialog).await {
                Err(err) => state.status.set(Some(format!(
                    "{MSG_BACKUP_FAILED_PREFIX}{}",
                    ledger_infra::error::AppError::coded(
                        boot::CODE_FILE_DIALOG_FAILED,
                        format!("文件对话框无法打开：{err}"),
                    )
                ))),
                Ok(None) => {}
                Ok(Some(target)) => {
                    let probe = target.clone();
                    let exists = blocking(move || probe.exists()).await.unwrap_or(false);
                    if exists {
                        state.pending_overwrite.set(Some(target));
                    } else {
                        run_backup_task(owner, state, target);
                    }
                }
            }
        })
    });
}

/// 一键备份入口（目录已配置的直写路径）：受管默认名（指纹标识段），目标
/// 存在即覆盖守卫；未配置与另存为同路。
fn begin_backup_once(owner: Owner, state: BackupUiState) {
    if state.backing_up.get_untracked() {
        return; // 单飞兜底
    }
    let dir = state.backup_dir.get_untracked();
    if dir.trim().is_empty() {
        begin_backup_as(owner, state);
        return;
    }
    owner.run(move || {
        spawn(async move {
            let db = crate::app::db_slot().clone();
            let fingerprint =
                blocking(move || db.as_ref().and_then(|db| boot::book_fingerprint(db)))
                    .await
                    .ok()
                    .flatten();
            let target = Path::new(dir.trim()).join(backup_entry::default_backup_file_name(
                chrono::Local::now(),
                fingerprint.as_deref(),
            ));
            let probe = target.clone();
            let exists = blocking(move || probe.exists()).await.unwrap_or(false);
            if exists {
                state.pending_overwrite.set(Some(target));
            } else {
                run_backup_task(owner, state, target);
            }
        })
    });
}

/// 恢复入口任务：文件选择 → 元数据校验 → 当前模式探测 → 意图落位（弹窗开）。
/// 校验失败即中止（确认弹窗不开，宁可不弹窗——安全面同旧壳）。
fn pick_restore_task(owner: Owner, state: BackupUiState) {
    owner.run(move || {
        spawn(async move {
            let picked = match pick_paths(
                OpenDialog::files()
                    .multiple(false)
                    .filter(FileFilter::new(FILTER_NAME, ["zip", "db"]))
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
            let db = crate::app::db_slot().clone();
            let probe_target = target.clone();
            let probe = blocking(move || match db.as_ref() {
                Some(_) => boot::backup_meta(Path::new(&probe_target)),
                None => Err(session_missing_err()),
            })
            .await;
            let meta = match probe {
                Ok(Ok(meta)) => meta,
                Ok(Err(e)) => {
                    state
                        .status
                        .set(Some(format!("{MSG_META_FAILED_PREFIX}{e}")));
                    return;
                }
                Err(_) => {
                    state.status.set(Some(MSG_READ_TASK_FAILED.to_owned()));
                    return;
                }
            };
            let current = blocking(boot::encryption_status).await;
            let current_encrypted = match current {
                Ok(Ok(flag)) => flag,
                Ok(Err(e)) => {
                    state
                        .status
                        .set(Some(format!("{MSG_ENCRYPTION_STATUS_FAILED_PREFIX}{e}")));
                    return;
                }
                Err(_) => {
                    state.status.set(Some(MSG_READ_TASK_FAILED.to_owned()));
                    return;
                }
            };
            state.restore_pass.set(String::new());
            state.restore_pass_revealed.set(false);
            state.restore_error.set(None);
            state.restore_intent.set(Some(RestoreIntent {
                path: target,
                backup_encrypted: meta.encrypted,
                current_encrypted,
                context_pass: None,
            }));
        })
    });
}

/// 恢复确认任务（弹窗提交）：密文备份附带主口令；失败不关弹窗（口令错误
/// 就地重试，解锁同语义），成功关弹窗反馈重启提示并刷新卡面。
fn confirm_restore_task(owner: Owner, state: BackupUiState) {
    let Some(intent) = state.restore_intent.get_untracked() else {
        return;
    };
    let pass_input = state.restore_pass.get_untracked().clone();
    if state.restore_busy.get_untracked()
        || (state.restore_pass_revealed.get_untracked() && pass_input.is_empty())
    {
        return;
    }
    state.restore_busy.set(true);
    state.restore_error.set(None);
    let passphrase = (intent.backup_encrypted
        || state.restore_pass_revealed.get_untracked()
        || !pass_input.is_empty())
    .then_some(pass_input);
    owner.run(move || {
        spawn(async move {
            let restore_target = intent.path.clone();
            let db = crate::app::db_slot().clone();
            let outcome = blocking(move || match db.as_ref() {
                Some(db) => {
                    boot::restore_backup(db, Path::new(&restore_target), passphrase.as_deref())
                }
                None => Err(session_missing_err()),
            })
            .await;
            match outcome {
                Ok(Ok(result)) => {
                    state.restore_intent.set(None);
                    state.restore_busy.set(false);
                    state.restore_pass.set(String::new());
                    state.status.set(Some(MSG_RESTORE_OK.to_owned()));
                    refresh_rows_task(owner, state);
                    refresh_auto_backup_state(owner, state);
                    tracing::info!(
                        schema_version = result.schema_version,
                        "备份恢复完成，待重启载入"
                    );
                }
                Ok(Err(e)) => {
                    state
                        .restore_error
                        .set(Some(format!("{MSG_RESTORE_FAIL_PREFIX}{e}")));
                    state.restore_busy.set(false);
                    // 后端探测实际密文而元数据未标记（异常产物）时，按需显出口令
                    // 输入——以后端探测为准（旧壳 passphraseRevealedByError 同位）。
                    // 判读以码化错误的 code 类型化判定（#2145：后端 Display 只含
                    // 中文消息，原文案子串 contains 恒不命中、显出接线形同虚设）。
                    if e.is_code(ledger_backup::CODE_PASSPHRASE_REQUIRED) {
                        state.restore_pass_revealed.set(true);
                    }
                }
                Err(_) => {
                    state.restore_error.set(Some(format!(
                        "{MSG_RESTORE_FAIL_PREFIX}{MSG_WRITE_TASK_FAILED}"
                    )));
                    state.restore_busy.set(false);
                }
            }
        })
    });
}

/// 备份管理五卡（动作 / 目录 / 自动 / 列表 / 恢复；五卡共用同一信号组）。
pub(crate) fn backup_cards(palette: views::ViewPalette) -> View {
    let owner = Owner::current();
    let state = BackupUiState {
        backing_up: signal(false),
        pending_overwrite: signal(None),
        last_backup: signal(None),
        last_backup_path: signal(None),
        status: signal(None),
        rows: signal(Vec::new()),
        rows_error: signal(None),
        rows_loading: signal(false),
        pruning: signal(false),
        prune_pending: signal(None),
        backup_dir: signal(pref_backup_dir()),
        keep: signal(pref_keep_limit()),
        keep_text: signal(pref_keep_limit().to_string()),
        auto_enabled: signal(true),
        auto_error: signal(None),
        auto_last: signal(None),
        auto_failing: signal(false),
        auto_failures: signal(0_u32),
        device: signal(None),
        restore_intent: signal(None),
        restore_pass: signal(String::new()),
        restore_pass_revealed: signal(false),
        restore_busy: signal(false),
        restore_error: signal(None),
    };

    // —— 装载批（行集 / 自动备份状态 / 本机设备标识，各一任务；删任一装载
    // 接线 → 对应卡面假默认 → 红）。
    refresh_rows_task(owner, state);
    refresh_auto_backup_state(owner, state);
    // 备份产物失效订阅（#2078）：自动备份完成 / 受管清理后，深路径发射经
    // pending 桥 + 泵换 [`crate::signals`] 广播，此处重读行集与自动状态（旧壳
    // `ledger:backups-changed` 订阅刷新同语义）。订阅随视图实例存亡（句柄移入
    // effect 闭包，交易列表同形）。
    let backups_subscription = signals::subscribe_backups_changed(move || {
        refresh_rows_task(owner, state);
        refresh_auto_backup_state(owner, state);
    });
    effect(move || {
        let _held_for_view_lifetime: &signals::BackupsChangedSubscription = &backups_subscription;
    });
    {
        owner.run(move || {
            spawn(async move {
                let db = crate::app::db_slot().clone();
                let id = blocking(move || db.as_ref().and_then(|db| boot::backup_device_id(db)))
                    .await
                    .ok()
                    .flatten();
                state.device.set(id);
            })
        });
    }

    // 恢复确认弹窗（旧壳 ModalIntent 同形：意图开着 = 呈现；关闭路径 = 取消 /
    // ESC / 成功后由任务关）。#2144 收敛共享封装
    // [`app_restore_confirm_modal`]，与锁定屏恢复弹窗消费同一成员。
    let restore_modal = app_restore_confirm_modal(
        RestoreSignals {
            intent: state.restore_intent,
            pass: state.restore_pass,
            revealed: state.restore_pass_revealed,
            busy: state.restore_busy,
            error: state.restore_error,
        },
        &DATA_RESTORE_IDS,
        move |_: &()| confirm_restore_task(owner, state),
    );

    // 动作卡（动作 + 最近备份回显 + 复制 + 状态条位）。
    let action_card = settings::card(
        CARD_BACKUP,
        palette,
        vec![
            view().child(
                text(BACKUP_HINT)
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.muted),
            ),
            view()
                .flex_row()
                .app_class("items-center gap-2")
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90 disabled:opacity-40",
                        )
                        .disabled(move || state.backing_up.get())
                        .child(
                            text(BACKUP_ONCE)
                                .text_size(px(font_size::SECONDARY))
                                .text_color(palette.primary_fg)
                                .test_id("data-backup-once"),
                        )
                        .on_press(move |_: &()| begin_backup_once(owner, state)),
                )
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-(--radius) border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(move || state.backing_up.get())
                        .child(
                            text(BACKUP_AS)
                                .text_size(px(font_size::SECONDARY))
                                .test_id("data-backup-as"),
                        )
                        .on_press(move |_: &()| begin_backup_as(owner, state)),
                ),
            view().child(
                when(
                    move || state.last_backup.get().is_some(),
                    move || {
                        view()
                            .flex_row()
                            .app_class("items-center gap-2 flex-wrap")
                            .child(
                                text(BACKUP_LAST_LABEL)
                                    .text_size(px(font_size::CAPTION))
                                    .text_color(palette.fg),
                            )
                            .child(
                                text(move || state.last_backup.get().unwrap_or_default())
                                    .text_size(px(font_size::CAPTION))
                                    .text_color(palette.fg)
                                    .test_id("data-last-backup"),
                            )
                            .child(copy_button(
                                "data-copy-last-backup-path",
                                move || state.last_backup_path.get_untracked(),
                                move || state.status.set(Some(MSG_COPIED.to_owned())),
                            ))
                    },
                )
                .otherwise(view),
            ),
            view().child(settings::status_line("data-backup-status", state.status, palette)),
        ],
    );

    // 目录卡（目录回显 / 选择 / 清除 + 保留上限输入）。
    let dir_card = settings::card(
        CARD_BACKUP_DIR,
        palette,
        vec![
            view().child(
                text(DIR_HINT)
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.muted),
            ),
            view().child(
                when(
                    move || !state.backup_dir.get().trim().is_empty(),
                    move || {
                        text(move || state.backup_dir.get())
                            .text_size(px(font_size::CAPTION))
                            .text_color(palette.fg)
                            .test_id("data-backup-dir-value")
                    },
                )
                .otherwise(move || {
                    view().child(
                        text(DIR_UNSET)
                            .text_size(px(font_size::CAPTION))
                            .text_color(palette.muted)
                            .test_id("data-backup-dir-unset"),
                    )
                }),
            ),
            view()
                .flex_row()
                .app_class("items-center gap-2")
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-(--radius) border border-border text-foreground hover:bg-accent",
                        )
                        .child(
                            text(move || {
                                if state.backup_dir.get().trim().is_empty() {
                                    DIR_CHOOSE
                                } else {
                                    DIR_CHANGE
                                }
                            })
                            .text_size(px(font_size::SECONDARY))
                            .test_id("data-pick-backup-dir"),
                        )
                        .on_press(move |_: &()| {
                            // 目录选择对话框：取消零副作用，选定写偏好 + 行集刷新。
                            let (owner, state) = (owner, state);
                            owner.run(move || spawn(async move {
                                let answer = pick_paths(OpenDialog::folders().title(DIR_PICK_TITLE)).await;
                                if let Ok(Some(paths)) = answer
                                    && let Some(dir) = paths.first()
                                {
                                    let dir: String = String::from(dir.to_string_lossy().as_ref());
                                    store_backup_dir(&dir);
                                    // 设备偏好推送（#2078）：目录镜像更新（旧壳
                                    // watch immediate 推送同位；首次兜底认领机会已
                                    // 被启动推送消费，此处镜像更新即时生效）。
                                    backup_host::push_backup_dir(&dir);
                                    state.backup_dir.set(dir);
                                    state.status.set(Some(MSG_DIR_SET.to_owned()));
                                    refresh_rows_task(owner, state);
                                }
                            }));
                        }),
                )
                .child(
                    when(
                        move || !state.backup_dir.get().trim().is_empty(),
                        move || {
                            button()
                                .app_class(
                                    "px-3 py-1.5 rounded-(--radius) border border-border text-foreground hover:bg-accent",
                                )
                                .child(
                                    text(DIR_CLEAR)
                                        .text_size(px(font_size::SECONDARY))
                                        .test_id("data-clear-backup-dir"),
                                )
                                .on_press(move |_: &()| {
                                    store_backup_dir("");
                                    // 设备偏好推送（#2078）：空串推送 = 目录镜像置空，
                                    // 触发入口回到未配置静默跳过（旧壳同语义）。
                                    backup_host::push_backup_dir("");
                                    state.backup_dir.set(String::new());
                                    state.rows.set(Vec::new());
                                    state.rows_error.set(None);
                                    refresh_rows_task(owner, state);
                                })
                        },
                    )
                    .otherwise(view),
                ),
            view()
                .flex_row()
                .app_class("items-center gap-2")
                .child(
                    text(KEEP_LIMIT_LABEL)
                        .text_size(px(font_size::SECONDARY))
                        .text_color(palette.fg),
                )
                .child(
                    input()
                        .test_id("data-backup-max-count")
                        .value(move || state.keep_text.get())
                        .text_color(palette.fg)
                        .w(px(72.0))
                        .app_class("bg-background rounded-(--radius) border border-border px-2 py-1")
                        .on_change(move |value: &str| {
                            // 数字过滤 + 闭集钳制：变更即刻提交，调小立即清理
                            //（旧壳 blur 提交的偏差：即时通道语义同向）。
                            if let Ok(next) = value.trim().parse::<u32>() {
                                let clamped = next.clamp(KEEP_LIMIT_MIN, KEEP_LIMIT_MAX);
                                let prev = state.keep.get_untracked();
                                store_keep_limit(clamped);
                                state.keep.set(clamped);
                                state.keep_text.set(clamped.to_string());
                                if clamped < prev && dir_set(state) {
                                    prune_to_limit_task(owner, state);
                                }
                            }
                        }),
                )
                .child(
                    text(KEEP_LIMIT_SUFFIX)
                        .text_size(px(font_size::CAPTION))
                        .text_color(palette.muted),
                ),
        ],
    );

    // 自动备份卡（开关 + 上次时刻 + 连续失败警示 + 目录缺置提醒 + 宿主注记）。
    let auto_card = {
        let state_row = state;
        settings::card(
            CARD_BACKUP_AUTO,
            palette,
            vec![
                view().flex_row().app_class("items-center gap-2").child(
                    nsh_base::switch()
                        .accessibility_label("自动备份开关")
                        .checked(move || state_row.auto_enabled.get())
                        .on_change({
                            let (owner, state_row) = (owner, state_row);
                            move |value: &bool| apply_auto_toggle(owner, state_row, value.to_owned())
                        })
                        .app_class("shrink-0"),
                ).child(
                    text(AUTO_SWITCH_LABEL)
                        .text_size(px(font_size::SECONDARY))
                        .text_color(palette.fg),
                ),
                view().child(
                    text(move || {
                        format!(
                            "{AUTO_LAST_LABEL}{}",
                            state.auto_last.get().map_or_else(|| AUTO_LAST_NEVER.to_owned(), |t| format_backup_time(&t))
                        )
                    })
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.fg)
                    .test_id("data-auto-last"),
                ),
                view().child(
                    when(
                        move || state.auto_failing.get(),
                        move || {
                            view()
                                .flex_col()
                                .app_class(
                                    "rounded-sm border border-destructive/30 bg-destructive/10 p-2 gap-1",
                                )
                                .child(
                                    text(move || {
                                        format!(
                                            "{AUTO_FAIL_ALERT_PREFIX}{}{AUTO_FAIL_ALERT_SUFFIX}",
                                            state.auto_failures.get()
                                        )
                                    })
                                    .text_size(px(font_size::CAPTION))
                                    .text_color(views::view_palette().danger_fg)
                                    .test_id("data-auto-failure-alert"),
                                )
                        },
                    )
                    .otherwise(view),
                ),
                view().child(
                    when(
                        move || state.backup_dir.get().trim().is_empty(),
                        move || {
                            view().child(
                                text(AUTO_NEED_DIR)
                                    .text_size(px(font_size::CAPTION))
                                    .text_color(palette.muted)
                                    .test_id("data-auto-need-dir"),
                            )
                        },
                    )
                    .otherwise(view),
                ),
                view().child(
                    when(
                        move || state.auto_error.get().is_some(),
                        move || {
                            text(move || state.auto_error.get().unwrap_or_default())
                                .text_size(px(font_size::CAPTION))
                                .text_color(views::view_palette().danger_fg)
                                .test_id("data-auto-error")
                        },
                    )
                    .otherwise(view),
                ),
            ],
        )
    };

    // 立即清理入口（本机桶超额 = 数量确认；桶空 / 无设备标识保守不误报——
    // 旧壳同口径）。删数量判定接线 → 清理不弹确认即执行 → 守卫用例红。
    let manual_prune = move || {
        if state.pruning.get_untracked() || !dir_set(state) {
            return;
        }
        let keep = state.keep.get_untracked() as usize;
        let own_bucket = match state.device.get_untracked().as_deref() {
            Some(device) => state
                .rows
                .get_untracked()
                .iter()
                .filter(|row| row.source_device_id.as_deref() == Some(device))
                .count(),
            None => 0,
        };
        let excess = own_bucket.saturating_sub(keep);
        if excess == 0 {
            state.status.set(Some(MSG_PRUNE_NOT_NEEDED.to_owned()));
            return;
        }
        state.prune_pending.set(Some(excess));
    };

    // 清理确认对话框（warning 级；文案带当次数量——实例随开合重建）。
    let prune_dialog = when(
        move || state.prune_pending.get().is_some(),
        move || {
            let excess = state.prune_pending.get_untracked().unwrap_or(0);
            danger_confirm(
                || true,
                DangerConfirmSpec {
                    level: DangerLevel::Warning,
                    title: PRUNE_CONFIRM_TITLE.to_owned(),
                    lead: None,
                    strong_warning: Some(format!(
                        "{PRUNE_CONFIRM_STRONG_PREFIX}{excess}{PRUNE_CONFIRM_STRONG_SUFFIX}"
                    )),
                    detail: Some(PRUNE_CONFIRM_DETAIL.to_owned()),
                    confirm_label: PRUNE_CONFIRM_OK.to_owned(),
                    cancel_label: PRUNE_CONFIRM_CANCEL.to_owned(),
                },
                move |outcome: DangerOutcome| {
                    state.prune_pending.set(None);
                    if outcome == DangerOutcome::Confirmed {
                        prune_to_limit_task(owner, state);
                    }
                },
            )
        },
    );

    // 覆盖确认对话框（warning 级）：确认才执行备份，取消零写入（#2038 同款
    // 分流）。删守卫分支（覆盖确认直落执行）→ 守卫用例红。
    let overwrite_dialog = danger_confirm(
        move || state.pending_overwrite.get().is_some(),
        DangerConfirmSpec {
            level: DangerLevel::Warning,
            title: OVERWRITE_TITLE.to_owned(),
            lead: Some(OVERWRITE_LEAD.to_owned()),
            strong_warning: Some(OVERWRITE_STRONG.to_owned()),
            detail: None,
            confirm_label: OVERWRITE_OK.to_owned(),
            cancel_label: OVERWRITE_CANCEL.to_owned(),
        },
        move |outcome: DangerOutcome| {
            let target = state.pending_overwrite.get_untracked();
            state.pending_overwrite.set(None);
            if outcome == DangerOutcome::Confirmed
                && let Some(target) = target
            {
                run_backup_task(owner, state, target);
            }
        },
    );

    // 列表卡（手动刷新 + 条数 + 立即清理 + 行集 / 空态 / 失败态）。
    let list_card = settings::card(
        CARD_BACKUP_LIST,
        palette,
        vec![
            view()
                .flex_row()
                .app_class("items-center gap-2")
                .child(
                    button()
                        .app_class(
                            "px-2 py-0.5 rounded-sm border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(move || state.rows_loading.get())
                        .child(
                            text(BUTTON_REFRESH)
                                .text_size(px(font_size::CAPTION))
                                .test_id("data-backup-list-refresh"),
                        )
                        .on_press(move |_: &()| refresh_rows_task(owner, state)),
                ),
            view().child(
                text(LIST_HINT)
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.muted),
            ),
            view()
                .flex_row()
                .app_class("items-center justify-between gap-2 flex-wrap")
                .child(
                    text(move || {
                        format!(
                            "{LIST_COUNT_PREFIX}{}{LIST_COUNT_MIDDLE}{}{LIST_COUNT_SUFFIX}",
                            state.rows.get().len(),
                            state.keep.get()
                        )
                    })
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.fg)
                    .test_id("data-backup-count"),
                )
                .child(
                    button()
                        .app_class(
                            "px-3 py-1 rounded-(--radius) border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(move || state.rows.get().is_empty() || state.pruning.get())
                        .child(
                            text(BUTTON_PRUNE_NOW)
                                .text_size(px(font_size::CAPTION))
                                .test_id("data-backup-prune"),
                        )
                        .on_press(manual_prune),
                ),
            view().child(
                when(
                    move || state.rows_error.get().is_some(),
                    move || {
                        text(move || state.rows_error.get().unwrap_or_default())
                            .text_size(px(font_size::CAPTION))
                            .text_color(views::view_palette().danger_fg)
                            .test_id("data-backup-list-error")
                    },
                )
                .otherwise(move || {
                    view().child(
                        when(
                            move || state.rows.get().is_empty(),
                            move || {
                                text(move || {
                                    if dir_set(state) {
                                        LIST_EMPTY_WITH_DIR.to_owned()
                                    } else {
                                        LIST_EMPTY_NO_DIR.to_owned()
                                    }
                                })
                                .text_size(px(font_size::CAPTION))
                                .text_color(palette.muted)
                                .test_id("data-backup-empty")
                            },
                        )
                        .otherwise(move || {
                            // 行集段 dynamic（读什么变什么重造，#2146 选型标准
                            // 整段重造归 dynamic）：修复前 otherwise 分支内静
                            // 态 children（state.rows 一次读入），列表非空期间
                            // 新增/清理备份后行集不重渲染，面板重开才见新清单
                            //（#2143 审计同根因实例；删 dynamic 回静态 →
                            // backup_list_rows_refresh_in_place_after_new_backup 红）。
                            dynamic(move || {
                                view().flex_col().app_class("gap-1").children(
                                    state
                                        .rows
                                        .get()
                                        .into_iter()
                                        .enumerate()
                                        .map(|(index, row)| backup_row(palette, index, row)),
                                )
                            })
                        }),
                    )
                }),
            ),
        ],
    );

    // 恢复卡（红色警示块 + 入口按钮；最高级危险确认在弹窗内呈现）。
    let restore_card = settings::card(
        CARD_BACKUP_RESTORE,
        palette,
        vec![
            view()
                .flex_col()
                .app_class("rounded-sm border border-destructive/30 bg-destructive/10 p-2 gap-1")
                .child(
                    view()
                        .flex_row()
                        .app_class("gap-1 flex-wrap")
                        .child(text(RESTORE_HINT_BEFORE).text_size(px(font_size::CAPTION)).text_color(palette.fg))
                        .child(
                            text(RESTORE_HINT_STRONG)
                                .text_size(px(font_size::CAPTION))
                                .font_weight(FontWeight::BOLD)
                                .text_color(views::view_palette().danger_fg),
                        )
                        .child(text(RESTORE_HINT_AFTER).text_size(px(font_size::CAPTION)).text_color(palette.fg)),
                ),
            view().child(
                button()
                    .app_class(
                        "px-3 py-1.5 rounded-(--radius) border border-border text-foreground hover:bg-accent disabled:opacity-40",
                    )
                    .disabled(move || state.restore_busy.get())
                    .child(
                        text(RESTORE_BUTTON)
                            .text_size(px(font_size::SECONDARY))
                            .test_id("data-restore"),
                    )
                    .on_press(move |_: &()| pick_restore_task(owner, state)),
            ),
        ],
    );

    view()
        .flex_col()
        .app_class("gap-3")
        .child(action_card)
        .child(dir_card)
        .child(auto_card)
        .child(list_card)
        .child(restore_card)
        .child(overwrite_dialog)
        .child(prune_dialog)
        .child(restore_modal)
}

/// 备份行（清单装配呈现：文件名 + 元件 + 行级「在访达中显示」）。
fn backup_row(palette: views::ViewPalette, index: usize, row: BackupRowView) -> impl IntoElement {
    let path = row.path.clone();
    view()
        .flex_col()
        .app_class("gap-0.5 py-1 border-b border-border/60")
        .child(
            text(row.file_name.clone())
                .text_size(px(font_size::CAPTION))
                .text_color(palette.fg)
                .test_id(format!("data-backup-row-{index}-name").as_str()),
        )
        .child(
            view()
                .flex_row()
                .app_class("items-center gap-2 flex-wrap")
                .child(
                    text(row.meta.clone())
                        .text_size(px(font_size::CAPTION))
                        .text_color(palette.muted)
                        .test_id(format!("data-backup-row-{index}-meta").as_str()),
                )
                .child(
                    button()
                        .app_class(
                            "px-2 py-0.5 rounded-sm border border-border text-foreground hover:bg-accent",
                        )
                        .child(
                            text(REVEAL_LABEL)
                                .text_size(px(font_size::CAPTION))
                                .test_id(format!("data-backup-reveal-{index}").as_str()),
                        )
                        .on_press(move |_: &()| {
                            drop(reveal_path(path.clone()));
                        }),
                ),
        )
}

// ---------------------------------------------------------------------------
// lib 单测：来源 / 设备词表闭集。headless 组件测试见 tests/app_headless.rs
// 设置②节。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 来源 / 设备词表：auto → 自动；manual / pre-update → 手动；缺水印 → 未知。
    #[test]
    fn source_and_device_texts_match_word_list() {
        use ledger_backup::BackupKind as K;
        assert_eq!(source_text(K::Auto), "自动");
        assert_eq!(source_text(K::Manual), "手动");
        assert_eq!(source_text(K::PreUpdate), "手动");
        assert_eq!(device_text(Some("dev-1")), "dev-1");
        assert_eq!(device_text(None), "未知来源");
    }
}
