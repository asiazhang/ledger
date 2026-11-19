//! 多端同步卡（数据页签卡组；旧壳 `SyncSettings` + `useSyncCard` 的重生，
//! ADR-0091，#2167 自 [`crate::settings_data`] 按职责拆出）：明文警示 / 状
//! 态回显（上次同步 / 挂起数量与明细）、立即同步（成功 → 轮次报告反馈 +
//! applied 失效信号广播）、通道配置表单（S3 七字段 + 同步空间 + 厂商预设
//! 预填 / 端点反查回显 + 密钥不回显口径）与保存前「测试连接」、检查点发布
//! / 新端引导向导。同步写语义不经本壳（轮次 / 发布 / 引导全在域内单点编
//! 排，本壳只解包口令与触发，票面实施决策）；不依赖同步协议 crate（壳只
//! 依赖域，ADR-0112）。
//!
//! S3 厂商预设为本卡私有展示层数据与纯函数（旧壳 s3-vendors 预设，宿主已随 Web 前端退役 #2171/#2172；
//! 的移植）：预设只预填端点模板 / 寻址 / 常用地域并按端点反查回显，不落
//! 库、不进后端契约。「verified」唯一翻转点在 [`S3_VENDOR_PRESETS`] 字段。
//!
//! 共用呈现件（密文输入行 / 表单字段行）与备份产物格式化装配纯函数消费
//! [`crate::settings_data`]；阻塞 IO（状态读、同步轮次、探测、引导换库）
//! 一律 `blocking` 后台执行，信号读写都在引擎 UI 线程。弹窗条件段机制选型
//! 布尔有无 → `when`（#2146 定案，模块族注记见 [`crate::settings_data`]）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use ledger_sync_engine::SyncChannelConfig;
use nsh_base::button;
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::assembly::CategoryOption;
use crate::design::font_size;
use crate::modal::app_modal;
use crate::settings_data::{field_input, format_backup_size, format_backup_time, pass_input};
use crate::shell_msgs::{MSG_READ_TASK_FAILED, MSG_WRITE_TASK_FAILED};
use crate::write_load::session_missing_err;
use crate::{boot, select, settings, signals, views};

const CARD_SYNC: &str = "多端同步";
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
const SYNC_BOOTSTRAP_OK_HOST_HINT: &str =
    "引导完成（本机账本数据已整库替换）：请重启应用载入引导数据";
const SYNC_BOOTSTRAP_REENCRYPTED_HINT: &str =
    "；本机已转为密文库：重启后需凭主口令解锁（即引导快照的主口令）";

// ---------------------------------------------------------------------------
// S3 厂商预设（旧壳 s3-vendors 的移植，宿主已随 Web 前端退役 #2171/#2172；纯展示层数据与纯函
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
                None => Err(session_missing_err()),
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
                Err(_) => state
                    .status_error
                    .set(Some(MSG_READ_TASK_FAILED.to_owned())),
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
                None => Err(session_missing_err()),
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
                Err(_) => state
                    .status_error
                    .set(Some(MSG_READ_TASK_FAILED.to_owned())),
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
                None => Err(session_missing_err()),
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
                    "{SYNC_SAVE_FAILED_PREFIX}{MSG_WRITE_TASK_FAILED}"
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
                    "{SYNC_TEST_FAILED_PREFIX}{MSG_WRITE_TASK_FAILED}"
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
            None => Err(session_missing_err()),
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
            Err(_) => state.status_msg.set(Some(format!("{SYNC_FAILED_PREFIX}{MSG_WRITE_TASK_FAILED}"))),
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
                None => Err(session_missing_err()),
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
                None => Err(session_missing_err()),
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
                    "{SYNC_PUBLISH_FAILED_PREFIX}{MSG_WRITE_TASK_FAILED}"
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
                None => Err(session_missing_err()),
            })
            .await;
            state.prechecking.set(false);
            match outcome {
                Ok(Ok(Some(pointer))) => state
                    .checkpoint
                    .set(Some((pointer.generation, pointer.size))),
                Ok(Ok(None)) => state.checkpoint.set(None),
                Ok(Err(e)) => state.precheck_error.set(Some(e.to_string())),
                Err(_) => state
                    .precheck_error
                    .set(Some(MSG_READ_TASK_FAILED.to_owned())),
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
                None => Err(session_missing_err()),
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
                    "{SYNC_BOOTSTRAP_FAILED_PREFIX}{MSG_WRITE_TASK_FAILED}"
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
            .text_size(px(font_size::CAPTION))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette.fg),
    );
    for (index, op) in rows.into_iter().enumerate() {
        host = host.child(
            text(format!("{SYNC_PARKED_ITEM_PREFIX}{}", op.message))
                .text_size(px(font_size::CAPTION))
                .text_color(palette.muted)
                .test_id(format!("sync-parked-item-{index}").as_str()),
        );
    }
    host
}

/// 引导向导弹窗内容（警示块 + 预检态 + 口令输入 + 确认行）。
fn bootstrap_modal_body(state: SyncState, owner: Owner, palette: views::ViewPalette) -> View {
    let body_owner = owner;
    let has_checkpoint = state.checkpoint.get().is_some();
    view()
        .flex_col()
        .app_class("gap-2")
        .test_id("sync-bootstrap-modal")
        .child(
            view()
                .flex_col()
                .app_class("rounded-sm border border-warning/30 bg-warning/10 p-2 gap-1")
                .child(
                    text(SYNC_BOOTSTRAP_WARNING)
                        .text_size(px(font_size::CAPTION))
                        .text_color(palette.fg)
                        .test_id("sync-bootstrap-warning"),
                ),
        )
        .child(
            when(
                move || state.prechecking.get(),
                move || {
                    text(SYNC_BOOTSTRAP_PRECHECKING)
                        .text_size(px(font_size::CAPTION))
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
                        .text_size(px(font_size::CAPTION))
                        .text_color(views::view_palette().danger_fg)
                        .test_id("sync-bootstrap-precheck-error")
                },
            ),
        )
        .child(
            when(
                move || state.checkpoint.get().is_none(),
                move || {
                    text(SYNC_BOOTSTRAP_NOT_FOUND)
                        .text_size(px(font_size::CAPTION))
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
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.fg)
                    .test_id("sync-bootstrap-found")
                },
            ),
        )
        .child(pass_input(
            state.bootstrap_pass, palette, "sync-bootstrap-passphrase", "主口令", SYNC_BOOTSTRAP_PASS_PLACEHOLDER,
        ))
        // 错误行随弹窗开着期间的信号变化（引导失败置错误）：布尔有无 →
        // when（模块头选型标准 #2146；when 分支闭包 untracked 求值、静态
        // children 开弹即冻结，失败错误行不呈现）——同弹窗 precheck_error
        // 同款 when 形态（与恢复确认弹窗错误位同款修复；无 headless 验证
        // 面〔同步引导旅程需通道桩，headless 面未覆盖〕，机械同款修复）。
        .child(when(
            move || state.bootstrap_error.get().is_some(),
            move || {
                text(move || state.bootstrap_error.get().unwrap_or_default())
                    .text_size(px(font_size::CAPTION))
                    .text_color(views::view_palette().danger_fg)
                    .test_id("sync-bootstrap-error")
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
                        .disabled(move || state.bootstrapping.get())
                        .child(
                            text(SYNC_BOOTSTRAP_CANCEL)
                                .text_size(px(font_size::SECONDARY))
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
                            "px-3 py-1.5 rounded-(--radius) bg-warning hover:opacity-90 disabled:opacity-40",
                        )
                        .disabled(move || state.bootstrapping.get() || !has_checkpoint)
                        .child(
                            text(SYNC_BOOTSTRAP_CONFIRM)
                                .text_size(px(font_size::SECONDARY))
                                // 低频专用配对前景保留 theme 单点直读（#2153
                                // 边界：warning-foreground 不入 palette 闭集，
                                // 正当边界见 ViewPalette 文档）。
                                .text_color(crate::theme::color("warning-foreground"))
                                .test_id("sync-bootstrap-confirm"),
                        )
                        .on_press(move |_: &()| confirm_bootstrap_task(body_owner, state)),
                ),
        )
}

pub(crate) fn sync_card(palette: views::ViewPalette) -> View {
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
                    .app_class("rounded-sm border border-warning/30 bg-warning/10 p-2 gap-1")
                    .child(
                        text(SYNC_PLAINTEXT_WARNING)
                            .text_size(px(font_size::CAPTION))
                            .text_color(palette.fg)
                            .test_id("sync-plaintext-warning"),
                    )
            },
        ))
        .child(when(
            move || state.status.get().is_some_and(|s| s.library_encrypted),
            move || {
                text(SYNC_ENCRYPTED_HINT)
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.fg)
                    .test_id("sync-encrypted-hint")
            },
        ))
        .child(
            text(move || format!("{SYNC_LAST_LABEL}{}", sync_last_text(state)))
                .text_size(px(font_size::CAPTION))
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
            .text_size(px(font_size::CAPTION))
            .text_color(palette.fg)
            .test_id("sync-parked-count"),
        )
        .child(when(
            move || state.status.get().is_some_and(|s| s.parked_count > 0),
            move || {
                text(SYNC_PARKED_HINT)
                    .text_size(px(font_size::CAPTION))
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
                        "px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90 disabled:opacity-40",
                    )
                    .disabled(move || state.syncing.get() || state.status_error.get().is_some())
                    .child(
                        text(SYNC_NOW)
                            .text_size(px(font_size::SECONDARY))
                            .text_color(palette.primary_fg)
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
                .text_size(px(font_size::SECONDARY))
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
                .text_size(px(font_size::CAPTION))
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
                .app_class("rounded-(--radius) items-center")
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
                                .text_size(px(font_size::CAPTION))
                                .text_color(palette.fg),
                        ),
                ),
        )
        .child(field_input(palette, "sync-space", SYNC_SPACE_PLACEHOLDER, state.space_id, false, None::<fn() -> &'static str>))
        .child(
            text(SYNC_SPACE_HINT)
                .text_size(px(font_size::CAPTION))
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
                            "px-3 py-1.5 rounded-(--radius) border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(move || state.testing.get())
                        .child(
                            text(SYNC_TEST_CONNECTION)
                                .text_size(px(font_size::SECONDARY))
                                .test_id("sync-test-connection"),
                        )
                        .on_press(move |_: &()| test_connection_task(owner, state)),
                )
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90 disabled:opacity-40",
                        )
                        .disabled(move || state.saving.get())
                        .child(
                            text(SYNC_SAVE_CHANNEL)
                                .text_size(px(font_size::SECONDARY))
                                .text_color(palette.primary_fg)
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
                .text_size(px(font_size::SECONDARY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.fg),
        )
        .child(
            text(SYNC_CHECKPOINT_HINT)
                .text_size(px(font_size::CAPTION))
                .text_color(palette.muted),
        )
        .child(
            view()
                .flex_row()
                .app_class("gap-2")
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-(--radius) border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(move || !configured() || state.publishing.get())
                        .child(
                            text(SYNC_PUBLISH)
                                .text_size(px(font_size::SECONDARY))
                                .test_id("sync-publish-checkpoint"),
                        )
                        .on_press(move |_: &()| publish_checkpoint_task(owner, state)),
                )
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-(--radius) border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(move || !configured())
                        .child(
                            text(SYNC_BOOTSTRAP_BUTTON)
                                .text_size(px(font_size::SECONDARY))
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
                        .text_size(px(font_size::CAPTION))
                        .text_color(views::view_palette().danger_fg)
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
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.muted)
                    .test_id("sync-publish-plaintext")
            },
        ));

    settings::card(CARD_SYNC, palette, vec![body]).child(bootstrap_modal)
}

// ---------------------------------------------------------------------------
// lib 单测：厂商预设闭集与预填 / 反查契约。headless 组件测试见
// tests/app_headless.rs 设置②节。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
