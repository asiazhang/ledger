//! 通道层（issue #859 / ADR-0091 决策 1/8/9）：哑通道上的目录布局、清单
//! （manifest）与同步轮次——把本域已有的引擎接缝（op 产出/幂等重放/位点/
//! Checkpoint）接到网盘字节世界上。
//!
//! 目录布局（wiki §7.3，按设备分流 + 分段追加 + 小 manifest）：
//!
//! ```text
//! <同步根>/book-<账本ID>/            # 每账本一份独立世界（ADR-0089 延伸）
//! ├── manifest.json                 # 各流段清单（序号区间+hash）+ Checkpoint 指针
//! ├── checkpoint/cp-<代>.enc        # 检查点快照，按代独立文件，写新换指针
//! └── streams/<DeviceId>/seg-<起>-<止>.enc
//!                                   # 每来源设备一流，仅该设备可写，其余端只读
//! ```
//!
//! 一致性纪律（WebDAV 无跨设备文件锁）：
//! - **按设备分流是正确性要求**：并发写冲突在物理上被流归属排除——每端只写
//!   自己 `streams/<DeviceId>/` 下的文件，对他人目录只读；段文件按内容时钟
//!   区间命名、永不覆盖他人段（op 只增不改，重传同段内容确定等同）。
//! - **manifest 是唯一小而整体替换的文件**：读最新远端 → 归并（自己流以本地
//!   为权威、他人流原样保留）→ 整体替换写入；并发整体替换的丢失更新由下一
//!   轮次再发布自愈。段/检查点条目携带尺寸与 SHA-256，下载后校验（内容自
//!   校验兜底弱原子性），不依赖锁；清单自身无副本回退——撕裂写被 JSON 解析
//!   拦下（`manifest-corrupt` 显性失败，不静默按空清单处理），修复路径 = 删
//!   清单文件后各端下轮重发布自愈（自己流重传内容确定等同，他人流随其下轮
//!   归并回归）。
//! - **拉取粒度为整段**（设计补充「段内偏移续读」的显性偏离）：信封是整包
//!   AEAD，密文分段解密无法验证完整性——偏移续读与信封认证不相容；位点落
//!   在段中间（挂起 op 钉住）时整段重拉、靠引擎幂等重放跳过已应用 op，多传
//!   字节以段容量为界（单文件永远有界）。
//! - **轮次顺序**：先发布（他人尽早可见，网络中断时已上传进度不回退）后拉取
//!   （按本端位点跳过已覆盖段）；manifest 只在归并结果变化时写回。
//!
//! 加密（ADR-0091 决策 8）：段与检查点逐文件封包（SyncEnvelope，[`envelope`]），
//! 模式由调用方按本库加密形态决定（复用备份域加密模式/主口令；#862 壳层
//! 接线）；未开加密模式为明文直通并在轮次报告中标记（界面显著提示依据）。
//! manifest 本身是纯元数据（设备 id、序号区间、密文 hash），两个模式下都不
//! 封包，保证弱原子性下的归并始终可读。
//!
//! 调用契约：`publish_checkpoint*` 须在单连接互斥锁内调用（[`checkpoint::
//! create_checkpoint`] 的位点/快照同刻成对约束，#862 轮次形态消费）。

use std::collections::BTreeMap;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::now_iso;
use crate::error::{AppError, Result};

use super::checkpoint::{self, Checkpoint};
use super::device;
use super::engine;
use super::envelope::{self, EnvelopeMode, EnvelopeParams};
use super::model::SyncOp;
use super::ops;
use super::positions;
use super::transport::Transport;

/// manifest 当前版本（未知更高版本拒绝解析：旧端收到新形态清单，提示升级）。
const MANIFEST_VERSION: u32 = 1;

/// 通道布局：按账本隔离的全部路径单点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelLayout {
    /// `book-<id>` 目录名（id 清洗为 URL 安全字符）。
    book_dir: String,
}

impl ChannelLayout {
    /// 从同步空间标识构造布局（`book-<space>`；空间是跨端共识的世界身份，
    /// 见多端同步域 Transport 词条——issue #862）：非法字符清洗，清洗后为空
    /// 拒绝（用户可见错误，文案对齐设置页「同步空间」字段语义）。
    pub fn new(book_id: &str) -> Result<Self> {
        let sanitized: String = book_id
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
            .collect();
        if sanitized.is_empty() {
            return Err(AppError::coded(
                "sync-channel.book-id-invalid",
                "同步空间非法（仅限小写字母、数字与短横线），无法构造同步目录",
            ));
        }
        Ok(Self {
            book_dir: format!("book-{sanitized}"),
        })
    }

    /// 账本目录（`book-<id>`）。
    pub fn book_dir(&self) -> String {
        self.book_dir.clone()
    }

    /// manifest 路径。
    pub fn manifest_path(&self) -> String {
        format!("{}/manifest.json", self.book_dir)
    }

    /// 检查点目录。
    pub fn checkpoint_dir(&self) -> String {
        format!("{}/checkpoint", self.book_dir)
    }

    /// 设备流根目录。
    pub fn streams_dir(&self) -> String {
        format!("{}/streams", self.book_dir)
    }

    /// 指定来源设备的流目录（仅该设备可写，其余端只读）。
    pub fn stream_dir(&self, device_id: &str) -> String {
        format!("{}/{}", self.streams_dir(), device_id)
    }

    /// 流目录下指定文件路径（manifest 段名 → 通道地址的单一出口）。
    pub fn stream_file_path(&self, device_id: &str, file: &str) -> String {
        format!("{}/{}", self.stream_dir(device_id), file)
    }

    /// 段文件路径（按 op 序号区间命名，永不与他人段重名冲突）。
    pub fn segment_path(&self, device_id: &str, first_clock: i64, last_clock: i64) -> String {
        self.stream_file_path(device_id, &segment_file_name(first_clock, last_clock))
    }

    /// 检查点目录下指定文件路径（manifest 指针 → 通道地址的单一出口）。
    pub fn checkpoint_file_path(&self, file: &str) -> String {
        format!("{}/{}", self.checkpoint_dir(), file)
    }

    /// 检查点文件路径（按代独立文件）。
    pub fn checkpoint_path(&self, generation: i64) -> String {
        format!(
            "{}/{}",
            self.checkpoint_dir(),
            checkpoint_file_name(generation)
        )
    }
}

/// 段文件名：`seg-<起>:010>-<止:010>.enc`。
fn segment_file_name(first_clock: i64, last_clock: i64) -> String {
    format!("seg-{first_clock:010}-{last_clock:010}.enc")
}

/// 检查点文件名：`cp-<代:06>.enc`。
fn checkpoint_file_name(generation: i64) -> String {
    format!("cp-{generation:06}.enc")
}

/// 通道清单（manifest.json）：各流段清单 + 当前 Checkpoint 指针。
///
/// 设备列表即各流条目的 `device_id` 集合（不单列设备清单：空流设备对拉取
/// 不可见，单列徒增一种待归并状态）。字段仅通道元数据——设备 id、序号区间、
/// 密文 hash——不含账本数据，明文存放。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelManifest {
    /// 清单格式版本。
    pub version: u32,
    /// 各来源设备的流清单（按 device_id 稳定序）。
    pub streams: Vec<StreamManifest>,
    /// 当前检查点指针（尚未发布过检查点时为空）。
    pub checkpoint: Option<CheckpointPointer>,
}

impl Default for ChannelManifest {
    fn default() -> Self {
        Self {
            version: MANIFEST_VERSION,
            streams: Vec::new(),
            checkpoint: None,
        }
    }
}

impl ChannelManifest {
    /// 从通道字节解析（缺失/损坏/版本未知均有明确码化错误，不静默按空处理）。
    fn parse(bytes: &[u8]) -> Result<Self> {
        let manifest: Self =
            serde_json::from_slice(bytes).map_err(|e| manifest_corrupt_error(&e.to_string()))?;
        if manifest.version != MANIFEST_VERSION {
            return Err(AppError::codedp(
                "sync-channel.manifest-version-newer",
                format!(
                    "同步清单来自更新版本的应用（清单版本 {}），请升级后再同步",
                    manifest.version
                ),
                &[manifest.version.to_string().as_str()],
            ));
        }
        Ok(manifest.normalize())
    }

    /// 序列化为通道字节（pretty JSON：弱原子性下便于人读排障）。
    fn serialize(&self) -> Result<Vec<u8>> {
        serde_json::to_vec_pretty(self)
            .map_err(|e| AppError::Invalid(format!("清单序列化失败: {e}")))
    }

    /// 稳定序归一：流按 device_id 升序、段按起始时钟升序（比较与写回同形，
    /// 避免顺序漂移触发无谓的整体替换）。
    fn normalize(mut self) -> Self {
        self.streams.sort_by(|a, b| a.device_id.cmp(&b.device_id));
        for stream in &mut self.streams {
            stream.segments.sort_by_key(|s| s.first_clock);
        }
        self
    }
}

/// 单个来源设备的流清单。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamManifest {
    /// 来源设备标识。
    pub device_id: String,
    /// 已上传段清单（按起始时钟序）。
    pub segments: Vec<SegmentEntry>,
}

impl StreamManifest {
    /// 流上已上传到的最大时钟（空流为 0——发布位点的通道视图）。
    fn uploaded_through(&self) -> i64 {
        self.segments.last().map(|s| s.last_clock).unwrap_or(0)
    }
}

/// 单个段文件的通道条目（内容自校验：尺寸 + SHA-256 兜底弱原子性）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentEntry {
    /// 段文件名（含 `seg-<起>-<止>.enc` 区间信息）。
    pub file: String,
    /// 段内最小 op 时钟。
    pub first_clock: i64,
    /// 段内最大 op 时钟。
    pub last_clock: i64,
    /// 密文字节数。
    pub size: u64,
    /// 密文 SHA-256（hex）。
    pub sha256: String,
}

/// 检查点指针（manifest 中当前代的引用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointPointer {
    /// 检查点文件名。
    pub file: String,
    /// 检查点代数（单调递增，自当前指针 +1）。
    pub generation: i64,
    /// 密文字节数。
    pub size: u64,
    /// 密文 SHA-256（hex）。
    pub sha256: String,
    /// 产出时刻（ISO；产出端本地事实，仅供排障展示）。
    pub created_at: String,
}

/// 单个同步轮次的共享上下文（恒结伴参数的聚合）。
struct RoundCtx<'a, 'p> {
    conn: &'a Connection,
    transport: &'a dyn Transport,
    layout: &'a ChannelLayout,
    mode: &'a EnvelopeMode<'p>,
    options: &'a ChannelOptions,
}

/// 通道轮次选项（段容量与封包参数；生产走默认值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelOptions {
    /// 单段最大 op 数（单文件永远有界，对网盘单文件限制与断点续传友好）。
    pub segment_max_ops: usize,
    /// 信封封包参数（KDF 迭代次数；见 [`envelope::EnvelopeParams`]）。
    pub envelope: EnvelopeParams,
}

impl Default for ChannelOptions {
    fn default() -> Self {
        Self {
            segment_max_ops: 2000,
            envelope: EnvelopeParams::default(),
        }
    }
}

/// 一次同步轮次的报告（触发编排与同步状态的消费形态，#862/#863 接线）。
///
/// `plaintext_mode` 为明文模式的显性标记：未开加密模式时界面须显著提示
/// （ADR-0091 决策 8；提示呈现归壳层）。Serialize 为 IPC wire 形态（#862
/// `sync_now` 响应体，前端据此轻量提示轮次结果）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct SyncRoundReport {
    /// 本轮上传段数。
    pub uploaded_segments: usize,
    /// 本轮上传 op 数。
    pub uploaded_ops: usize,
    /// 本轮下载并应用的段数。
    pub downloaded_segments: usize,
    /// 重放归宿计数：应用 / 期次去重 / LWW 压制 / 已知跳过 / 挂起。
    pub applied: usize,
    pub deduped: usize,
    pub superseded: usize,
    pub skipped: usize,
    pub parked: usize,
    /// 明文模式标记（界面显著提示依据）。
    pub plaintext_mode: bool,
}

/// 执行一次同步轮次（默认选项）：发布自己流新 op + 拉取他人流增量。
///
/// 失败即整体报错返回（凭据/网络/损坏均有码化错误），不产生部分静默状态；
/// 已上传的段与已应用的重放各自原子（重试轮次幂等续作），本地记账不受影响。
pub fn run_round(
    conn: &Connection,
    transport: &dyn Transport,
    layout: &ChannelLayout,
    mode: &EnvelopeMode<'_>,
) -> Result<SyncRoundReport> {
    run_round_with(conn, transport, layout, mode, &ChannelOptions::default())
}

/// 执行一次同步轮次（显式选项；测试注入低 KDF 迭代与段容量）。
pub fn run_round_with(
    conn: &Connection,
    transport: &dyn Transport,
    layout: &ChannelLayout,
    mode: &EnvelopeMode<'_>,
    options: &ChannelOptions,
) -> Result<SyncRoundReport> {
    let device_id = device::device_id(conn)?;
    transport.ensure_dir(&layout.book_dir())?;
    transport.ensure_dir(&layout.checkpoint_dir())?;
    transport.ensure_dir(&layout.streams_dir())?;

    let ctx = RoundCtx {
        conn,
        transport,
        layout,
        mode,
        options,
    };
    let remote = read_manifest(transport, layout)?;
    let mut report = SyncRoundReport {
        plaintext_mode: mode.is_plaintext(),
        ..SyncRoundReport::default()
    };

    // 发布：自己流新 op → 段（封包 → PUT）→ manifest 归并（自己流本地权威）。
    let mut manifest = remote.clone();
    publish_own_ops(&ctx, &device_id, &remote, &mut manifest, &mut report)?;

    // manifest 变化才整体替换写回（读侧轮次零写入）。
    if manifest != remote {
        transport.write_file(&layout.manifest_path(), &manifest.serialize()?)?;
    }

    // 拉取：他人流按本端位点跳过已覆盖段（manifest 视图），逐段校验 → 解封 → 应用。
    pull_foreign_streams(&ctx, &device_id, &manifest, &mut report)?;
    Ok(report)
}

/// 读取并解析通道 manifest（不存在按空清单；损坏/版本未知报码化错误）。
fn read_manifest(transport: &dyn Transport, layout: &ChannelLayout) -> Result<ChannelManifest> {
    match transport.read_file(&layout.manifest_path())? {
        None => Ok(ChannelManifest::default()),
        Some(bytes) => ChannelManifest::parse(&bytes),
    }
}

/// 发布自己流：自 manifest 上传位点（通道视图）之后的本机 op，按容量切段，
/// 逐段封包上传并归并进 manifest。
///
/// 上传位点取自 manifest 而非本地表——「清单写回失败」后重传同段内容确定
/// 等同（op 只增不改），天然幂等；本地不为此新增状态。
fn publish_own_ops(
    ctx: &RoundCtx<'_, '_>,
    device_id: &str,
    remote: &ChannelManifest,
    manifest: &mut ChannelManifest,
    report: &mut SyncRoundReport,
) -> Result<()> {
    let remote_own = remote
        .streams
        .iter()
        .find(|s| s.device_id == device_id)
        .cloned()
        .unwrap_or_else(|| StreamManifest {
            device_id: device_id.to_string(),
            segments: Vec::new(),
        });
    let own_ops = ops::read_own_since(ctx.conn, device_id, remote_own.uploaded_through())?;
    if own_ops.is_empty() {
        return Ok(());
    }
    // 归并基线：远端自己流段（同文件名幂等跳过）+ 本轮新增段。
    let mut segments: BTreeMap<String, SegmentEntry> = remote_own
        .segments
        .iter()
        .map(|s| (s.file.clone(), s.clone()))
        .collect();
    ctx.transport
        .ensure_dir(&ctx.layout.stream_dir(device_id))?;
    for chunk in split_chunks(&own_ops, ctx.options.segment_max_ops) {
        let first = chunk.first().map(|o| o.clock).unwrap_or(0);
        let last = chunk.last().map(|o| o.clock).unwrap_or(0);
        let file = segment_file_name(first, last);
        if segments.contains_key(&file) {
            continue;
        }
        let payload = serde_json::to_vec(&chunk)
            .map_err(|e| AppError::Invalid(format!("段载荷序列化失败: {e}")))?;
        let sealed = envelope::seal(&payload, ctx.mode, &ctx.options.envelope)?;
        ctx.transport
            .write_file(&ctx.layout.segment_path(device_id, first, last), &sealed)?;
        segments.insert(
            file.clone(),
            SegmentEntry {
                file,
                first_clock: first,
                last_clock: last,
                size: sealed.len() as u64,
                sha256: sha256_hex(&sealed),
            },
        );
        report.uploaded_segments += 1;
        report.uploaded_ops += chunk.len();
    }
    // 自己流以本地权威覆写（他人流在归并外原样保留）。
    let entry = StreamManifest {
        device_id: device_id.to_string(),
        segments: segments.into_values().collect(),
    };
    match manifest
        .streams
        .iter_mut()
        .find(|s| s.device_id == device_id)
    {
        Some(slot) => *slot = entry,
        None => manifest.streams.push(entry),
    }
    *manifest = manifest.clone().normalize();
    Ok(())
}

/// 拉取他人流：按本端位点跳过已覆盖段；段下载后校验尺寸与 hash，解封解析，
/// 经同步引擎幂等重放（LWW / 挂起 / 去重语义全在引擎，通道不重复裁决）。
fn pull_foreign_streams(
    ctx: &RoundCtx<'_, '_>,
    device_id: &str,
    manifest: &ChannelManifest,
    report: &mut SyncRoundReport,
) -> Result<()> {
    let passphrase = match ctx.mode {
        EnvelopeMode::Encrypted { passphrase } => Some(*passphrase),
        EnvelopeMode::Plaintext => None,
    };
    for stream in &manifest.streams {
        if stream.device_id == device_id {
            continue;
        }
        let position = positions::position_of(ctx.conn, &stream.device_id)?.unwrap_or(0);
        for segment in &stream.segments {
            if segment.last_clock <= position {
                continue; // 位点已覆盖整段：无需下载。
            }
            let path = ctx
                .layout
                .stream_file_path(&stream.device_id, &segment.file);
            let bytes = ctx
                .transport
                .read_file(&path)?
                .ok_or_else(|| segment_missing_error(&path))?;
            if bytes.len() as u64 != segment.size || sha256_hex(&bytes) != segment.sha256 {
                return Err(segment_corrupt_error(&path));
            }
            let payload = envelope::open(&bytes, passphrase)?;
            let incoming: Vec<SyncOp> = serde_json::from_slice(&payload)
                .map_err(|e| segment_corrupt_error(&format!("{path}: {}", e)))?;
            // 防御：段内 op 必须属于该流（错流即损坏，零丢失不允许串流）。
            if incoming.iter().any(|op| op.device_id != stream.device_id) {
                return Err(segment_corrupt_error(&path));
            }
            let reports = engine::apply_ops(ctx.conn, &incoming)?;
            for item in reports {
                match item.outcome {
                    super::OpOutcome::Applied => report.applied += 1,
                    super::OpOutcome::Deduped => report.deduped += 1,
                    super::OpOutcome::Superseded => report.superseded += 1,
                    super::OpOutcome::Skipped => report.skipped += 1,
                    super::OpOutcome::Parked { .. } => report.parked += 1,
                }
            }
            report.downloaded_segments += 1;
        }
    }
    Ok(())
}

/// 发布检查点：全量快照 + 位点成对封包上通道，manifest 换指针（写新文件 →
/// 完整上传 → 原子换指针；并发发布的代数撞号由 hash 校验兜底）。
///
/// 须在单连接互斥锁内调用（位点与快照同刻成对，见 [`checkpoint::
/// create_checkpoint`]）。
pub fn publish_checkpoint(
    conn: &Connection,
    transport: &dyn Transport,
    layout: &ChannelLayout,
    mode: &EnvelopeMode<'_>,
) -> Result<CheckpointPointer> {
    publish_checkpoint_with(conn, transport, layout, mode, &ChannelOptions::default())
}

/// 发布检查点（显式选项）。
pub fn publish_checkpoint_with(
    conn: &Connection,
    transport: &dyn Transport,
    layout: &ChannelLayout,
    mode: &EnvelopeMode<'_>,
    options: &ChannelOptions,
) -> Result<CheckpointPointer> {
    let checkpoint = checkpoint::create_checkpoint(conn)?;
    transport.ensure_dir(&layout.checkpoint_dir())?;

    let remote = read_manifest(transport, layout)?;
    let generation = remote
        .checkpoint
        .as_ref()
        .map(|p| p.generation)
        .unwrap_or(0)
        + 1;
    let sealed = envelope::seal(
        &frame_checkpoint_bundle(&checkpoint)?,
        mode,
        &options.envelope,
    )?;
    transport.write_file(&layout.checkpoint_path(generation), &sealed)?;
    let pointer = CheckpointPointer {
        file: checkpoint_file_name(generation),
        generation,
        size: sealed.len() as u64,
        sha256: sha256_hex(&sealed),
        created_at: now_iso(),
    };
    let mut manifest = remote.clone();
    manifest.checkpoint = Some(pointer.clone());
    if manifest != remote {
        transport.write_file(&layout.manifest_path(), &manifest.serialize()?)?;
    }
    tracing::info!(
        book = %layout.book_dir(),
        generation,
        size = pointer.size,
        streams = checkpoint.positions.len(),
        "检查点已发布到同步通道"
    );
    Ok(pointer)
}

/// 拉取所得的检查点：检查点本体 + 通道快照的封包形态标记。
///
/// `sealed` 为真表示通道上的快照是密文信封（源库为加密形态）——引导端据此
/// 对齐本库加密形态：明文库引导密文快照后若不转换为本机密文库，后续轮次
/// 便无法开封通道上的密文段（信封形态全通道一致是隐含契约）；对齐动作归
/// 壳层引导编排（issue #864，复用备份域整库加密转换）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedCheckpoint {
    /// 检查点本体（快照 + 位点）。
    pub checkpoint: Checkpoint,
    /// 通道快照是否为密文信封。
    pub sealed: bool,
    /// 采纳的检查点代数（manifest 指针，向导展示与日志用）。
    pub generation: i64,
    /// 快照密文字节数。
    pub size: u64,
}

/// 读取通道上的当前检查点指针（不下载快照体；新端引导前的预检接缝，
/// 壳层向导据此区分「通道上还没有检查点」与「发现检查点可引导」）。
pub fn peek_checkpoint_pointer(
    transport: &dyn Transport,
    layout: &ChannelLayout,
) -> Result<Option<CheckpointPointer>> {
    Ok(read_manifest(transport, layout)?.checkpoint)
}

/// 拉取通道上的当前检查点（新端引导的取件接缝；解封凭主口令，明文模式免口令）。
///
/// 返回的 [`FetchedCheckpoint`] 交给 [`checkpoint::bootstrap_from_checkpoint`] 完成
/// 整库换入（引导流程编排归壳层加入向导，issue #864）。
pub fn fetch_checkpoint(
    transport: &dyn Transport,
    layout: &ChannelLayout,
    passphrase: Option<&str>,
) -> Result<FetchedCheckpoint> {
    let manifest = read_manifest(transport, layout)?;
    let pointer = manifest.checkpoint.ok_or_else(|| {
        AppError::coded("sync-channel.checkpoint-none", "同步通道上还没有检查点快照")
    })?;
    let path = layout.checkpoint_file_path(&pointer.file);
    let bytes = transport
        .read_file(&path)?
        .ok_or_else(|| checkpoint_missing_error(&path))?;
    if bytes.len() as u64 != pointer.size || sha256_hex(&bytes) != pointer.sha256 {
        return Err(checkpoint_corrupt_error(&path));
    }
    // 封包形态先于开封判定（`is_sealed` 按文件自身魔数，自描述）：引导端
    // 据此对齐本库加密形态，见 [`FetchedCheckpoint::sealed`]。
    let sealed = envelope::is_sealed(&bytes);
    let bundle = envelope::open(&bytes, passphrase)?;
    let checkpoint =
        unframe_checkpoint_bundle(&bundle).map_err(|_| checkpoint_corrupt_error(&path))?;
    Ok(FetchedCheckpoint {
        checkpoint,
        sealed,
        generation: pointer.generation,
        size: pointer.size,
    })
}

/// 检查点通道载荷组帧：头（位点 JSON，4 字节 LE 长度前缀）+ 快照字节。
///
/// 位点与快照随信封整体封包——引导所需的两半必须同刻成对到达，不依赖
/// manifest（manifest 是明文元数据，不承载位点数据面）。
fn frame_checkpoint_bundle(checkpoint: &Checkpoint) -> Result<Vec<u8>> {
    let header = serde_json::json!({
        "positions": checkpoint
            .positions
            .iter()
            .map(|p| {
                serde_json::json!({
                    "device_id": p.device_id,
                    "applied_through": p.applied_through,
                })
            })
            .collect::<Vec<_>>(),
    });
    let header_bytes = serde_json::to_vec(&header)
        .map_err(|e| AppError::Invalid(format!("检查点头序列化失败: {e}")))?;
    let mut bundle = Vec::with_capacity(4 + header_bytes.len() + checkpoint.snapshot.len());
    bundle.extend_from_slice(&(header_bytes.len() as u32).to_le_bytes());
    bundle.extend_from_slice(&header_bytes);
    bundle.extend_from_slice(&checkpoint.snapshot);
    Ok(bundle)
}

/// 检查点通道载荷拆帧（组帧的逆；损坏报错由调用方归并为检查点损坏）。
fn unframe_checkpoint_bundle(bundle: &[u8]) -> std::result::Result<Checkpoint, ()> {
    if bundle.len() < 4 {
        return Err(());
    }
    let header_len = u32::from_le_bytes(bundle[..4].try_into().map_err(|_| ())?) as usize;
    let header_end = 4usize.checked_add(header_len).ok_or(())?;
    if header_end > bundle.len() {
        return Err(());
    }
    let header: serde_json::Value =
        serde_json::from_slice(&bundle[4..header_end]).map_err(|_| ())?;
    let positions = header
        .get("positions")
        .and_then(|v| v.as_array())
        .ok_or(())?
        .iter()
        .map(|p| -> std::result::Result<super::StreamPosition, ()> {
            Ok(super::StreamPosition {
                device_id: p
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or(())?
                    .to_string(),
                applied_through: p
                    .get("applied_through")
                    .and_then(|v| v.as_i64())
                    .ok_or(())?,
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(Checkpoint {
        positions,
        snapshot: bundle[header_end..].to_vec(),
    })
}

/// op 序列按容量切连续段（容量下界 1，防零除/空步进）。
fn split_chunks(ops: &[SyncOp], max_ops: usize) -> Vec<&[SyncOp]> {
    ops.chunks(max_ops.max(1)).collect()
}

/// SHA-256 hex 摘要（通道条目自校验单点）。
///
/// `pub(crate)` 而非私有（issue #956）：测试支持域的「通道线格式替身」
/// （`test_support::channel`）据此成帧，使测试侧不再自建第二份摘要口径。
/// 这是测试专用放宽（ADR-0084 决策 2 同款：`test_support` 本身即测试专用
/// 放宽面），产品路径的消费者仍只有本模块。
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// 清单损坏码化错误单点。
fn manifest_corrupt_error(detail: &str) -> AppError {
    AppError::codedp(
        "sync-channel.manifest-corrupt",
        format!("同步清单损坏或无法解析: {detail}"),
        &[detail],
    )
}

/// 段缺失码化错误单点（网盘清单先行/文件未到齐；可重试）。
fn segment_missing_error(path: &str) -> AppError {
    AppError::codedp(
        "sync-channel.segment-missing",
        format!("同步段文件在通道上缺失（网盘可能尚未同步完成），请稍后重试: {path}"),
        &[path],
    )
}

/// 段损坏码化错误单点（hash/尺寸不符或载荷不可解析；可重试重拉）。
fn segment_corrupt_error(detail: &str) -> AppError {
    AppError::codedp(
        "sync-channel.segment-corrupt",
        format!("同步段文件校验失败（可能未完整上传或已损坏），请重试同步: {detail}"),
        &[detail],
    )
}

/// 检查点缺失码化错误单点。
fn checkpoint_missing_error(path: &str) -> AppError {
    AppError::codedp(
        "sync-channel.checkpoint-missing",
        format!("同步通道上的检查点文件缺失，请重新发布检查点: {path}"),
        &[path],
    )
}

/// 检查点损坏码化错误单点。
fn checkpoint_corrupt_error(path: &str) -> AppError {
    AppError::codedp(
        "sync-channel.checkpoint-corrupt",
        format!("检查点快照校验失败（可能未完整上传或已损坏），请重新发布检查点: {path}"),
        &[path],
    )
}
