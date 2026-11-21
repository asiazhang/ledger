use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use ledger_infra::db::now_iso;
use ledger_infra::error::{AppError, Result};

use ledger_sync_protocol::device;
use ledger_sync_protocol::position as positions;

use super::checkpoint::Checkpoint;
use super::engine;
use super::envelope::{self, EnvelopeMode, EnvelopeParams};
use super::model::SyncOp;
use super::ops;
use super::transport::Transport;

const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelLayout {
    book_dir: String,
}

impl ChannelLayout {
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

    pub fn book_dir(&self) -> String {
        self.book_dir.clone()
    }

    pub fn manifest_path(&self) -> String {
        format!("{}/manifest.json", self.book_dir)
    }

    pub fn probe_path(&self) -> String {
        format!("{}/probe/connectivity", self.book_dir)
    }

    pub fn checkpoint_dir(&self) -> String {
        format!("{}/checkpoint", self.book_dir)
    }

    pub fn streams_dir(&self) -> String {
        format!("{}/streams", self.book_dir)
    }

    pub fn stream_dir(&self, device_id: &str) -> String {
        format!("{}/{}", self.streams_dir(), device_id)
    }

    pub fn stream_file_path(&self, device_id: &str, file: &str) -> String {
        format!("{}/{}", self.stream_dir(device_id), file)
    }

    pub fn segment_path(&self, device_id: &str, first_clock: i64, last_clock: i64) -> String {
        self.stream_file_path(device_id, &segment_file_name(first_clock, last_clock))
    }

    pub fn checkpoint_file_path(&self, file: &str) -> String {
        format!("{}/{}", self.checkpoint_dir(), file)
    }

    pub fn checkpoint_path(&self, generation: i64) -> String {
        format!(
            "{}/{}",
            self.checkpoint_dir(),
            checkpoint_file_name(generation)
        )
    }

    pub fn checkpoint_sync_path(&self, generation: i64) -> String {
        format!(
            "{}/{}",
            self.checkpoint_dir(),
            checkpoint_sync_file_name(generation)
        )
    }
}

fn segment_file_name(first_clock: i64, last_clock: i64) -> String {
    format!("seg-{first_clock:010}-{last_clock:010}.enc")
}

fn checkpoint_file_name(generation: i64) -> String {
    format!("cp-{generation:06}.enc")
}

fn checkpoint_sync_file_name(generation: i64) -> String {
    format!("cp-{generation:06}-sync.enc")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelManifest {
    pub version: u32,
    pub streams: Vec<StreamManifest>,
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

    fn serialize(&self) -> Result<Vec<u8>> {
        serde_json::to_vec_pretty(self)
            .map_err(|e| AppError::Invalid(format!("清单序列化失败: {e}")))
    }

    fn normalize(mut self) -> Self {
        self.streams.sort_by(|a, b| a.device_id.cmp(&b.device_id));
        for stream in &mut self.streams {
            stream.segments.sort_by_key(|s| s.first_clock);
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamManifest {
    pub device_id: String,
    pub segments: Vec<SegmentEntry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub applied_positions: BTreeMap<String, i64>,
}

impl StreamManifest {
    fn uploaded_through(&self) -> i64 {
        self.segments.last().map(|s| s.last_clock).unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentEntry {
    pub file: String,
    pub first_clock: i64,
    pub last_clock: i64,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointPointer {
    pub file: String,
    pub generation: i64,
    pub size: u64,
    pub sha256: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub applied_positions: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnSegment {
    Read,
    Replay,
    Bookkeep,
}

pub trait RoundConn {
    fn with_connection<R, F>(&self, segment: ConnSegment, use_connection: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R>;

    fn round_key(&self) -> u64;
}

pub fn connection_round_key(conn: &std::sync::Mutex<Connection>) -> u64 {
    std::ptr::from_ref(conn) as u64
}

pub struct DirectConn<'a> {
    conn: &'a Connection,
}

impl<'a> DirectConn<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }
}

impl RoundConn for DirectConn<'_> {
    fn with_connection<R, F>(&self, _segment: ConnSegment, use_connection: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R>,
    {
        use_connection(self.conn)
    }

    fn round_key(&self) -> u64 {
        std::ptr::from_ref(self.conn) as u64
    }
}

struct RoundCtx<'a, 'p, S: RoundConn> {
    locks: &'a S,
    transport: &'a dyn Transport,
    layout: &'a ChannelLayout,
    mode: &'a EnvelopeMode<'p>,
    options: &'a ChannelOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelOptions {
    pub segment_max_ops: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct SyncRoundReport {
    pub uploaded_segments: usize,
    pub uploaded_ops: usize,
    pub downloaded_segments: usize,
    pub applied: usize,
    pub deduped: usize,
    pub superseded: usize,
    pub skipped: usize,
    pub parked: usize,
    pub plaintext_mode: bool,
}

pub fn run_round<S: RoundConn>(
    locks: &S,
    transport: &dyn Transport,
    layout: &ChannelLayout,
    mode: &EnvelopeMode<'_>,
) -> Result<SyncRoundReport> {
    run_round_with(locks, transport, layout, mode, &ChannelOptions::default())
}

pub fn run_round_with<S: RoundConn>(
    locks: &S,
    transport: &dyn Transport,
    layout: &ChannelLayout,
    mode: &EnvelopeMode<'_>,
    options: &ChannelOptions,
) -> Result<SyncRoundReport> {
    let device_id = locks.with_connection(ConnSegment::Read, device::device_id)?;
    transport.ensure_dir(&layout.book_dir())?;
    transport.ensure_dir(&layout.checkpoint_dir())?;
    transport.ensure_dir(&layout.streams_dir())?;

    let ctx = RoundCtx {
        locks,
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

    let mut manifest = remote.clone();
    publish_own_ops(&ctx, &device_id, &remote, &mut manifest, &mut report)?;

    declare_applied_positions(&ctx, &device_id, &mut manifest)?;

    if manifest != remote {
        transport.write_file(&layout.manifest_path(), &manifest.serialize()?)?;
    }

    pull_foreign_streams(&ctx, &device_id, &manifest, &mut report)?;

    truncate_own_stream_to_watermark(&ctx, &manifest, &device_id);
    Ok(report)
}

fn declare_applied_positions<S: RoundConn>(
    ctx: &RoundCtx<'_, '_, S>,
    device_id: &str,
    manifest: &mut ChannelManifest,
) -> Result<()> {
    let own: BTreeMap<String, i64> = ctx.locks.with_connection(ConnSegment::Read, |conn| {
        Ok(position_map(&positions::list(conn)?))
    })?;
    for stream in &mut manifest.streams {
        if let Some(applied) = own.get(&stream.device_id) {
            stream
                .applied_positions
                .insert(device_id.to_string(), *applied);
        }
    }
    Ok(())
}

fn position_map(rows: &[ledger_sync_protocol::position::StreamPosition]) -> BTreeMap<String, i64> {
    rows.iter()
        .map(|p| (p.device_id.clone(), p.applied_through))
        .collect()
}

fn own_stream_watermark(manifest: &ChannelManifest, device_id: &str) -> Option<i64> {
    let mut participants: BTreeSet<&str> = manifest
        .streams
        .iter()
        .map(|s| s.device_id.as_str())
        .collect();
    for stream in &manifest.streams {
        participants.extend(stream.applied_positions.keys().map(String::as_str));
    }
    participants.remove(device_id);
    if participants.is_empty() {
        return None;
    }
    let own = manifest.streams.iter().find(|s| s.device_id == device_id)?;
    let declared_min = participants
        .iter()
        .map(|p| own.applied_positions.get(*p).copied().unwrap_or(0))
        .fold(i64::MAX, i64::min);
    let covered = manifest
        .checkpoint
        .as_ref()?
        .applied_positions
        .get(device_id)
        .copied()?;
    let watermark = declared_min.min(covered);
    (watermark > 0).then_some(watermark)
}

fn truncate_own_stream_to_watermark<S: RoundConn>(
    ctx: &RoundCtx<'_, '_, S>,
    manifest: &ChannelManifest,
    device_id: &str,
) {
    let Some(watermark) = own_stream_watermark(manifest, device_id) else {
        return;
    };
    match ctx.locks.with_connection(ConnSegment::Bookkeep, |conn| {
        super::checkpoint::truncate_stream_before(conn, device_id, watermark)
    }) {
        Ok(deleted) if deleted > 0 => tracing::info!(
            device = %device_id,
            watermark,
            deleted,
            "轮次落库段按安全水位截断本机日志流"
        ),
        Ok(_) => {}
        Err(error) => tracing::warn!(error = %error, watermark, "截断失败（下一轮重试）"),
    }
}

fn read_manifest(transport: &dyn Transport, layout: &ChannelLayout) -> Result<ChannelManifest> {
    match transport.read_file(&layout.manifest_path())? {
        None => Ok(ChannelManifest::default()),
        Some(bytes) => ChannelManifest::parse(&bytes),
    }
}

fn publish_own_ops<S: RoundConn>(
    ctx: &RoundCtx<'_, '_, S>,
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
            applied_positions: BTreeMap::new(),
        });
    let own_ops = ctx.locks.with_connection(ConnSegment::Read, |conn| {
        ops::read_own_since(conn, device_id, remote_own.uploaded_through())
    })?;
    if own_ops.is_empty() {
        return Ok(());
    }
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
    let entry = StreamManifest {
        device_id: device_id.to_string(),
        segments: segments.into_values().collect(),
        applied_positions: remote_own.applied_positions.clone(),
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

fn pull_foreign_streams<S: RoundConn>(
    ctx: &RoundCtx<'_, '_, S>,
    device_id: &str,
    manifest: &ChannelManifest,
    report: &mut SyncRoundReport,
) -> Result<()> {
    let passphrase = match ctx.mode {
        EnvelopeMode::Encrypted { passphrase } => Some(*passphrase),
        EnvelopeMode::Plaintext => None,
    };
    let positions: std::collections::BTreeMap<String, i64> =
        ctx.locks.with_connection(ConnSegment::Read, |conn| {
            manifest
                .streams
                .iter()
                .filter(|stream| stream.device_id != device_id)
                .map(|stream| {
                    Ok((
                        stream.device_id.clone(),
                        positions::position_of(conn, &stream.device_id)?.unwrap_or(0),
                    ))
                })
                .collect()
        })?;
    for stream in &manifest.streams {
        if stream.device_id == device_id {
            continue;
        }
        let position = positions[&stream.device_id];
        for segment in &stream.segments {
            if segment.last_clock <= position {
                continue;
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
            if incoming.iter().any(|op| op.device_id != stream.device_id) {
                return Err(segment_corrupt_error(&path));
            }
            let reports = ctx.locks.with_connection(ConnSegment::Replay, |conn| {
                engine::apply_ops(conn, &incoming)
            })?;
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

pub fn upload_checkpoint(
    transport: &dyn Transport,
    layout: &ChannelLayout,
    mode: &EnvelopeMode<'_>,
    checkpoint: &Checkpoint,
) -> Result<CheckpointPointer> {
    upload_checkpoint_with(
        transport,
        layout,
        mode,
        checkpoint,
        &ChannelOptions::default(),
    )
}

pub fn upload_checkpoint_with(
    transport: &dyn Transport,
    layout: &ChannelLayout,
    mode: &EnvelopeMode<'_>,
    checkpoint: &Checkpoint,
    options: &ChannelOptions,
) -> Result<CheckpointPointer> {
    transport.ensure_dir(&layout.checkpoint_dir())?;

    let remote = read_manifest(transport, layout)?;
    let generation = remote
        .checkpoint
        .as_ref()
        .map(|p| p.generation)
        .unwrap_or(0)
        + 1;
    let sealed = envelope::seal(
        &frame_checkpoint_bundle(checkpoint)?,
        mode,
        &options.envelope,
    )?;
    transport.write_file(&layout.checkpoint_path(generation), &sealed)?;
    let (sync_file, sync_size, sync_sha256) = if checkpoint.sync_snapshot.is_empty() {
        (None, None, None)
    } else {
        let sync_sealed = envelope::seal(&checkpoint.sync_snapshot, mode, &options.envelope)?;
        transport.write_file(&layout.checkpoint_sync_path(generation), &sync_sealed)?;
        (
            Some(checkpoint_sync_file_name(generation)),
            Some(sync_sealed.len() as u64),
            Some(sha256_hex(&sync_sealed)),
        )
    };
    let pointer = CheckpointPointer {
        file: checkpoint_file_name(generation),
        generation,
        size: sealed.len() as u64,
        sha256: sha256_hex(&sealed),
        created_at: now_iso(),
        sync_file,
        sync_size,
        sync_sha256,
        applied_positions: position_map(&checkpoint.positions),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedCheckpoint {
    pub checkpoint: Checkpoint,
    pub sealed: bool,
    pub generation: i64,
    pub size: u64,
}

pub fn peek_checkpoint_pointer(
    transport: &dyn Transport,
    layout: &ChannelLayout,
) -> Result<Option<CheckpointPointer>> {
    Ok(read_manifest(transport, layout)?.checkpoint)
}

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
    let sealed = envelope::is_sealed(&bytes);
    let bundle = envelope::open(&bytes, passphrase)?;
    let mut checkpoint =
        unframe_checkpoint_bundle(&bundle).map_err(|_| checkpoint_corrupt_error(&path))?;
    if let (Some(sync_file), Some(sync_size), Some(sync_sha256)) =
        (&pointer.sync_file, pointer.sync_size, &pointer.sync_sha256)
    {
        let sync_path = layout.checkpoint_file_path(sync_file);
        let sync_bytes = transport
            .read_file(&sync_path)?
            .ok_or_else(|| checkpoint_missing_error(&sync_path))?;
        if sync_bytes.len() as u64 != sync_size || sha256_hex(&sync_bytes) != *sync_sha256 {
            return Err(checkpoint_corrupt_error(&sync_path));
        }
        checkpoint.sync_snapshot = envelope::open(&sync_bytes, passphrase)?;
    }
    Ok(FetchedCheckpoint {
        checkpoint,
        sealed,
        generation: pointer.generation,
        size: pointer.size,
    })
}

#[doc(hidden)]
pub fn frame_checkpoint_bundle(checkpoint: &Checkpoint) -> Result<Vec<u8>> {
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
        sync_snapshot: Vec::new(),
    })
}

fn split_chunks(ops: &[SyncOp], max_ops: usize) -> Vec<&[SyncOp]> {
    ops.chunks(max_ops.max(1)).collect()
}

#[doc(hidden)]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn manifest_corrupt_error(detail: &str) -> AppError {
    AppError::codedp(
        "sync-channel.manifest-corrupt",
        format!("同步清单损坏或无法解析: {detail}"),
        &[detail],
    )
}

fn segment_missing_error(path: &str) -> AppError {
    AppError::codedp(
        "sync-channel.segment-missing",
        format!("同步段文件在通道上缺失（通道可能尚未同步完成），请稍后重试: {path}"),
        &[path],
    )
}

fn segment_corrupt_error(detail: &str) -> AppError {
    AppError::codedp(
        "sync-channel.segment-corrupt",
        format!("同步段文件校验失败（可能未完整上传或已损坏），请重试同步: {detail}"),
        &[detail],
    )
}

fn checkpoint_missing_error(path: &str) -> AppError {
    AppError::codedp(
        "sync-channel.checkpoint-missing",
        format!("同步通道上的检查点文件缺失，请重新发布检查点: {path}"),
        &[path],
    )
}

fn checkpoint_corrupt_error(path: &str) -> AppError {
    AppError::codedp(
        "sync-channel.checkpoint-corrupt",
        format!("检查点快照校验失败（可能未完整上传或已损坏），请重新发布检查点: {path}"),
        &[path],
    )
}
