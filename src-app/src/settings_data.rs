//! 数据安全与连接四卡（issue #2054 NSH 迁移㉓；旧壳 `BackupSettings` /
//! `EncryptionSettings` / `SyncSettings` + `useSyncCard` / `useBookSwitcher`
//! 的重生，ADR-0022 数据页签 / ADR-0078 危险确认 / ADR-0091 多端同步 /
//! ADR-0114 跨账本）。
//!
//! **卡片组**（挂 [`crate::settings`] 数据页签的卡片宿主与状态条位，容器面
//! 单点不变， [`DataSafetyCards`] 为入口组件）：
//! - **备份管理**（`BackupSettings` 全量面，超越 #2038 最小入口）：动作卡
//!   （一键备份〔配置了备份目录时直写受管默认名，否则同另存为对话框〕 +
//!   另存为 + 最近备份回显与复制）、备份目录卡（目录选择 / 清除 + 保留上限
//!   输入）、自动备份卡（开关 + 上次时刻 + 连续失败警示 + 本壳调度宿主缺位
//!   注记）、备份文件列表卡（按当前账本指纹分域的行集 + 手动刷新 + 立即清
//!   理〔warning 级数量确认〕+ 行级「在访达中显示」）与恢复卡（红色警示块
//!   + 恢复确认弹窗——恢复安全备份 + 替换当前全部数据，issue #572 跨模式
//!     警告 + 密文备份主口令，口令错误不关弹窗就地重试）。
//! - **加密**（`EncryptionSettings` 明文库形态的开启流）：warning 提示 +
//!   主口令两框 + ≥8 位 / 一致守卫 + error 级无后门确认。密文库形态（已开
//!   启 / 修改主口令 / 关闭 / 自动解锁）在本壳不可达：NSH 只引导未加密库
//!   （立桩口径；解锁向导界外另票），在屏偏差注记；口令强度实时条未迁
//!   （zxcvbn 依赖面归偏差记录，见交付报告）。
//! - **多端同步**（`useSyncCard`）：明文警示 / 状态回显（上次同步 / 挂起
//!   数量与明细）、立即同步（成功 → 轮次报告反馈 + 失效信号广播）、通道配
//!   置表单（S3 七字段 + 同步空间 + 厂商预设预填 / 端点反查回显 + 密钥不回
//!   显口径）、保存前「测试连接」与检查点发布 / 新端引导向导。同步写语义
//!   不经本壳（轮次 / 发布 / 引导全在域内单点编排，本壳只解包口令与触发，
//!   票面实施决策）；不依赖同步协议 crate（壳只依赖域，ADR-0112）。
//! - **账本切换**（`useBookSwitcher`）：清单卡（跨账本汇总**置顶入口** +
//!   行按压切换〔warning 级轻量确认〕+ 行级改名弹窗 / 移除气泡确认 + 新建
//!   弹窗），清单读注册表最新落盘态；切换写活动指针，本壳无原位重引导
//!   （旧壳 restart_app 自动重载的偏差在反馈句内披露）。
//!
//! 偏好镜像（备份目录 / 保留上限）：NSH 自建平台存储 [`Storage`]（键值形
//! 态同旧壳 `backup_dir` / `backup_max_count` 的 JSON 形状，不与旧壳共享、
//! 不进 SQLite、不动 schema——外观卡同款契约）。动作反馈落卡内状态条位
//! （#2038 裁决，NSH 无 toast）；阻塞 IO（备份写、恢复、探测、同步网络段、
//! 注册表读写）一律 `blocking` 后台执行；信号读写都在引擎 UI 线程。界面文
//! 案仅 zh-CN（旧壳 settings.* / books.* zh-CN 词条原样承载）；码化错误的
//! 中文原样呈现。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::path::{Path, PathBuf};

use ledger_sync_engine::SyncChannelConfig;
use nsh_base::{button, input};
use nsh_core::element::FontWeight;
use nsh_core::file_dialog::{FileFilter, OpenDialog, SaveDialog};
use nsh_core::prelude::*;
use nsh_core::storage::Storage;

use crate::app::book_dir;
use crate::assembly::CategoryOption;
use crate::danger::{DangerConfirmSpec, DangerLevel, DangerOutcome, danger_confirm};
use crate::modal::app_modal;
use crate::nav_model::NavRequest;
use crate::popconfirm;
use crate::{backup_entry, backup_host, boot, nav, nav_model, select, settings, signals, views};

// ---------------------------------------------------------------------------
// 词表（旧壳 settings.data.* / books.* zh-CN 词条原样；i18n 框架级方案不在
// 本系列，字面承载）。
// ---------------------------------------------------------------------------

const CARD_BACKUP: &str = "备份";
const CARD_BACKUP_DIR: &str = "备份目录";
const CARD_BACKUP_AUTO: &str = "自动备份";
const CARD_BACKUP_LIST: &str = "备份文件列表";
const CARD_BACKUP_RESTORE: &str = "恢复";
const CARD_ENCRYPTION: &str = "加密";
const CARD_SYNC: &str = "多端同步";
const CARD_BOOKS: &str = "账本切换";
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
const RESTORE_MODAL_TITLE: &str = "确认恢复";
const RESTORE_CONFIRM_STRONG: &str =
    "恢复将替换当前全部数据，且不可撤销。系统会在恢复前自动备份当前数据到应用数据目录。";
const RESTORE_CONFIRM_ACTION: &str = "确认恢复";
const RESTORE_CANCEL: &str = "取消";
const RESTORE_PASS_LABEL: &str = "备份的主口令";
const RESTORE_PASS_PLACEHOLDER: &str = "输入该备份的主口令";
const RESTORE_CROSS_MODE_TITLE: &str = "加密模式不一致";
const RESTORE_TO_PLAINTEXT_WARN: &str =
    "当前库为加密库：恢复这份明文备份后，数据将变为未加密，应用重启后以明文库启动。";
const RESTORE_TO_ENCRYPTED_WARN: &str =
    "这份备份为密文备份：恢复后此库将变为加密库，应用重启后需凭该备份的主口令解锁。";
const RESTORE_FAIL_PREFIX: &str = "恢复失败: ";
const MSG_RESTORE_OK: &str =
    "恢复成功，请重启应用进入新库（本壳无原位重引导：旧壳恢复后自动重启的偏差见交付报告）";
const MSG_META_FAILED_PREFIX: &str = "读取备份信息失败: ";
const MSG_ENCRYPTION_STATUS_FAILED_PREFIX: &str = "读取加密状态失败: ";
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
const PRUNE_CONFIRM_DETAIL: &str =
    "删除的是备份产物，账本数据本身不受影响；自动备份开启时会按保留上限继续生成新备份。";
const PRUNE_CONFIRM_OK: &str = "确认清理";
const PRUNE_CONFIRM_CANCEL: &str = "取消";
const FILTER_NAME: &str = "开源记账备份";
const SESSION_MISSING: &str = "内部错误：账本库会话缺失";
const READ_TASK_FAILED: &str = "内部错误：读任务异常退出";
const WRITE_TASK_FAILED: &str = "内部错误：写入任务异常退出";

const ENCRYPTION_HINT: &str = "为账本数据库开启文件级加密：磁盘上的库文件与备份不再能被任意工具直接阅读。开启是一次自动完成的整库转换，失败时原库原样保留。";
const ENCRYPTION_WARN_TITLE: &str = "开启前请确认";
const ENCRYPTION_WARN_BODY: &str = "开启后每次启动都需输入主口令解锁；主口令不会被保存、也没有找回手段——遗忘将无法读取数据（可重置为全新空库，旧库保留为密文副本）。转换完成后应用将自动重启。";
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
    "将把整个账本库转换为加密库（转换期间请勿关闭应用），完成后应用自动重启。";
const ENCRYPT_CONFIRM_OK: &str = "继续开启加密";
const ENCRYPT_CONFIRM_CANCEL: &str = "取消";
const ENCRYPT_OK_HOST_HINT: &str = "加密已开启（整库转换完成）。本壳暂不支持密文库启动——解锁向导界外另票：请用旧壳打开此数据（下次启动本壳将呈现「密文库暂不支持」）。";
const ENCRYPT_FAILED_PREFIX: &str = "开启加密失败: ";
const ENCRYPT_STATUS_FAILED_PREFIX: &str = "读取加密状态失败: ";
const ENCRYPT_RETRY: &str = "重试";
const ENCRYPT_ENABLED_LOCAL_TITLE: &str = "已开启加密（密文库）";
const ENCRYPT_ENABLED_LOCAL_BODY: &str = "当前账本库为密文库；本壳只引导未加密账本库（解锁向导界外另票）——修改主口令、关闭加密与自动解锁随解锁向导另票开门。";
const ENCRYPT_PLAIN_ONLY_NOTE: &str = "本壳只引导未加密账本库：密文库形态（修改主口令 / 关闭加密 / 自动解锁）与解锁向导随解锁票开门。";

const SYNC_PLAINTEXT_WARNING: &str = "当前账本未开启加密：同步数据将以明文存放于对象存储。如需加密同步，请先在本页「加密」卡开启加密模式。";
const SYNC_ENCRYPTED_HINT: &str = "本账库为密文形态：同步数据将以主口令加密后上通道。";
const SYNC_LAST_LABEL: &str = "上次同步：";
const SYNC_NEVER: &str = "从未同步";
const SYNC_PARKED_LABEL: &str = "挂起操作：";
const SYNC_PARKED_HINT: &str =
    "存在无法在本机执行、待后续重投递的同步操作；升级或补齐数据后将自动重试。";
const SYNC_PARKED_LIST_TITLE: &str = "挂起操作明细";
const SYNC_PARKED_ITEM_PREFIX: &str = "挂起：";
const SYNC_NOW: &str = "立即同步";
const SYNC_OK_PREFIX: &str = "同步完成：上传 ";
const SYNC_OK_MIDDLE_UP: &str = " 条，应用 ";
const SYNC_OK_MIDDLE_APPLIED: &str = " 条，挂起 ";
const SYNC_OK_SUFFIX: &str = " 条。";
const SYNC_PARKED_PREFIX: &str = "同步完成，但有 ";
const SYNC_PARKED_SUFFIX: &str = " 条操作无法在本机执行，已挂起待处理。";
const SYNC_PASS_LABEL: &str = "主口令";
const SYNC_PASS_PLACEHOLDER: &str = "主口令（留空使用已记住的口令）";
const SYNC_CHANNEL_TITLE: &str = "通道配置";
const SYNC_ENDPOINT_PLACEHOLDER: &str = "端点地址，如 https://s3.example.com";
const SYNC_REGION_PLACEHOLDER: &str = "签名区域，如 cn-hangzhou";
const SYNC_BUCKET_PLACEHOLDER: &str = "桶名";
const SYNC_PREFIX_PLACEHOLDER: &str = "对象键前缀（留空 = 桶根）";
const SYNC_SPACE_PLACEHOLDER: &str = "同步空间";
const SYNC_SPACE_HINT: &str = "同步空间是两端约定的同步世界：两台设备填写同一空间即同步同一份账本数据（默认 default）；不同账本用不同空间隔离。仅限小写字母、数字与短横线。";
const SYNC_ACCESS_KEY_PLACEHOLDER: &str = "Access Key ID";
const SYNC_SECRET_PLACEHOLDER: &str = "Secret Access Key";
const SYNC_SECRET_SAVED_PLACEHOLDER: &str = "Secret Access Key（已保存，留空则保持不变）";
const SYNC_PATH_STYLE_LABEL: &str = "使用 path-style 寻址（自建服务多为是）";
const SYNC_VENDOR_CUSTOM: &str = "其他（自定义）";
const SYNC_VENDOR_HINT: &str =
    "预设只用于快速预填端点、地域与寻址方式，所有字段仍可编辑；预设本身不保存进配置。";
const SYNC_VENDOR_META_TIER_UNVERIFIED: &str = "未实测";
const SYNC_SAVE_CHANNEL: &str = "保存配置";
const SYNC_TEST_CONNECTION: &str = "测试连接";
const SYNC_SAVE_OK: &str = "通道配置已保存";
const SYNC_TEST_OK: &str = "连接成功：通道可读，这份配置可以同步";
const SYNC_FAILED_PREFIX: &str = "同步失败: ";
const SYNC_SAVE_FAILED_PREFIX: &str = "保存通道配置失败: ";
const SYNC_TEST_FAILED_PREFIX: &str = "测试连接失败: ";
const SYNC_STATUS_FAILED_PREFIX: &str = "读取同步状态失败: ";
const SYNC_CHECKPOINT_TITLE: &str = "检查点与新端加入";
const SYNC_CHECKPOINT_HINT: &str = "已有账本数据的设备先「发布检查点」把全量快照放上通道；全新空账本的设备（如手机）经「从通道引导本端」加入同步。";
const SYNC_PUBLISH: &str = "发布检查点到通道";
const SYNC_PUBLISH_OK_PREFIX: &str = "检查点已发布：第 ";
const SYNC_PUBLISH_OK_MIDDLE: &str = " 代，";
const SYNC_PUBLISH_PLAINTEXT: &str = "本次发布未开启加密：检查点快照将明文存放于对象存储。";
const SYNC_PUBLISH_FAILED_PREFIX: &str = "发布检查点失败: ";
const SYNC_BOOTSTRAP_BUTTON: &str = "从通道引导本端…";
const SYNC_BOOTSTRAP_MODAL_TITLE: &str = "从通道引导本端";
const SYNC_BOOTSTRAP_WARNING: &str = "引导将以通道快照整库替换本机账本数据，本机现有数据将丢失；完成后应用将自动重启。仅在本机是全新空账本时使用。";
const SYNC_BOOTSTRAP_PRECHECKING: &str = "正在检查通道上的检查点…";
const SYNC_BOOTSTRAP_NOT_FOUND: &str =
    "通道上还没有检查点：请先在已有数据的设备上「发布检查点」，再回到本机引导。";
const SYNC_BOOTSTRAP_FOUND_PREFIX: &str = "通道上发现检查点：第 ";
const SYNC_BOOTSTRAP_FOUND_MIDDLE: &str = " 代，";
const SYNC_BOOTSTRAP_PASS_PLACEHOLDER: &str = "主口令（快照为密文时必填）";
const SYNC_BOOTSTRAP_CANCEL: &str = "取消";
const SYNC_BOOTSTRAP_CONFIRM: &str = "引导并重启";
const SYNC_BOOTSTRAP_FAILED_PREFIX: &str = "引导失败: ";
const SYNC_BOOTSTRAP_OK_HOST_HINT: &str = "引导完成（本机账本数据已整库替换）：请重启应用载入引导数据（本壳无原位重引导，旧壳自动重启的偏差见交付报告）";
const SYNC_BOOTSTRAP_REENCRYPTED_HINT: &str =
    "；本机已转为密文库：重启后本壳将呈现启动失败，请改用旧壳（解锁向导界外另票）";

const BOOKS_ENTRY_SUMMARY: &str = "全部账本（投资汇总）";
const BOOKS_FALLBACK_TITLE: &str = "账本注册表异常，已回退默认账本";
const BOOKS_PANEL_CURRENT: &str = "当前";
const BOOKS_NEW: &str = "新建账本";
const BOOKS_RENAME: &str = "重命名";
const BOOKS_REMOVE: &str = "移除";
const BOOKS_REMOVE_CONFIRM_PREFIX: &str =
    "移除后可随时重新登记找回，账本文件仍保留在磁盘。确定移除「";
const BOOKS_REMOVE_CONFIRM_SUFFIX: &str = "」？";
const BOOKS_REMOVE_ACTION: &str = "移除";
const BOOKS_CANCEL: &str = "取消";
const BOOKS_CREATE_TITLE: &str = "新建账本";
const BOOKS_RENAME_TITLE: &str = "重命名账本";
const BOOKS_NAME_LABEL: &str = "账本名称";
const BOOKS_CREATE_PLACEHOLDER: &str = "例如：家庭账本";
const BOOKS_CREATE_CONFIRM: &str = "创建";
const BOOKS_RENAME_CONFIRM: &str = "保存";
const BOOKS_SWITCH_TITLE: &str = "切换账本";
const BOOKS_SWITCH_CONFIRM: &str = "切换并重启后生效";
const BOOKS_SWITCH_DETAIL: &str =
    "将写活动账本指针：重启应用后进入目标账本。未保存的界面状态会丢失，账本数据本身不受影响。";
const BOOKS_SWITCHING_PREFIX: &str = "已切换到「";
const BOOKS_SWITCHING_SUFFIX: &str = "」，重启应用后进入新账本（旧壳自动重载的偏差见交付报告）";
const BOOKS_ACTION_FAILED_PREFIX: &str = "操作失败: ";
const BOOKS_SET_FAILED_PREFIX: &str = "设置失败: ";
const BOOKS_SESSION_UNAVAILABLE: &str = "账本注册表尚未就绪，请重启应用后再试";
const BOOKS_CREATED_PREFIX: &str = "已创建账本「";
const BOOKS_CREATED_SUFFIX: &str = "」，进入即自动建库";
const BOOKS_RENAMED_PREFIX: &str = "已重命名为「";
const BOOKS_RENAMED_SUFFIX: &str = "」";
const BOOKS_REMOVED_PREFIX: &str = "已移除账本「";
const BOOKS_REMOVED_SUFFIX: &str = "」，文件仍保留在磁盘";

/// 主口令最小长度（旧壳 `PASSPHRASE_MIN_LENGTH` 同值：≥8 仅界面判定，后端
/// 契约不动；短口令即时红显 + 提交禁用，不拦截键入，ADR-0058 口径）。
const PASSPHRASE_MIN_LENGTH: usize = 8;

/// 保留上限默认值（旧壳 store `backup_max_count` 出厂值同值；上限面 1–100）。
const KEEP_LIMIT_DEFAULT: u32 = 30;
const KEEP_LIMIT_MIN: u32 = 1;
const KEEP_LIMIT_MAX: u32 = 100;

/// 偏好存储键（旧壳 store `backup_dir` / `backup_max_count` 同键字面；NSH
/// 自建平台存储，不与旧壳共享——外观卡同款契约）。
const BACKUP_DIR_KEY: &str = "backup_dir";
const BACKUP_MAX_COUNT_KEY: &str = "backup_max_count";

// ---------------------------------------------------------------------------
// 装配层（纯函数与纯投影；渲染期零格式化——行文本已是装配产物）。
// ---------------------------------------------------------------------------

/// 备份产物大小展示（旧壳 formatSize 同口径：B / ×1 小数 KB / MB）。
pub(crate) fn format_backup_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// 备份时刻展示（旧壳 formatIsoMinute 同口径：ISO 剥到分钟 `YYYY-MM-DD
/// HH:mm`；形状不符原样透传，不做第二份口径的格式化猜测）。
pub(crate) fn format_backup_time(iso: &str) -> String {
    let bytes: Vec<char> = iso.chars().collect();
    match (bytes.get(10), bytes.get(16)) {
        (Some('T'), _) if bytes.len() >= 16 => {
            let date: String = bytes[..10].iter().collect();
            let clock: String = bytes[11..16].iter().collect();
            format!("{date} {clock}")
        }
        _ => iso.to_owned(),
    }
}

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
// S3 厂商预设（旧壳 `@ledger/utils/s3-vendors` 的移植；纯展示层数据与纯函
// 数：预设只预填端点模板 / 寻址 / 常用地域并按端点反查回显，不落库、不进
// 后端契约）。「verified」唯一翻转点在 [`S3_VENDOR_PRESETS`] 字段。
// ---------------------------------------------------------------------------

/// 端点模板里的地域占位符（预填时按所选地域替换）。
const REGION_PLACEHOLDER: &str = "{region}";
/// 「其他（自定义）」哨兵 id：不在预设表内，恒为下拉末项。
const CUSTOM_VENDOR_ID: &str = "custom";

/// 单个厂商预设（字段全部只读；与旧壳 S3VendorPreset 同形状）。
pub(crate) struct S3VendorPreset {
    /// 稳定判别键。
    pub id: &'static str,
    /// 厂商专名（不进翻译资源）。
    pub name: &'static str,
    /// 端点模板（含 [`REGION_PLACEHOLDER`]）。
    pub endpoint_template: &'static str,
    /// 默认寻址方式。
    pub path_style: bool,
    /// 常用地域快捷项（首项为预填默认地域）。
    pub regions: &'static [&'static str],
    /// 端点主机名的服务段前缀（反查判据之一）。
    pub host_prefixes: &'static [&'static str],
    /// 端点主机名后缀（含前导点，防同名后缀仿冒）。
    pub host_suffixes: &'static [&'static str],
}

/// 国内主流对象存储厂商预设表（端点取各家公开文档的 S3 协议专属端点，与
/// 旧壳同行注释一致；地域只给常用地域快捷项，不是全部地域）。
pub(crate) const S3_VENDOR_PRESETS: &[S3VendorPreset] = &[
    S3VendorPreset {
        id: "aliyun-oss",
        name: "阿里云 OSS",
        endpoint_template: "https://s3.oss-{region}.aliyuncs.com",
        path_style: false,
        regions: &[
            "cn-hangzhou",
            "cn-shanghai",
            "cn-beijing",
            "cn-shenzhen",
            "cn-guangzhou",
            "cn-chengdu",
        ],
        host_prefixes: &["s3.oss-", "oss-"],
        host_suffixes: &[".aliyuncs.com"],
    },
    S3VendorPreset {
        id: "tencent-cos",
        name: "腾讯云 COS",
        endpoint_template: "https://cos.{region}.myqcloud.com",
        path_style: false,
        regions: &[
            "ap-guangzhou",
            "ap-beijing",
            "ap-shanghai",
            "ap-chengdu",
            "ap-hongkong",
        ],
        host_prefixes: &["cos."],
        host_suffixes: &[".myqcloud.com"],
    },
    S3VendorPreset {
        id: "huawei-obs",
        name: "华为云 OBS",
        endpoint_template: "https://obs.{region}.myhuaweicloud.com",
        path_style: false,
        regions: &[
            "cn-north-4",
            "cn-east-3",
            "cn-south-1",
            "cn-southwest-2",
            "cn-north-1",
        ],
        host_prefixes: &["obs."],
        host_suffixes: &[".myhuaweicloud.com"],
    },
    S3VendorPreset {
        id: "volcengine-tos",
        name: "火山引擎 TOS",
        endpoint_template: "https://tos-s3-{region}.volces.com",
        path_style: false,
        regions: &["cn-beijing", "cn-shanghai", "cn-guangzhou"],
        host_prefixes: &["tos-s3-", "tos-"],
        host_suffixes: &[".volces.com"],
    },
    S3VendorPreset {
        id: "qiniu-kodo",
        name: "七牛云 Kodo",
        endpoint_template: "https://s3.{region}.qiniucs.com",
        path_style: true,
        regions: &["cn-east-1", "cn-east-2", "cn-north-1", "cn-south-1"],
        host_prefixes: &["s3."],
        host_suffixes: &[".qiniucs.com"],
    },
];

/// 按 id 取厂商预设（自定义或未知 id 回 `None`）。
pub(crate) fn find_vendor_preset(vendor_id: &str) -> Option<&'static S3VendorPreset> {
    S3_VENDOR_PRESETS
        .iter()
        .find(|vendor| vendor.id == vendor_id)
}

/// 下拉项全集：预设按声明序 + 末尾固定「其他（自定义）」（旧壳验收判据）。
/// 当前无一家实测，档位标注一律如实「未实测」（verified 唯一翻转点在预设
/// 表字段，实测结论回写后再分级）。
pub(crate) fn vendor_options() -> Vec<CategoryOption> {
    let mut options: Vec<CategoryOption> = S3_VENDOR_PRESETS
        .iter()
        .map(|vendor| CategoryOption {
            id: vendor.id.to_owned(),
            label: format!("{}（{}）", vendor.name, SYNC_VENDOR_META_TIER_UNVERIFIED),
        })
        .collect();
    options.push(CategoryOption {
        id: CUSTOM_VENDOR_ID.to_owned(),
        label: SYNC_VENDOR_CUSTOM.to_owned(),
    });
    options
}

/// 选中厂商后的字段预填（旧壳 vendorPrefill 同口径：地域缺省或不在常用地
/// 域清单时回退首项；自定义 / 未知厂商不预填回 `None`）。返回
/// （端点，地域，寻址方式）。
pub(crate) fn vendor_prefill(
    vendor_id: &str,
    region: Option<&str>,
) -> Option<(String, String, bool)> {
    let vendor = find_vendor_preset(vendor_id)?;
    let chosen = match region {
        Some(region) if vendor.regions.contains(&region) => region.to_owned(),
        _ => vendor.regions.first().unwrap_or(&"").to_string(),
    };
    let endpoint = vendor
        .endpoint_template
        .replace(REGION_PLACEHOLDER, &chosen);
    Some((endpoint, chosen, vendor.path_style))
}

/// 从端点里取主机名（容忍省略 scheme 的写法；取不到回空串）。
fn hostname_of(endpoint: &str) -> String {
    let raw = endpoint.trim();
    if raw.is_empty() {
        return String::new();
    }
    let without_scheme = raw
        .strip_prefix("https://")
        .or_else(|| raw.strip_prefix("http://"))
        .unwrap_or(raw);
    without_scheme
        .split(['/', ':'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// 去掉主机名最左一段（虚拟托管形态的 bucket 前缀）；无剩余段回空串。
fn without_leading_label(host: &str) -> String {
    match host.find('.') {
        Some(dot) => host[dot + 1..].to_owned(),
        None => String::new(),
    }
}

/// 按端点反查厂商（旧壳 matchVendorByEndpoint 同判据：「服务段前缀 + 品牌
/// 后缀」两端同时成立；虚拟托管下 bucket 是最左一段，两级都试。空串、无法
/// 解析与自建域名一律回「其他（自定义）」——不猜、不按相似度兜底）。
pub(crate) fn match_vendor_by_endpoint(endpoint: &str) -> &'static str {
    let host = hostname_of(endpoint);
    if host.is_empty() {
        return CUSTOM_VENDOR_ID;
    }
    let candidates = [host.clone(), without_leading_label(&host)];
    let hit = S3_VENDOR_PRESETS.iter().find(|vendor| {
        candidates.iter().any(|candidate| {
            vendor
                .host_suffixes
                .iter()
                .any(|suffix| candidate.ends_with(suffix))
                && vendor
                    .host_prefixes
                    .iter()
                    .any(|prefix| candidate.starts_with(prefix))
        })
    });
    hit.map(|vendor| vendor.id).unwrap_or(CUSTOM_VENDOR_ID)
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

// ---------------------------------------------------------------------------
// 共用呈现件（密文输入行 / 破坏色便捷面 / 表单字段行）。
// ---------------------------------------------------------------------------

/// 破坏色便捷面（主题令牌缺失回退亮灰可读兜底，backup_entry 同款口径）。
fn danger_fg() -> AdaptiveColor {
    nsh_core::element::tokens()
        .color("destructive")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into())
}

/// 复制路径按钮（旧壳 copy-text 通道：写剪贴板 + 卡内状态条位反馈）。
fn copy_button(
    id: &'static str,
    value_of: impl Fn() -> Option<String> + Copy + 'static,
    on_copied: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    button()
        .app_class("px-2 py-0.5 rounded-md border border-border text-foreground hover:bg-accent")
        .child(
            text(BUTTON_COPY_PATH)
                .text_size(px(11.0))
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

/// 密文输入行（标签 + masked 输入框；旧壳 NInput type=password 对应件）。
fn pass_input(
    target: Signal<String>,
    palette: views::ViewPalette,
    id: &'static str,
    label: &'static str,
    placeholder: &'static str,
) -> View {
    view()
        .flex_col()
        .app_class("gap-1")
        .child(
            text(label)
                .text_size(px(12.0))
                .text_color(palette.fg),
        )
        .child(
            input()
                .test_id(id)
                .value(move || target.get())
                .masked(true)
                .placeholder(placeholder)
                .optional_label(label)
                .text_color(palette.fg)
                .w(px(260.0))
                .app_class(
                    "bg-background rounded-md border border-border px-2 py-1 placeholder:text-muted-foreground",
                )
                .on_change(move |value: &str| target.set(value.to_owned())),
        )
}

/// 通道表单普通字段行（占位即标签；无障碍名 = 占位原文）。
fn field_input(
    palette: views::ViewPalette,
    id: &'static str,
    placeholder: &'static str,
    target: Signal<String>,
    masked: bool,
    dynamic_placeholder: Option<impl Fn() -> &'static str + Copy + 'static>,
) -> View {
    let width = if masked { px(260.0) } else { px(420.0) };
    let base = input()
        .test_id(id)
        .value(move || target.get())
        .optional_label(placeholder)
        .text_color(palette.fg)
        .w(width)
        .app_class(
            "bg-background rounded-md border border-border px-2 py-1 placeholder:text-muted-foreground",
        )
        .on_change(move |value: &str| target.set(value.to_owned()));
    let base = match dynamic_placeholder {
        Some(provider) => base.placeholder(provider),
        None => base.placeholder(placeholder),
    };
    let base = if masked { base.masked(true) } else { base };
    view().child(base)
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
    /// 恢复意图（弹窗开着 = Some）。
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

/// 恢复确认弹窗意图（旧壳 RestoreIntent 的本壳子集；本壳无上下文口令面）。
#[derive(Clone, Debug, PartialEq, Eq)]
struct RestoreIntent {
    /// 所选备份文件路径。
    path: String,
    /// 备份是否为密文（元数据标记）。
    backup_encrypted: bool,
    /// 当前库是否为密文（文件即真相）。
    current_encrypted: bool,
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
                None => Err(ledger_infra::error::AppError::coded(
                    boot::CODE_SESSION_MISSING,
                    SESSION_MISSING,
                )),
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
                    state.rows_error.set(Some(READ_TASK_FAILED.to_owned()));
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
                None => Err(ledger_infra::error::AppError::coded(
                    boot::CODE_SESSION_MISSING,
                    SESSION_MISSING,
                )),
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
                Err(_) => state.auto_error.set(Some(READ_TASK_FAILED.to_owned())),
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
                None => Err(ledger_infra::error::AppError::coded(
                    boot::CODE_SESSION_MISSING,
                    SESSION_MISSING,
                )),
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
                    "{MSG_BACKUP_FAILED_PREFIX}{WRITE_TASK_FAILED}"
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
            None => Err(ledger_infra::error::AppError::coded(
                boot::CODE_SESSION_MISSING,
                SESSION_MISSING,
            )),
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
            Err(_) => state.status.set(Some(format!("{MSG_PRUNE_FAILED_PREFIX}{WRITE_TASK_FAILED}"))),
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
                None => Err(ledger_infra::error::AppError::coded(
                    boot::CODE_SESSION_MISSING,
                    SESSION_MISSING,
                )),
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
                        "{MSG_AUTO_TOGGLE_FAILED_PREFIX}{WRITE_TASK_FAILED}"
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
                None => Err(ledger_infra::error::AppError::coded(
                    boot::CODE_SESSION_MISSING,
                    SESSION_MISSING,
                )),
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
                    state.status.set(Some(READ_TASK_FAILED.to_owned()));
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
                    state.status.set(Some(READ_TASK_FAILED.to_owned()));
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
                None => Err(ledger_infra::error::AppError::coded(
                    boot::CODE_SESSION_MISSING,
                    SESSION_MISSING,
                )),
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
                        .set(Some(format!("{RESTORE_FAIL_PREFIX}{e}")));
                    state.restore_busy.set(false);
                    // 后端探测实际密文而元数据未标记（异常产物）时，按需显出口令
                    // 输入——以后端探测为准（旧壳 passphraseRevealedByError 同位）。
                    if e.to_string().contains("backup.passphrase-required") {
                        state.restore_pass_revealed.set(true);
                    }
                }
                Err(_) => {
                    state
                        .restore_error
                        .set(Some(format!("{RESTORE_FAIL_PREFIX}{WRITE_TASK_FAILED}")));
                    state.restore_busy.set(false);
                }
            }
        })
    });
}

/// 恢复确认可提交判定（口令框显出时须非空；执行中禁用）。
fn can_submit_restore(state: BackupUiState) -> bool {
    let needs_pass = state.restore_pass_revealed.get()
        || state
            .restore_intent
            .get_untracked()
            .is_some_and(|intent| intent.backup_encrypted);
    !needs_pass || !state.restore_pass.get().is_empty()
}

/// 恢复确认弹窗内容（旧壳 RestoreConfirmModal 语义面：恢复后果警示 +
/// 跨模式警告 + 密文备份主口令 + 弹窗内错误位）。
fn restore_modal_body(state: BackupUiState, owner: Owner, palette: views::ViewPalette) -> View {
    let body_owner = owner;
    let Some(intent) = state.restore_intent.get() else {
        return view();
    };
    let cross_mode = (intent.backup_encrypted != intent.current_encrypted).then_some({
        if intent.backup_encrypted {
            (RESTORE_CROSS_MODE_TITLE, RESTORE_TO_ENCRYPTED_WARN)
        } else {
            (RESTORE_CROSS_MODE_TITLE, RESTORE_TO_PLAINTEXT_WARN)
        }
    });
    let show_pass = intent.backup_encrypted || state.restore_pass_revealed.get();
    let busy = state.restore_busy.get();
    view()
        .flex_col()
        .app_class("gap-2")
        .child(
            text(format!("备份文件：{}", intent.path))
                .text_size(px(12.0))
                .text_color(palette.fg)
                .test_id("data-restore-path"),
        )
        .child(
            view()
                .flex_col()
                .app_class("rounded-md border border-destructive/30 bg-destructive/10 p-2 gap-1")
                .child(
                    text(RESTORE_CONFIRM_STRONG)
                        .text_size(px(12.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(danger_fg()),
                ),
        )
        .children(cross_mode.map(|(title, warning)| {
            view()
                .flex_col()
                .app_class("rounded-md border border-destructive/30 bg-destructive/10 p-2 gap-1")
                .child(
                    text(title)
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.fg)
                        .test_id("data-restore-cross-mode-title"),
                )
                .child(
                    text(warning)
                        .text_size(px(12.0))
                        .text_color(palette.fg)
                        .test_id("data-restore-cross-mode"),
                )
        }))
        .children(show_pass.then(|| pass_input(
            state.restore_pass,
            palette,
            "data-restore-passphrase",
            RESTORE_PASS_LABEL,
            RESTORE_PASS_PLACEHOLDER,
        )))
        .children(state.restore_error.get().map(|err| {
            text(err.clone())
                .text_size(px(12.0))
                .text_color(danger_fg())
                .test_id("data-restore-error")
        }))
        .child(
            view()
                .flex_row()
                .justify_end()
                .app_class("gap-2")
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(busy)
                        .child(
                            text(RESTORE_CANCEL)
                                .text_size(px(13.0))
                                .test_id("data-restore-cancel"),
                        )
                        .on_press(move |_: &()| {
                            if !state.restore_busy.get_untracked() {
                                state.restore_intent.set(None);
                            }
                        }),
                )
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-md bg-destructive hover:opacity-90 disabled:opacity-40",
                        )
                        .disabled(!can_submit_restore(state) || busy)
                        .child(
                            text(RESTORE_CONFIRM_ACTION)
                                .text_size(px(13.0))
                                .text_color(rgb(0xffffff))
                                .test_id("data-restore-confirm"),
                        )
                        .on_press(move |_: &()| confirm_restore_task(body_owner, state)),
                ),
        )
}

/// 备份管理五卡（动作 / 目录 / 自动 / 列表 / 恢复；五卡共用同一信号组）。
fn backup_cards(palette: views::ViewPalette) -> View {
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
    // ESC / 成功后由任务关）。
    let restore_modal = app_modal(
        move || state.restore_intent.get().is_some(),
        || RESTORE_MODAL_TITLE.to_owned(),
        move || {
            if !state.restore_busy.get_untracked() {
                state.restore_intent.set(None);
            }
        },
        move || restore_modal_body(state, owner, palette),
    );

    // 动作卡（动作 + 最近备份回显 + 复制 + 状态条位）。
    let action_card = settings::card(
        CARD_BACKUP,
        palette,
        vec![
            view().child(
                text(BACKUP_HINT)
                    .text_size(px(12.0))
                    .text_color(palette.muted),
            ),
            view()
                .flex_row()
                .app_class("items-center gap-2")
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-md bg-primary hover:opacity-90 disabled:opacity-40",
                        )
                        .disabled(move || state.backing_up.get())
                        .child(
                            text(BACKUP_ONCE)
                                .text_size(px(13.0))
                                .text_color(rgb(0xffffff))
                                .test_id("data-backup-once"),
                        )
                        .on_press(move |_: &()| begin_backup_once(owner, state)),
                )
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(move || state.backing_up.get())
                        .child(
                            text(BACKUP_AS)
                                .text_size(px(13.0))
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
                                    .text_size(px(12.0))
                                    .text_color(palette.fg),
                            )
                            .child(
                                text(move || state.last_backup.get().unwrap_or_default())
                                    .text_size(px(12.0))
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
                    .text_size(px(12.0))
                    .text_color(palette.muted),
            ),
            view().child(
                when(
                    move || !state.backup_dir.get().trim().is_empty(),
                    move || {
                        text(move || state.backup_dir.get())
                            .text_size(px(12.0))
                            .text_color(palette.fg)
                            .test_id("data-backup-dir-value")
                    },
                )
                .otherwise(move || {
                    view().child(
                        text(DIR_UNSET)
                            .text_size(px(12.0))
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
                            "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent",
                        )
                        .child(
                            text(move || {
                                if state.backup_dir.get().trim().is_empty() {
                                    DIR_CHOOSE
                                } else {
                                    DIR_CHANGE
                                }
                            })
                            .text_size(px(13.0))
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
                                    "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent",
                                )
                                .child(
                                    text(DIR_CLEAR)
                                        .text_size(px(13.0))
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
                        .text_size(px(13.0))
                        .text_color(palette.fg),
                )
                .child(
                    input()
                        .test_id("data-backup-max-count")
                        .value(move || state.keep_text.get())
                        .text_color(palette.fg)
                        .w(px(72.0))
                        .app_class("bg-background rounded-md border border-border px-2 py-1")
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
                        .text_size(px(11.0))
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
                        .text_size(px(13.0))
                        .text_color(palette.fg),
                ),
                view().child(
                    text(move || {
                        format!(
                            "{AUTO_LAST_LABEL}{}",
                            state.auto_last.get().map_or_else(|| AUTO_LAST_NEVER.to_owned(), |t| format_backup_time(&t))
                        )
                    })
                    .text_size(px(12.0))
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
                                    "rounded-md border border-destructive/30 bg-destructive/10 p-2 gap-1",
                                )
                                .child(
                                    text(move || {
                                        format!(
                                            "{AUTO_FAIL_ALERT_PREFIX}{}{AUTO_FAIL_ALERT_SUFFIX}",
                                            state.auto_failures.get()
                                        )
                                    })
                                    .text_size(px(12.0))
                                    .text_color(danger_fg())
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
                                    .text_size(px(12.0))
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
                                .text_size(px(12.0))
                                .text_color(danger_fg())
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
                            "px-2 py-0.5 rounded-md border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(move || state.rows_loading.get())
                        .child(
                            text(BUTTON_REFRESH)
                                .text_size(px(11.0))
                                .test_id("data-backup-list-refresh"),
                        )
                        .on_press(move |_: &()| refresh_rows_task(owner, state)),
                ),
            view().child(
                text(LIST_HINT)
                    .text_size(px(12.0))
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
                    .text_size(px(12.0))
                    .text_color(palette.fg)
                    .test_id("data-backup-count"),
                )
                .child(
                    button()
                        .app_class(
                            "px-3 py-1 rounded-md border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(move || state.rows.get().is_empty() || state.pruning.get())
                        .child(
                            text(BUTTON_PRUNE_NOW)
                                .text_size(px(12.0))
                                .test_id("data-backup-prune"),
                        )
                        .on_press(manual_prune),
                ),
            view().child(
                when(
                    move || state.rows_error.get().is_some(),
                    move || {
                        text(move || state.rows_error.get().unwrap_or_default())
                            .text_size(px(12.0))
                            .text_color(danger_fg())
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
                                .text_size(px(12.0))
                                .text_color(palette.muted)
                                .test_id("data-backup-empty")
                            },
                        )
                        .otherwise(move || {
                            view().flex_col().app_class("gap-1").children(
                                state
                                    .rows
                                    .get()
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, row)| backup_row(palette, index, row)),
                            )
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
                .app_class("rounded-md border border-destructive/30 bg-destructive/10 p-2 gap-1")
                .child(
                    view()
                        .flex_row()
                        .app_class("gap-1 flex-wrap")
                        .child(text(RESTORE_HINT_BEFORE).text_size(px(12.0)).text_color(palette.fg))
                        .child(
                            text(RESTORE_HINT_STRONG)
                                .text_size(px(12.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(danger_fg()),
                        )
                        .child(text(RESTORE_HINT_AFTER).text_size(px(12.0)).text_color(palette.fg)),
                ),
            view().child(
                button()
                    .app_class(
                        "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent disabled:opacity-40",
                    )
                    .disabled(move || state.restore_busy.get())
                    .child(
                        text(RESTORE_BUTTON)
                            .text_size(px(13.0))
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
                .text_size(px(12.0))
                .text_color(palette.fg)
                .test_id(format!("data-backup-row-{index}-name").as_str()),
        )
        .child(
            view()
                .flex_row()
                .app_class("items-center gap-2 flex-wrap")
                .child(
                    text(row.meta.clone())
                        .text_size(px(11.0))
                        .text_color(palette.muted)
                        .test_id(format!("data-backup-row-{index}-meta").as_str()),
                )
                .child(
                    button()
                        .app_class(
                            "px-2 py-0.5 rounded-md border border-border text-foreground hover:bg-accent",
                        )
                        .child(
                            text(REVEAL_LABEL)
                                .text_size(px(11.0))
                                .test_id(format!("data-backup-reveal-{index}").as_str()),
                        )
                        .on_press(move |_: &()| {
                            drop(reveal_path(path.clone()));
                        }),
                ),
        )
}

// ---------------------------------------------------------------------------
// 加密卡（明文库形态的开启流；密文库形态本壳不可达——偏差注记在屏）。
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct EncryptionState {
    /// 库文件密文形态（文件即真相；NSH 常态 = false）。
    file_encrypted: Signal<bool>,
    /// 状态读取失败（错误位 + 重试）。
    status_error: Signal<Option<String>>,
    /// 主口令与确认。
    pass: Signal<String>,
    confirm: Signal<String>,
    /// 开启加密确认弹窗开着。
    confirm_open: Signal<bool>,
    /// 执行中（单飞）。
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
                    .set(Some(format!("{ENCRYPT_STATUS_FAILED_PREFIX}{e}"))),
                Err(_) => state.status_error.set(Some(READ_TASK_FAILED.to_owned())),
            }
        })
    });
}

/// 开启加密执行任务（确认弹窗先关再执行——确认弹窗不是重试现场；整库转换
/// 直调基建单点，失败原库原样保留）：成功反馈如实携带本壳密文启动暂不支
/// 持的后果（旧壳自动重启 + 解锁屏的偏差在句内披露）。
fn run_enable_encryption(owner: Owner, state: EncryptionState) {
    let passphrase = state.pass.get_untracked();
    state.busy.set(true);
    owner.run(move || {
        spawn(async move {
            let db = crate::app::db_slot().clone();
            let pass = passphrase;
            let outcome = blocking(move || match db.as_ref() {
                Some(db) => boot::enable_encryption(db, &pass),
                None => Err(ledger_infra::error::AppError::coded(
                    boot::CODE_SESSION_MISSING,
                    SESSION_MISSING,
                )),
            })
            .await;
            state.busy.set(false);
            match outcome {
                Ok(Ok(())) => {
                    state.pass.set(String::new());
                    state.confirm.set(String::new());
                    state.status.set(Some(ENCRYPT_OK_HOST_HINT.to_owned()));
                    refresh_encryption_status(owner, state);
                }
                Ok(Err(e)) => state
                    .status
                    .set(Some(format!("{ENCRYPT_FAILED_PREFIX}{e}"))),
                Err(_) => state
                    .status
                    .set(Some(format!("{ENCRYPT_FAILED_PREFIX}{WRITE_TASK_FAILED}"))),
            }
        })
    });
}

fn encryption_card(palette: views::ViewPalette) -> View {
    let owner = Owner::current();
    let state = EncryptionState {
        file_encrypted: signal(false),
        status_error: signal(None),
        pass: signal(String::new()),
        confirm: signal(String::new()),
        confirm_open: signal(false),
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
                .text_size(px(12.0))
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
                        move || view().child(encrypted_form(state)),
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
                .text_size(px(12.0))
                .text_color(danger_fg())
                .test_id("enc-status-error"),
        )
        .child(
            button()
                .app_class(
                    "px-2 py-0.5 rounded-md border border-border text-foreground hover:bg-accent",
                )
                .child(text(ENCRYPT_RETRY).text_size(px(12.0)).test_id("enc-retry"))
                .on_press(move |_: &()| refresh_encryption_status(retry_owner, state)),
        )
}

/// 密文库形态呈现（「已开启」警示块 + 本壳不可达形态的偏差注记）。
fn encrypted_form(state: EncryptionState) -> impl IntoElement {
    let _ = state;
    view()
        .flex_col()
        .app_class("gap-2")
        .child(
            view()
                .flex_col()
                .app_class("rounded-md border border-destructive/30 bg-destructive/10 p-2 gap-1")
                .child(
                    text(ENCRYPT_ENABLED_LOCAL_TITLE)
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(views::view_palette().fg)
                        .test_id("enc-enabled-title"),
                )
                .child(
                    text(ENCRYPT_ENABLED_LOCAL_BODY)
                        .text_size(px(12.0))
                        .text_color(views::view_palette().fg),
                ),
        )
        .child(
            text(ENCRYPT_PLAIN_ONLY_NOTE)
                .text_size(px(11.0))
                .text_color(views::view_palette().muted)
                .test_id("enc-plain-only-note"),
        )
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
                .app_class("rounded-md border border-destructive/30 bg-destructive/10 p-2 gap-1")
                .child(
                    text(ENCRYPTION_WARN_TITLE)
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(danger_fg())
                        .test_id("enc-warn-title"),
                )
                .child(
                    text(ENCRYPTION_WARN_BODY)
                        .text_size(px(12.0))
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
                .text_size(px(11.0))
                .text_color(danger_fg())
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
                    .text_size(px(11.0))
                    .text_color(danger_fg())
                    .test_id("enc-mismatch")
            },
        ))
        .child(
            button()
                .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90 disabled:opacity-40")
                .disabled(move || !enable_ready(state))
                .child(
                    text(ENCRYPT_BUTTON)
                        .text_size(px(13.0))
                        .text_color(rgb(0xffffff))
                        .test_id("enc-enable"),
                )
                .on_press(move |_: &()| request_enable()),
        )
}

// ---------------------------------------------------------------------------
// 多端同步卡（状态回显 + 立即同步 + 通道配置表单 + 保存前测试连接 + 检查
// 点发布与新端引导向导；同步写语义不经本壳）。
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct SyncState {
    /// 状态回显（Some = 已装载）。
    status: Signal<Option<boot::SyncStatusSeam>>,
    /// 状态读取失败反馈。
    status_error: Signal<Option<String>>,
    /// 挂起明细行集。
    parked: Signal<Vec<ledger_sync_engine::ParkedOp>>,
    /// 动作反馈状态条位。
    status_msg: Signal<Option<String>>,
    /// 手动同步执行中。
    syncing: Signal<bool>,
    /// 密文库同步主口令（明文库不消费、不在屏）。
    sync_pass: Signal<String>,
    // 通道表单（S3 七字段 + 同步空间 + 寻址方式；密钥回显口径 = 不回显）。
    endpoint: Signal<String>,
    region: Signal<String>,
    bucket: Signal<String>,
    prefix: Signal<String>,
    access_key: Signal<String>,
    secret_saved: Signal<String>,
    path_style: Signal<bool>,
    space_id: Signal<String>,
    secret_input: Signal<String>,
    /// 厂商下拉判别键（回显按端点反查；自定义哨兵恒末项）。
    vendor: Signal<Option<String>>,
    /// 厂商变更的净信号（回显装载两值同设，不触发预填）。
    last_vendor: Signal<String>,
    /// 保存执行中。
    saving: Signal<bool>,
    /// 测试连接执行中。
    testing: Signal<bool>,
    /// 发布检查点执行中。
    publishing: Signal<bool>,
    /// 发布明文标记（快照明文上通道）。
    publish_plaintext: Signal<bool>,
    /// 引导向导开着。
    bootstrap_open: Signal<bool>,
    /// 引导预检执行中。
    prechecking: Signal<bool>,
    /// 引导预检失败反馈（弹窗错误位）。
    precheck_error: Signal<Option<String>>,
    /// 通道检查点信息（generation, size）。
    checkpoint: Signal<Option<(i64, u64)>>,
    /// 引导主口令输入。
    bootstrap_pass: Signal<String>,
    /// 引导执行中（兼重入守卫）。
    bootstrapping: Signal<bool>,
    /// 引导弹窗错误位（就地重试）。
    bootstrap_error: Signal<Option<String>>,
}

/// 表单快照 → 命令入参（保存与测试连接的单一转换点：密钥输入有内容用新
/// 值，留空沿用已存值；同步空间 blank 归默认——归一在 boot 接缝单点）。
fn channel_payload(state: SyncState) -> SyncChannelConfig {
    SyncChannelConfig {
        space_id: state.space_id.get_untracked().trim().to_owned(),
        endpoint: state.endpoint.get_untracked(),
        region: state.region.get_untracked(),
        bucket: state.bucket.get_untracked(),
        prefix: state.prefix.get_untracked(),
        access_key: state.access_key.get_untracked(),
        secret_key: {
            let typed = state.secret_input.get_untracked();
            if typed.is_empty() {
                state.secret_saved.get_untracked()
            } else {
                typed
            }
        },
        path_style: state.path_style.get_untracked(),
    }
}

/// 同步状态刷新（状态回显 + 挂起数量随行）。
fn refresh_sync_status(owner: Owner, state: SyncState) {
    owner.run(move || {
        spawn(async move {
            let db = crate::app::db_slot().clone();
            let outcome = blocking(move || match db.as_ref() {
                Some(db) => boot::sync_status(db),
                None => Err(ledger_infra::error::AppError::coded(
                    boot::CODE_SESSION_MISSING,
                    SESSION_MISSING,
                )),
            })
            .await;
            match outcome {
                Ok(Ok(next)) => {
                    state.status.set(Some(next));
                    state.status_error.set(None);
                }
                Ok(Err(e)) => state
                    .status_error
                    .set(Some(format!("{SYNC_STATUS_FAILED_PREFIX}{e}"))),
                Err(_) => state.status_error.set(Some(READ_TASK_FAILED.to_owned())),
            }
        })
    });
}

/// 通道配置回显装载（表单初值 = 已存配置；未配置 = 空表单）。密钥输入恒
/// 空白起填（不回显完整密钥，#1218）；厂商判别键按端点反查（两值同设 =
/// 回显不触发预填）。删此装载接线 → 表单空 / 状态假默认 → 红。
fn load_channel_config(owner: Owner, state: SyncState) {
    owner.run(move || {
        spawn(async move {
            let db = crate::app::db_slot().clone();
            let outcome = blocking(move || match db.as_ref() {
                Some(db) => boot::sync_channel_config(db),
                None => Err(ledger_infra::error::AppError::coded(
                    boot::CODE_SESSION_MISSING,
                    SESSION_MISSING,
                )),
            })
            .await;
            match outcome {
                Ok(Ok(Some(config))) => {
                    let vendor = match_vendor_by_endpoint(&config.endpoint).to_owned();
                    state.space_id.set(config.space_id);
                    state.endpoint.set(config.endpoint);
                    state.region.set(config.region);
                    state.bucket.set(config.bucket);
                    state.prefix.set(config.prefix);
                    state.access_key.set(config.access_key);
                    state.secret_saved.set(config.secret_key);
                    state.path_style.set(config.path_style);
                    state.secret_input.set(String::new());
                    state.vendor.set(Some(vendor.clone()));
                    state.last_vendor.set(vendor);
                }
                Ok(Ok(None)) => {
                    state.space_id.set(String::new());
                    state.vendor.set(Some(CUSTOM_VENDOR_ID.to_owned()));
                    state.last_vendor.set(String::new());
                }
                Ok(Err(e)) => state
                    .status_error
                    .set(Some(format!("{SYNC_STATUS_FAILED_PREFIX}{e}"))),
                Err(_) => state.status_error.set(Some(READ_TASK_FAILED.to_owned())),
            }
            refresh_sync_status(owner, state);
        })
    });
}

/// 保存通道配置任务（表单 → 接缝 → 成功后回显刷新）。
fn save_channel_task(owner: Owner, state: SyncState) {
    let config = channel_payload(state);
    state.saving.set(true);
    owner.run(move || {
        spawn(async move {
            let db = crate::app::db_slot().clone();
            let config = config;
            let outcome = blocking(move || match db.as_ref() {
                Some(db) => boot::save_sync_channel_config(db, config),
                None => Err(ledger_infra::error::AppError::coded(
                    boot::CODE_SESSION_MISSING,
                    SESSION_MISSING,
                )),
            })
            .await;
            state.saving.set(false);
            match outcome {
                Ok(Ok(())) => {
                    state.status_msg.set(Some(SYNC_SAVE_OK.to_owned()));
                    state.secret_input.set(String::new());
                    load_channel_config(owner, state);
                }
                Ok(Err(e)) => state
                    .status_msg
                    .set(Some(format!("{SYNC_SAVE_FAILED_PREFIX}{e}"))),
                Err(_) => state.status_msg.set(Some(format!(
                    "{SYNC_SAVE_FAILED_PREFIX}{WRITE_TASK_FAILED}"
                ))),
            }
        })
    });
}

/// 保存前测试连接任务（表单快照探测，零持久化副作用）。
fn test_connection_task(owner: Owner, state: SyncState) {
    let config = channel_payload(state);
    state.testing.set(true);
    owner.run(move || {
        spawn(async move {
            let outcome = blocking(move || boot::test_sync_connection(&config)).await;
            state.testing.set(false);
            match outcome {
                Ok(Ok(())) => state.status_msg.set(Some(SYNC_TEST_OK.to_owned())),
                Ok(Err(e)) => state
                    .status_msg
                    .set(Some(format!("{SYNC_TEST_FAILED_PREFIX}{e}"))),
                Err(_) => state.status_msg.set(Some(format!(
                    "{SYNC_TEST_FAILED_PREFIX}{WRITE_TASK_FAILED}"
                ))),
            }
        })
    });
}

/// 立即同步任务（手动轮次触发）：成功反馈轮次报告 + 挂起警示 + `applied`
/// 失效信号广播（重放是账本数据变化的入口之一；删除广播接线 → applied 后
/// 列表不重读 → headless 用例红）。
fn sync_now_task(owner: Owner, state: SyncState) {
    let passphrase =
        (!state.sync_pass.get_untracked().is_empty()).then_some(state.sync_pass.get_untracked());
    state.syncing.set(true);
    owner.run(move || spawn(async move {
        let db = crate::app::db_slot().clone();
        let outcome = blocking(move || match db.as_ref() {
            Some(db) => boot::sync_now(db, passphrase.as_deref()),
            None => Err(ledger_infra::error::AppError::coded(
                boot::CODE_SESSION_MISSING,
                SESSION_MISSING,
            )),
        })
        .await;
        state.syncing.set(false);
        match outcome {
            Ok(Ok(report)) => {
                if report.applied > 0 {
                    signals::notify_transactions_invalidated();
                    signals::notify_budgets_invalidated();
                    signals::notify_investments_invalidated();
                }
                let mut message = format!(
                    "{SYNC_OK_PREFIX}{}{SYNC_OK_MIDDLE_UP}{}{SYNC_OK_MIDDLE_APPLIED}{}{SYNC_OK_SUFFIX}",
                    report.uploaded_ops,
                    report.applied,
                    report.parked
                );
                if report.parked > 0 {
                    message.push_str(&format!(
                        " {SYNC_PARKED_PREFIX}{}{SYNC_PARKED_SUFFIX}",
                        report.parked
                    ));
                }
                state.status_msg.set(Some(message));
                refresh_sync_status(owner, state);
                refresh_parked_rows(owner, state);
            }
            Ok(Err(e)) => state.status_msg.set(Some(format!("{SYNC_FAILED_PREFIX}{e}"))),
            Err(_) => state.status_msg.set(Some(format!("{SYNC_FAILED_PREFIX}{WRITE_TASK_FAILED}"))),
        }
    }));
}

/// 挂起明细装载（失败不升级为卡片级错误：明细置空 + 告警，重试即下次刷新；
/// 旧壳刻意降级同口径）。
fn refresh_parked_rows(owner: Owner, state: SyncState) {
    owner.run(move || {
        spawn(async move {
            let db = crate::app::db_slot().clone();
            let outcome = blocking(move || match db.as_ref() {
                Some(db) => boot::parked_rows(db),
                None => Err(ledger_infra::error::AppError::coded(
                    boot::CODE_SESSION_MISSING,
                    SESSION_MISSING,
                )),
            })
            .await;
            match outcome {
                Ok(Ok(rows)) => state.parked.set(rows),
                Ok(Err(_)) | Err(_) => {
                    tracing::warn!("挂起明细拉取失败");
                    state.parked.set(Vec::new());
                }
            }
        })
    });
}

/// 发布检查点任务（快照产出 + 封包上传，域内编排；零信号零置脏）。
fn publish_checkpoint_task(owner: Owner, state: SyncState) {
    let passphrase =
        (!state.sync_pass.get_untracked().is_empty()).then_some(state.sync_pass.get_untracked());
    state.publishing.set(true);
    owner.run(move || {
        spawn(async move {
            let db = crate::app::db_slot().clone();
            let outcome = blocking(move || match db.as_ref() {
                Some(db) => boot::publish_sync_checkpoint(db, passphrase.as_deref()),
                None => Err(ledger_infra::error::AppError::coded(
                    boot::CODE_SESSION_MISSING,
                    SESSION_MISSING,
                )),
            })
            .await;
            state.publishing.set(false);
            match outcome {
                Ok(Ok(result)) => {
                    state.status_msg.set(Some(format!(
                        "{SYNC_PUBLISH_OK_PREFIX}{}{SYNC_PUBLISH_OK_MIDDLE}{}",
                        result.generation,
                        format_backup_size(result.size)
                    )));
                    state.publish_plaintext.set(result.plaintext_mode);
                }
                Ok(Err(e)) => state
                    .status_msg
                    .set(Some(format!("{SYNC_PUBLISH_FAILED_PREFIX}{e}"))),
                Err(_) => state.status_msg.set(Some(format!(
                    "{SYNC_PUBLISH_FAILED_PREFIX}{WRITE_TASK_FAILED}"
                ))),
            }
        })
    });
}

/// 引导向导打开任务（预检：只读 manifest 不下载快照体）。
fn open_bootstrap_task(owner: Owner, state: SyncState) {
    state.bootstrap_pass.set(String::new());
    state.checkpoint.set(None);
    state.precheck_error.set(None);
    state.bootstrap_open.set(true);
    state.prechecking.set(true);
    owner.run(move || {
        spawn(async move {
            let db = crate::app::db_slot().clone();
            let outcome = blocking(move || match db.as_ref() {
                Some(db) => boot::sync_channel_checkpoint(db),
                None => Err(ledger_infra::error::AppError::coded(
                    boot::CODE_SESSION_MISSING,
                    SESSION_MISSING,
                )),
            })
            .await;
            state.prechecking.set(false);
            match outcome {
                Ok(Ok(Some(pointer))) => state
                    .checkpoint
                    .set(Some((pointer.generation, pointer.size))),
                Ok(Ok(None)) => state.checkpoint.set(None),
                Ok(Err(e)) => state.precheck_error.set(Some(e.to_string())),
                Err(_) => state.precheck_error.set(Some(READ_TASK_FAILED.to_owned())),
            }
        })
    });
}

/// 引导确认任务（整库换入域内联点；成功关弹窗 + 重启提示反馈）。
fn confirm_bootstrap_task(owner: Owner, state: SyncState) {
    if state.checkpoint.get_untracked().is_none() || state.bootstrapping.get_untracked() {
        return;
    }
    let passphrase = (!state.bootstrap_pass.get_untracked().is_empty())
        .then_some(state.bootstrap_pass.get_untracked());
    state.bootstrapping.set(true);
    state.bootstrap_error.set(None);
    owner.run(move || {
        spawn(async move {
            let db = crate::app::db_slot().clone();
            let outcome = blocking(move || match db.as_ref() {
                Some(db) => boot::bootstrap_sync_from_channel(db, passphrase.as_deref()),
                None => Err(ledger_infra::error::AppError::coded(
                    boot::CODE_SESSION_MISSING,
                    SESSION_MISSING,
                )),
            })
            .await;
            state.bootstrapping.set(false);
            match outcome {
                Ok(Ok(result)) => {
                    state.bootstrap_open.set(false);
                    let mut message = format!(
                        "{SYNC_BOOTSTRAP_OK_HOST_HINT}（采纳第 {} 代快照 {}）",
                        result.generation,
                        format_backup_size(result.size)
                    );
                    if result.reencrypted {
                        message.push_str(SYNC_BOOTSTRAP_REENCRYPTED_HINT);
                    }
                    state.status_msg.set(Some(message));
                }
                Ok(Err(e)) => state
                    .bootstrap_error
                    .set(Some(format!("{SYNC_BOOTSTRAP_FAILED_PREFIX}{e}"))),
                Err(_) => state.bootstrap_error.set(Some(format!(
                    "{SYNC_BOOTSTRAP_FAILED_PREFIX}{WRITE_TASK_FAILED}"
                ))),
            }
        })
    });
}

/// 上次同步时刻展示文本（ISO → 分钟拆分；从未同步回「从未同步」）。
fn sync_last_text(state: SyncState) -> String {
    match state
        .status
        .get()
        .as_ref()
        .and_then(|s| s.last_sync_at.clone())
    {
        Some(iso) => format_backup_time(&iso),
        None => SYNC_NEVER.to_owned(),
    }
}

/// 挂起明细清单（逐条已渲染中文详情原样呈现）。
fn parked_list(palette: views::ViewPalette, state: SyncState) -> View {
    let rows = state.parked.get();
    let mut host = view().flex_col().app_class("gap-1").child(
        text(SYNC_PARKED_LIST_TITLE)
            .text_size(px(12.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette.fg),
    );
    for (index, op) in rows.into_iter().enumerate() {
        host = host.child(
            text(format!("{SYNC_PARKED_ITEM_PREFIX}{}", op.message))
                .text_size(px(11.0))
                .text_color(palette.muted)
                .test_id(format!("sync-parked-item-{index}").as_str()),
        );
    }
    host
}

/// 引导向导弹窗内容（警示块 + 预检态 + 口令输入 + 确认行）。
fn bootstrap_modal_body(state: SyncState, owner: Owner, palette: views::ViewPalette) -> View {
    let body_owner = owner;
    let busy = state.bootstrapping.get();
    let has_checkpoint = state.checkpoint.get().is_some();
    view()
        .flex_col()
        .app_class("gap-2")
        .test_id("sync-bootstrap-modal")
        .child(
            view()
                .flex_col()
                .app_class("rounded-md border border-amber-500/30 bg-amber-500/10 p-2 gap-1")
                .child(
                    text(SYNC_BOOTSTRAP_WARNING)
                        .text_size(px(12.0))
                        .text_color(palette.fg)
                        .test_id("sync-bootstrap-warning"),
                ),
        )
        .child(
            when(
                move || state.prechecking.get(),
                move || {
                    text(SYNC_BOOTSTRAP_PRECHECKING)
                        .text_size(px(12.0))
                        .text_color(palette.muted)
                        .test_id("sync-bootstrap-prechecking")
                },
            ),
        )
        .child(
            when(
                move || state.precheck_error.get().is_some(),
                move || {
                    text(move || state.precheck_error.get().unwrap_or_default())
                        .text_size(px(12.0))
                        .text_color(danger_fg())
                        .test_id("sync-bootstrap-precheck-error")
                },
            ),
        )
        .child(
            when(
                move || state.checkpoint.get().is_none(),
                move || {
                    text(SYNC_BOOTSTRAP_NOT_FOUND)
                        .text_size(px(12.0))
                        .text_color(palette.muted)
                        .test_id("sync-bootstrap-not-found")
                },
            ),
        )
        .child(
            when(
                move || state.checkpoint.get().is_some(),
                move || {
                    let (generation, size) = state.checkpoint.get_untracked().unwrap_or((0, 0));
                    text(format!(
                        "{SYNC_BOOTSTRAP_FOUND_PREFIX}{generation}{SYNC_BOOTSTRAP_FOUND_MIDDLE}{}",
                        format_backup_size(size)
                    ))
                    .text_size(px(12.0))
                    .text_color(palette.fg)
                    .test_id("sync-bootstrap-found")
                },
            ),
        )
        .child(pass_input(
            state.bootstrap_pass, palette, "sync-bootstrap-passphrase", "主口令", SYNC_BOOTSTRAP_PASS_PLACEHOLDER,
        ))
        .children(state.bootstrap_error.get().map(|err| {
            text(err.clone())
                .text_size(px(12.0))
                .text_color(danger_fg())
                .test_id("sync-bootstrap-error")
        }))
        .child(
            view()
                .flex_row()
                .justify_end()
                .app_class("gap-2")
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(busy)
                        .child(
                            text(SYNC_BOOTSTRAP_CANCEL)
                                .text_size(px(13.0))
                                .test_id("sync-bootstrap-cancel"),
                        )
                        .on_press(move |_: &()| {
                            if !state.bootstrapping.get_untracked() {
                                state.bootstrap_open.set(false);
                            }
                        }),
                )
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-md bg-amber-500 hover:opacity-90 disabled:opacity-40",
                        )
                        .disabled(busy || !has_checkpoint)
                        .child(
                            text(SYNC_BOOTSTRAP_CONFIRM)
                                .text_size(px(13.0))
                                .text_color(rgb(0xffffff))
                                .test_id("sync-bootstrap-confirm"),
                        )
                        .on_press(move |_: &()| confirm_bootstrap_task(body_owner, state)),
                ),
        )
}

fn sync_card(palette: views::ViewPalette) -> View {
    let owner = Owner::current();
    let state = SyncState {
        status: signal(None),
        status_error: signal(None),
        parked: signal(Vec::new()),
        status_msg: signal(None),
        syncing: signal(false),
        sync_pass: signal(String::new()),
        endpoint: signal(String::new()),
        region: signal(String::new()),
        bucket: signal(String::new()),
        prefix: signal(String::new()),
        access_key: signal(String::new()),
        secret_saved: signal(String::new()),
        path_style: signal(false),
        space_id: signal(String::new()),
        secret_input: signal(String::new()),
        vendor: signal(Some(CUSTOM_VENDOR_ID.to_owned())),
        last_vendor: signal(String::new()),
        saving: signal(false),
        testing: signal(false),
        publishing: signal(false),
        publish_plaintext: signal(false),
        bootstrap_open: signal(false),
        prechecking: signal(false),
        precheck_error: signal(None),
        checkpoint: signal(None),
        bootstrap_pass: signal(String::new()),
        bootstrapping: signal(false),
        bootstrap_error: signal(None),
    };

    load_channel_config(owner, state);

    // 厂商变更预填（净信号 = vendor 与 last_vendor 分叉；回显装载两值同设，
    // 不触发预填——预填会覆盖用户已存配置）。删除预填充点接线 → 选中厂商
    // 字段不预填 → headless 用例红。
    effect(move || {
        let Some(vendor) = state.vendor.get() else {
            return;
        };
        if vendor == state.last_vendor.get() {
            return;
        }
        state.last_vendor.set(vendor.clone());
        if let Some((endpoint, region, path_style)) = vendor_prefill(&vendor, None) {
            state.endpoint.set(endpoint);
            state.region.set(region);
            state.path_style.set(path_style);
        }
    });

    let configured = move || state.status.get().is_some_and(|s| s.channel_configured);

    // 状态区（明文警示 / 密文提示 + 上次同步 / 挂起数量与明细 + 立即同步）。
    let status_section = view()
        .flex_col()
        .app_class("gap-2")
        .child(when(
            move || state.status.get().is_some_and(|s| !s.library_encrypted),
            move || {
                view()
                    .flex_col()
                    .app_class("rounded-md border border-amber-500/30 bg-amber-500/10 p-2 gap-1")
                    .child(
                        text(SYNC_PLAINTEXT_WARNING)
                            .text_size(px(12.0))
                            .text_color(palette.fg)
                            .test_id("sync-plaintext-warning"),
                    )
            },
        ))
        .child(when(
            move || state.status.get().is_some_and(|s| s.library_encrypted),
            move || {
                text(SYNC_ENCRYPTED_HINT)
                    .text_size(px(12.0))
                    .text_color(palette.fg)
                    .test_id("sync-encrypted-hint")
            },
        ))
        .child(
            text(move || format!("{SYNC_LAST_LABEL}{}", sync_last_text(state)))
                .text_size(px(12.0))
                .text_color(palette.fg)
                .test_id("sync-last-time"),
        )
        .child(
            text(move || {
                format!(
                    "{SYNC_PARKED_LABEL}{}",
                    state.status.get().map_or(0, |s| s.parked_count)
                )
            })
            .text_size(px(12.0))
            .text_color(palette.fg)
            .test_id("sync-parked-count"),
        )
        .child(when(
            move || state.status.get().is_some_and(|s| s.parked_count > 0),
            move || {
                text(SYNC_PARKED_HINT)
                    .text_size(px(11.0))
                    .text_color(palette.muted)
                    .test_id("sync-parked-hint")
            },
        ))
        .child(when(
            move || !state.parked.get().is_empty(),
            move || parked_list(palette, state),
        ))
        .child(
            view().flex_row().app_class("gap-2").child(
                button()
                    .app_class(
                        "px-3 py-1.5 rounded-md bg-primary hover:opacity-90 disabled:opacity-40",
                    )
                    .disabled(move || state.syncing.get() || state.status_error.get().is_some())
                    .child(
                        text(SYNC_NOW)
                            .text_size(px(13.0))
                            .text_color(rgb(0xffffff))
                            .test_id("sync-now"),
                    )
                    .on_press(move |_: &()| sync_now_task(owner, state)),
            ),
        )
        .child(when(
            move || state.status.get().is_some_and(|s| s.library_encrypted),
            move || {
                pass_input(
                    state.sync_pass,
                    palette,
                    "sync-passphrase",
                    SYNC_PASS_LABEL,
                    SYNC_PASS_PLACEHOLDER,
                )
            },
        ));

    // 通道配置表单（厂商下拉 + S3 七字段 + 同步空间 + 测试 / 保存）。
    let channel_section = view()
        .flex_col()
        .app_class("gap-2")
        .child(
            text(SYNC_CHANNEL_TITLE)
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.fg),
        )
        .child(select::app_select(
            "sync-vendor",
            vendor_options,
            state.vendor,
            SYNC_VENDOR_CUSTOM,
            false,
        ))
        .child(
            text(SYNC_VENDOR_HINT)
                .text_size(px(11.0))
                .text_color(palette.muted)
                .test_id("sync-vendor-hint"),
        )
        .child(field_input(palette, "sync-endpoint", SYNC_ENDPOINT_PLACEHOLDER, state.endpoint, false, None::<fn() -> &'static str>))
        .child(field_input(palette, "sync-region", SYNC_REGION_PLACEHOLDER, state.region, false, None::<fn() -> &'static str>))
        .child(field_input(palette, "sync-bucket", SYNC_BUCKET_PLACEHOLDER, state.bucket, false, None::<fn() -> &'static str>))
        .child(field_input(palette, "sync-prefix", SYNC_PREFIX_PLACEHOLDER, state.prefix, false, None::<fn() -> &'static str>))
        .child(field_input(palette, "sync-access-key", SYNC_ACCESS_KEY_PLACEHOLDER, state.access_key, false, None::<fn() -> &'static str>))
        .child(field_input(
            palette,
            "sync-secret-key",
            SYNC_SECRET_PLACEHOLDER,
            state.secret_input,
            true,
            Some(move || {
                if state.secret_saved.get().is_empty() {
                    SYNC_SECRET_PLACEHOLDER
                } else {
                    SYNC_SECRET_SAVED_PLACEHOLDER
                }
            }),
        ))
        .child(
            // 寻址方式行（开关 + 标签；行按压替身承载 headless 锚点，
            // feature_row 同形）。
            view()
                .test_id("sync-path-style")
                .role(nsh_core::AccessibilityRole::Button)
                .optional_label(SYNC_PATH_STYLE_LABEL)
                .app_class("rounded-md items-center")
                .on_press(move |_: &PressEvent| state.path_style.set(!state.path_style.get_untracked()))
                .child(
                    view()
                        .flex_row()
                        .app_class("items-center gap-2")
                        .child(
                            nsh_base::switch()
                                .accessibility_label(SYNC_PATH_STYLE_LABEL)
                                .checked(move || state.path_style.get())
                                .on_change(move |value: &bool| state.path_style.set(value.to_owned()))
                                .app_class("shrink-0"),
                        )
                        .child(
                            text(SYNC_PATH_STYLE_LABEL)
                                .text_size(px(12.0))
                                .text_color(palette.fg),
                        ),
                ),
        )
        .child(field_input(palette, "sync-space", SYNC_SPACE_PLACEHOLDER, state.space_id, false, None::<fn() -> &'static str>))
        .child(
            text(SYNC_SPACE_HINT)
                .text_size(px(11.0))
                .text_color(palette.muted)
                .test_id("sync-space-hint"),
        )
        .child(
            view()
                .flex_row()
                .app_class("gap-2")
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(move || state.testing.get())
                        .child(
                            text(SYNC_TEST_CONNECTION)
                                .text_size(px(13.0))
                                .test_id("sync-test-connection"),
                        )
                        .on_press(move |_: &()| test_connection_task(owner, state)),
                )
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-md bg-primary hover:opacity-90 disabled:opacity-40",
                        )
                        .disabled(move || state.saving.get())
                        .child(
                            text(SYNC_SAVE_CHANNEL)
                                .text_size(px(13.0))
                                .text_color(rgb(0xffffff))
                                .test_id("sync-save-channel"),
                        )
                        .on_press(move |_: &()| save_channel_task(owner, state)),
                ),
        );

    // 检查点与新端加入（发布 + 向导入口）。
    let checkpoint_section = view()
        .flex_col()
        .app_class("gap-2")
        .child(
            text(SYNC_CHECKPOINT_TITLE)
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.fg),
        )
        .child(
            text(SYNC_CHECKPOINT_HINT)
                .text_size(px(11.0))
                .text_color(palette.muted),
        )
        .child(
            view()
                .flex_row()
                .app_class("gap-2")
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(move || !configured() || state.publishing.get())
                        .child(
                            text(SYNC_PUBLISH)
                                .text_size(px(13.0))
                                .test_id("sync-publish-checkpoint"),
                        )
                        .on_press(move |_: &()| publish_checkpoint_task(owner, state)),
                )
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(move || !configured())
                        .child(
                            text(SYNC_BOOTSTRAP_BUTTON)
                                .text_size(px(13.0))
                                .test_id("sync-bootstrap"),
                        )
                        .on_press(move |_: &()| open_bootstrap_task(owner, state)),
                ),
        );

    let bootstrap_modal = app_modal(
        move || state.bootstrap_open.get(),
        || SYNC_BOOTSTRAP_MODAL_TITLE.to_owned(),
        move || {
            if !state.bootstrapping.get_untracked() {
                state.bootstrap_open.set(false);
            }
        },
        move || bootstrap_modal_body(state, owner, palette),
    );

    let body = view()
        .flex_col()
        .app_class("gap-2")
        .child(
            when(
                move || state.status_error.get().is_some(),
                move || {
                    text(move || state.status_error.get().unwrap_or_default())
                        .text_size(px(12.0))
                        .text_color(danger_fg())
                        .test_id("sync-status-error")
                },
            )
            .otherwise(move || view().child(status_section.clone())),
        )
        .child(channel_section)
        .child(checkpoint_section)
        .child(settings::status_line(
            "sync-status",
            state.status_msg,
            palette,
        ))
        .child(when(
            move || state.publish_plaintext.get(),
            move || {
                text(SYNC_PUBLISH_PLAINTEXT)
                    .text_size(px(11.0))
                    .text_color(palette.muted)
                    .test_id("sync-publish-plaintext")
            },
        ));

    settings::card(CARD_SYNC, palette, vec![body]).child(bootstrap_modal)
}

// ---------------------------------------------------------------------------
// 账本切换卡（清单 + 跨账本汇总置顶入口 + 行级改名 / 移除 + 新建 + 切换）。
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct BooksState {
    /// 清单行集（最新落盘态读注册表）。
    books: Signal<Vec<ledger_infra::db::book_registry::Book>>,
    /// 活动账本 id。
    active: Signal<Option<String>>,
    /// 登记变更可用性（可变 = 注册表可读且无推迟搬迁窗口）。
    mutable: Signal<bool>,
    /// 回退警示（回退原因随行）。
    fallback_reason: Signal<Option<String>>,
    /// 状态条位（动作成功 / 失败反馈）。
    status: Signal<Option<String>>,
    /// 命名弹窗意图（新建 / 改名）。
    name_intent: Signal<Option<BookNameIntent>>,
    /// 命名输入草稿。
    name_draft: Signal<String>,
    /// 命名提交执行中。
    name_busy: Signal<bool>,
    /// 切换确认待决目标（id, name）。
    switch_pending: Signal<Option<(String, String)>>,
    /// 切换执行中（在途拒绝再次发起）。
    switching: Signal<bool>,
    /// 移除气泡确认的开合（行级 id = 已开气泡的行）。
    popconfirm_open: Signal<Option<String>>,
}

/// 命名弹窗意图（旧壳 BookNameIntent 闭集同形）。
#[derive(Clone, Debug, PartialEq)]
enum BookNameIntent {
    /// 新建。
    Create,
    /// 改名（携带目标账本行快照）。
    Rename(ledger_infra::db::book_registry::Book),
}

/// 清单装载（聚合读注册表最新落盘态；本壳默认目录 = 启动参数账本目录）。
/// 删装载接线 → 清单永远空态 → 红。
fn refresh_books(owner: Owner, state: BooksState) {
    owner.run(move || {
        spawn(async move {
            let launch = book_dir();
            let outcome = blocking(move || match launch {
                Some(launch) => Ok(boot::book_list(&launch)),
                None => Err(ledger_infra::error::AppError::coded(
                    "book.registry-unavailable",
                    BOOKS_SESSION_UNAVAILABLE,
                )),
            })
            .await;
            match outcome {
                Ok(Ok(info)) => {
                    state.books.set(info.books.clone());
                    state.active.set(info.active_id.clone());
                    state.mutable.set(info.mutable);
                    state.fallback_reason.set(info.fallback_reason.clone());
                }
                Ok(Err(_)) | Err(_) => {
                    state.status.set(Some(format!(
                        "{BOOKS_ACTION_FAILED_PREFIX}{BOOKS_SESSION_UNAVAILABLE}"
                    )));
                }
            }
        })
    });
}

/// 账本切换卡（清单 + 汇总入口 + 行操作 + 新建 + 切换确认）。
fn books_card(palette: views::ViewPalette) -> View {
    let owner = Owner::current();
    let state = BooksState {
        books: signal(Vec::new()),
        active: signal(None),
        mutable: signal(false),
        fallback_reason: signal(None),
        status: signal(None),
        name_intent: signal(None),
        name_draft: signal(String::new()),
        name_busy: signal(false),
        switch_pending: signal(None),
        switching: signal(false),
        popconfirm_open: signal(None),
    };

    refresh_books(owner, state);

    // 跨账本汇总置顶入口（非账本条目、仅导航——不动活动指针；消费 nav 单
    // 点接缝不自建；汇总视图本体已随㉔就位，注册表不可达 / 投资关闭时不可用）。
    let open_summary = move || {
        nav::open(NavRequest::bare(nav_model::ViewName::CrossBookSummary));
    };
    let summary_available = move || {
        state.mutable.get()
            && state.fallback_reason.get().is_none()
            && !nav::store().is_closed(nav_model::ViewName::Investments)
    };

    // 行级动作入口。
    let open_create = move || {
        state.name_draft.set(String::new());
        state.name_intent.set(Some(BookNameIntent::Create));
    };
    let open_rename = move |book: ledger_infra::db::book_registry::Book| {
        state.name_draft.set(book.name.clone());
        state.name_intent.set(Some(BookNameIntent::Rename(book)));
    };
    let switch_request = move |book: ledger_infra::db::book_registry::Book| {
        let is_active = state.active.get_untracked().as_deref() == Some(book.id.as_str());
        if is_active || !state.mutable.get_untracked() || state.switching.get_untracked() {
            return;
        }
        state
            .switch_pending
            .set(Some((book.id.clone(), book.name.clone())));
    };

    // 切换确认对话框（轻量确认语义，warning 级；确认后写活动指针并反馈重
    // 启提示——本壳无原位重引导，删除切换写链 → 指针不写 → 旅程用例红）。
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
                            BOOKS_SESSION_UNAVAILABLE,
                        )),
                    })
                    .await;
                    state.switching.set(false);
                    match outcome {
                        Ok(Ok(book)) => {
                            state.status.set(Some(format!(
                                "{BOOKS_SWITCHING_PREFIX}{}{BOOKS_SWITCHING_SUFFIX}",
                                book.name
                            )));
                            refresh_books(owner, state);
                        }
                        Ok(Err(e)) => state
                            .status
                            .set(Some(format!("{BOOKS_ACTION_FAILED_PREFIX}{e}"))),
                        Err(_) => state.status.set(Some(format!(
                            "{BOOKS_ACTION_FAILED_PREFIX}{WRITE_TASK_FAILED}"
                        ))),
                    }
                })
            });
        };
        danger_confirm(
            move || state.switch_pending.get().is_some(),
            DangerConfirmSpec {
                level: DangerLevel::Warning,
                title: BOOKS_SWITCH_TITLE.to_owned(),
                lead: None,
                strong_warning: None,
                detail: Some(BOOKS_SWITCH_DETAIL.to_owned()),
                confirm_label: BOOKS_SWITCH_CONFIRM.to_owned(),
                cancel_label: BOOKS_CANCEL.to_owned(),
            },
            on_outcome,
        )
    };

    // 移除动作（行级气泡确认的确认回调；仅摘登记，目录与库文件原样保留）。
    let remove_row = {
        let (state, owner) = (state, owner);
        move |book: ledger_infra::db::book_registry::Book| {
            owner.run(move || {
                spawn(async move {
                    let launch = book_dir();
                    let outcome = blocking(move || match launch {
                        Some(launch) => boot::remove_book(&launch, &book.id),
                        None => Err(ledger_infra::error::AppError::coded(
                            "book.registry-unavailable",
                            BOOKS_SESSION_UNAVAILABLE,
                        )),
                    })
                    .await;
                    match outcome {
                        Ok(Ok(())) => {
                            state.status.set(Some(format!(
                                "{BOOKS_REMOVED_PREFIX}{}{BOOKS_REMOVED_SUFFIX}",
                                book.name
                            )));
                            refresh_books(owner, state);
                        }
                        Ok(Err(e)) => state
                            .status
                            .set(Some(format!("{BOOKS_ACTION_FAILED_PREFIX}{e}"))),
                        Err(_) => state.status.set(Some(format!(
                            "{BOOKS_ACTION_FAILED_PREFIX}{WRITE_TASK_FAILED}"
                        ))),
                    }
                })
            });
        }
    };

    // 命名弹窗提交（新建 / 改名共用；空名不提交，失败保持打开可改）。
    let submit_name = {
        let (state, owner) = (state, owner);
        move || {
            let Some(intent) = state.name_intent.get_untracked() else {
                return;
            };
            let name = state.name_draft.get_untracked().trim().to_owned();
            if name.is_empty() || state.name_busy.get_untracked() {
                return;
            }
            state.name_busy.set(true);
            owner.run(move || {
                spawn(async move {
                    let launch = book_dir();
                    let is_create = matches!(intent, BookNameIntent::Create);
                    let outcome = blocking(move || match launch {
                        Some(launch) => match intent {
                            BookNameIntent::Create => {
                                boot::create_book(launch.as_path(), name.as_str())
                            }
                            BookNameIntent::Rename(book) => {
                                boot::rename_book(launch.as_path(), &book.id, name.as_str())
                            }
                        },
                        None => Err(ledger_infra::error::AppError::coded(
                            "book.registry-unavailable",
                            BOOKS_SESSION_UNAVAILABLE,
                        )),
                    })
                    .await;
                    state.name_busy.set(false);
                    match outcome {
                        Ok(Ok(book)) => {
                            state.name_intent.set(None);
                            state.status.set(Some(if is_create {
                                format!("{BOOKS_CREATED_PREFIX}{}{BOOKS_CREATED_SUFFIX}", book.name)
                            } else {
                                format!("{BOOKS_RENAMED_PREFIX}{}{BOOKS_RENAMED_SUFFIX}", book.name)
                            }));
                            refresh_books(owner, state);
                        }
                        Ok(Err(e)) => state
                            .status
                            .set(Some(format!("{BOOKS_SET_FAILED_PREFIX}{e}"))),
                        Err(_) => state.status.set(Some(format!(
                            "{BOOKS_SET_FAILED_PREFIX}{WRITE_TASK_FAILED}"
                        ))),
                    }
                })
            });
        }
    };

    // 命名弹窗（旧壳 ModalIntent 二闭集同形；标题 / 确认文案随意图切换）。
    let name_modal = app_modal(
        move || state.name_intent.get().is_some(),
        move || match state.name_intent.get_untracked() {
            Some(BookNameIntent::Rename(_)) => BOOKS_RENAME_TITLE.to_owned(),
            _ => BOOKS_CREATE_TITLE.to_owned(),
        },
        move || {
            if !state.name_busy.get_untracked() {
                state.name_intent.set(None);
            }
        },
        move || {
            let confirm_label = match state.name_intent.get_untracked() {
                Some(BookNameIntent::Rename(_)) => BOOKS_RENAME_CONFIRM,
                _ => BOOKS_CREATE_CONFIRM,
            };
            let submit_row = view()
                .flex_row()
                .justify_end()
                .app_class("gap-2")
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(move || state.name_busy.get())
                        .child(
                            text(BOOKS_CANCEL)
                                .text_size(px(13.0))
                                .test_id("book-name-cancel"),
                        )
                        .on_press(move |_: &()| {
                            if !state.name_busy.get_untracked() {
                                state.name_intent.set(None);
                            }
                        }),
                )
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-md bg-primary hover:opacity-90 disabled:opacity-40",
                        )
                        .disabled(move || {
                            state.name_busy.get() || state.name_draft.get().trim().is_empty()
                        })
                        .child(
                            text(confirm_label)
                                .text_size(px(13.0))
                                .text_color(rgb(0xffffff))
                                .test_id("book-name-submit"),
                        )
                        .on_press(move |_: &()| submit_name()),
                );
            view()
                .flex_col()
                .app_class("gap-2")
                .child(
                    text(BOOKS_NAME_LABEL)
                        .text_size(px(12.0))
                        .text_color(palette.fg),
                )
                .child(
                    input()
                        .test_id("book-name-input")
                        .value(move || state.name_draft.get())
                        .placeholder(BOOKS_CREATE_PLACEHOLDER)
                        .optional_label(BOOKS_NAME_LABEL)
                        .text_color(palette.fg)
                        .w(px(260.0))
                        .app_class(
                            "bg-background rounded-md border border-border px-2 py-1 placeholder:text-muted-foreground",
                        )
                        .on_change(move |value: &str| state.name_draft.set(value.to_owned())),
                )
                .child(submit_row)
        },
    );

    // 卡体（回退警示 + 汇总入口 + 清单行 + 新建 + 状态条位）。
    let body = view()
        .flex_col()
        .app_class("gap-2")
        .child(
            when(
                move || state.fallback_reason.get().is_some(),
                move || {
                    view()
                        .flex_col()
                        .app_class("rounded-md border border-amber-500/30 bg-amber-500/10 p-2 gap-1")
                        .child(
                            text(BOOKS_FALLBACK_TITLE)
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette.fg)
                                .test_id("book-fallback-title"),
                        )
                        .child(
                            text(move || state.fallback_reason.get().unwrap_or_default())
                                .text_size(px(12.0))
                                .text_color(palette.fg),
                        )
                },
            ),
        )
        .child(
            button()
                .app_class(
                    "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent disabled:opacity-40",
                )
                .disabled(move || !summary_available())
                .child(
                    text(BOOKS_ENTRY_SUMMARY)
                        .text_size(px(13.0))
                        .test_id("book-summary-entry"),
                )
                .on_press(move |_: &()| open_summary()),
        )
        .child(each(
            move || state.books.get(),
            // key = id + 名称：组件均一次体内静态文案（如移除气泡确认中容
            // 携带行名），改名后同名 id 旧模版不重渲染；key 含名称让改名即
            // 重建行（极少频登记变更，重建代价可忽略）。
            |book: &ledger_infra::db::book_registry::Book| {
                format!("{}:{}", book.id, book.name)
            },
            move |item: ReadSignal<ledger_infra::db::book_registry::Book>,
                  _index: ReadSignal<usize>| {
                let is_active = move || {
                    state.active.get().as_deref() == Some(item.get().id.as_str())
                };
                book_row_view(
                    palette,
                    state,
                    _index.get(),
                    item,
                    BookRowActions {
                        is_active,
                        open_rename,
                        switch_request,
                        remove_row,
                    },
                )
            },
        ))
        .child(
            view().flex_row().app_class("items-center gap-2").child(
                button()
                    .app_class(
                        "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent disabled:opacity-40",
                    )
                    .disabled(move || !state.mutable.get())
                    .child(text(BOOKS_NEW).text_size(px(13.0)).test_id("book-create"))
                    .on_press(move |_: &()| open_create()),
            ),
        )
        .child(settings::status_line("book-status", state.status, palette));

    settings::card(CARD_BOOKS, palette, vec![body])
        .child(switch_dialog)
        .child(name_modal)
}

/// 账本行（行按压切换意图 + 当前标记 + 改名 / 移除〔非活动本，气泡确认〕；
/// 行内容按注册表 key 各建一次，内部状态读走响应式闭包）。
/// 行级动作闭包组（[`book_row_view`] 的消费面；Copy 闭包打包成单参数）。
#[derive(Clone, Copy)]
struct BookRowActions<
    A: Fn() -> bool + Copy + 'static,
    B: Fn(ledger_infra::db::book_registry::Book) + Copy + 'static,
    C: Fn(ledger_infra::db::book_registry::Book) + Copy + 'static,
    D: Fn(ledger_infra::db::book_registry::Book) + Copy + 'static,
> {
    is_active: A,
    open_rename: B,
    switch_request: C,
    remove_row: D,
}

fn book_row_view(
    palette: views::ViewPalette,
    state: BooksState,
    index: usize,
    book: ReadSignal<ledger_infra::db::book_registry::Book>,
    actions: BookRowActions<
        impl Fn() -> bool + Copy + 'static,
        impl Fn(ledger_infra::db::book_registry::Book) + Copy + 'static,
        impl Fn(ledger_infra::db::book_registry::Book) + Copy + 'static,
        impl Fn(ledger_infra::db::book_registry::Book) + Copy + 'static,
    >,
) -> View {
    let pop_id = book.get_untracked().id.clone();
    let row_id = pop_id.clone();
    let popconfirm_state = state;
    let pop_id_confirm = row_id.clone();
    let access_disabled = move || !state.mutable.get();
    // 行锚点（旧壳 .book-row click 同位）：按压 = 切换意图请求（活动本在请
    // 求闭包内挡掉）。无障碍角色按钮（name = 账本名）。
    let name_anchor = view()
        .test_id(format!("book-switch-{row_id}").as_str())
        .role(nsh_core::AccessibilityRole::Button)
        .optional_label(move || Some(SharedString::from(book.get().name.clone())))
        .app_class("rounded-md min-w-0")
        .on_press(move |_: &PressEvent| (actions.switch_request)(book.get_untracked().clone()))
        .child(
            text(move || book.get().name.clone())
                .text_size(px(13.0))
                .text_color(move || {
                    if (actions.is_active)() {
                        palette.fg
                    } else {
                        palette.muted
                    }
                })
                .test_id(format!("book-row-{index}-name").as_str()),
        );
    let mut row = view()
        .flex_row()
        .app_class("items-center gap-2 py-1 border-b border-border/60");
    row = row.child(name_anchor);
    row = row.child(when(
        move || (actions.is_active)(),
        move || {
            text(BOOKS_PANEL_CURRENT)
                .text_size(px(11.0))
                .text_color(palette.fg)
                .test_id(format!("book-row-{index}-current").as_str())
        },
    ));
    row = row.child(
        view()
            .flex_row()
            .app_class("items-center gap-1")
            .child(
                button()
                    .app_class(
                        "px-2 py-0.5 rounded-md border border-border text-foreground hover:bg-accent disabled:opacity-40",
                    )
                    .disabled(access_disabled)
                    .child(
                        text(BOOKS_RENAME)
                            .text_size(px(11.0))
                            .test_id(format!("book-rename-{row_id}").as_str()),
                    )
                    .on_press(move |_: &()| (actions.open_rename)(book.get_untracked().clone())),
            ),
    );
    row = row.child(
        when(
            move || !(actions.is_active)() && state.mutable.get(), // 移除入口与登记可用性同门（旧壳 disabled 同口径）
            move || {
                popconfirm::popconfirm(
                    "book-remove",
                    {
                        let pop_id_open = pop_id_confirm.clone();
                        move || {
                            popconfirm_state
                                .popconfirm_open
                                .get()
                                .as_deref()
                                == Some(pop_id_open.as_str())
                        }
                    },
                    {
                        let pop_id = pop_id_confirm.clone();
                        move |shown: bool| {
                            popconfirm_state
                                .popconfirm_open
                                .set(shown.then(|| pop_id.clone()));
                        }
                    },
                    popconfirm::PopconfirmSpec {
                        message: format!(
                            "{BOOKS_REMOVE_CONFIRM_PREFIX}{}{BOOKS_REMOVE_CONFIRM_SUFFIX}",
                            book.get().name
                        ),
                        confirm_label: BOOKS_REMOVE_ACTION.to_owned(),
                        cancel_label: BOOKS_CANCEL.to_owned(),
                    },
                    // 触发件用纯视图（nsh-base Button 自含按压语义会吞掉外
                        // 层按压，预算删除先例同判；封装的按压源是外层包裹视
                        // 图）。
                        view()
                            .app_class(
                                "px-2 py-0.5 rounded-md border border-border text-foreground hover:bg-accent",
                            )
                            .child(
                                text(BOOKS_REMOVE)
                                    .text_size(px(11.0))
                                    .test_id(format!("book-remove-{row_id}").as_str()),
                            ),
                    move || (actions.remove_row)(book.get_untracked().clone()),
                )
            },
        ),
    );
    row
}

/// 数据安全四卡入口（数据页签消费面）。
#[component]
pub fn DataSafetyCards() -> impl IntoElement {
    let palette = views::view_palette();
    view()
        .flex_col()
        .app_class("gap-3")
        .child(backup_cards(palette))
        .child(encryption_card(palette))
        .child(sync_card(palette))
        .child(books_card(palette))
}

// ---------------------------------------------------------------------------
// lib 单测：装配层纯面（大小 / 时刻格式化、来源 / 设备词表、厂商预设闭集
// 与反查）。headless 组件测试见 tests/app_headless.rs 设置②节。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 大小格式化闭集：B / KB（一位小数）/ MB（一位小数）三段。
    #[test]
    fn backup_size_covers_unit_closure() {
        assert_eq!(format_backup_size(512), "512 B");
        assert_eq!(format_backup_size(2048), "2.0 KB");
        assert_eq!(format_backup_size(1536 * 1024), "1.5 MB");
    }

    /// 时刻格式化：ISO 剥到分钟（YYYY-MM-DD HH:mm）；形状不符原样透传。
    #[test]
    fn backup_time_projects_iso_minute() {
        assert_eq!(
            format_backup_time("2026-01-01T10:20:30Z"),
            "2026-01-01 10:20"
        );
        assert_eq!(format_backup_time("非ISO形状"), "非ISO形状");
    }

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

    /// 厂商下拉闭集：五预设按声明序 + 末尾「其他（自定义）」；档位标注未实测。
    #[test]
    fn vendor_options_cover_closed_set() {
        let options = vendor_options();
        let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "aliyun-oss",
                "tencent-cos",
                "huawei-obs",
                "volcengine-tos",
                "qiniu-kodo",
                CUSTOM_VENDOR_ID,
            ]
        );
        assert!(
            options.iter().take(5).all(|o| o.label.contains("未实测")),
            "未实测厂商一律如实标注（verified 唯一翻转点在预设表字段）"
        );
        assert_eq!(options[0].label, "阿里云 OSS（未实测）");
        assert_eq!(options[5].label, SYNC_VENDOR_CUSTOM);
    }

    /// 预填与反查：模板地域替换 / 默认地域回退 / 自定义不预填；反查「服务段
    /// 前缀 + 品牌后缀」两端同时成立（虚拟托管 bucket 前缀两级都试）。
    #[test]
    fn vendor_prefill_and_endpoint_match_follow_contract() {
        let (endpoint, region, path_style) = vendor_prefill("tencent-cos", None).unwrap();
        assert_eq!(endpoint, "https://cos.ap-guangzhou.myqcloud.com");
        assert_eq!(region, "ap-guangzhou");
        assert!(!path_style);
        let (endpoint, region, _) = vendor_prefill("tencent-cos", Some("ap-beijing")).unwrap();
        assert_eq!(endpoint, "https://cos.ap-beijing.myqcloud.com");
        assert_eq!(region, "ap-beijing");
        assert!(vendor_prefill(CUSTOM_VENDOR_ID, None).is_none());
        assert_eq!(
            match_vendor_by_endpoint("https://obs.cn-north-4.myhuaweicloud.com"),
            "huawei-obs"
        );
        // 虚拟托管形态：bucket 是主机名最左一段，去掉后两级都试。
        assert_eq!(
            match_vendor_by_endpoint("https://bucket.obs.cn-north-4.myhuaweicloud.com"),
            "huawei-obs"
        );
        // 同品牌非对象存储服务不误判（华为云 ECS 的服务前缀不在预设表）。
        assert_eq!(
            match_vendor_by_endpoint("https://ecs.cn-north-4.myhuaweicloud.com"),
            CUSTOM_VENDOR_ID
        );
        assert_eq!(
            match_vendor_by_endpoint("https://s3.example.com"),
            CUSTOM_VENDOR_ID
        );
        assert_eq!(match_vendor_by_endpoint(""), CUSTOM_VENDOR_ID);
    }
}
