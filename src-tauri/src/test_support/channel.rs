use std::collections::BTreeMap;

use ledger_infra::error::{AppError, Result};
use ledger_sync_engine::Checkpoint;
use ledger_sync_engine::channel::{
    ChannelLayout, ChannelManifest, CheckpointPointer, SegmentEntry, StreamManifest, sha256_hex,
};
use ledger_sync_engine::envelope::{self, EnvelopeMode, EnvelopeParams};
use ledger_sync_engine::model::SyncOp;
use ledger_sync_engine::transport::Transport;

const FIXTURE_SEGMENT_MAX_OPS: usize = 2000;

pub fn publish_raw_segment(
    transport: &dyn Transport,
    layout: &ChannelLayout,
    device_id: &str,
    mode: &EnvelopeMode<'_>,
    ops: &[SyncOp],
) -> Result<()> {
    if ops.is_empty() {
        return Ok(());
    }
    transport.ensure_dir(&layout.stream_dir(device_id))?;
    let mut segments = Vec::new();
    for chunk in ops.chunks(FIXTURE_SEGMENT_MAX_OPS) {
        let first = chunk.first().map(|o| o.clock).unwrap_or(0);
        let last = chunk.last().map(|o| o.clock).unwrap_or(0);
        let path = layout.segment_path(device_id, first, last);
        let file = path
            .rsplit('/')
            .next()
            .ok_or_else(|| AppError::Invalid("段路径缺文件名".to_string()))?
            .to_string();
        let payload = serde_json::to_vec(&chunk)
            .map_err(|e| AppError::Invalid(format!("段载荷序列化失败: {e}")))?;
        let sealed = envelope::seal(&payload, mode, &EnvelopeParams::default())?;
        transport.write_file(&path, &sealed)?;
        segments.push(SegmentEntry {
            file,
            first_clock: first,
            last_clock: last,
            size: sealed.len() as u64,
            sha256: sha256_hex(&sealed),
        });
    }
    let manifest = ChannelManifest {
        streams: vec![StreamManifest {
            device_id: device_id.to_string(),
            segments,
            applied_positions: BTreeMap::new(),
        }],
        ..ChannelManifest::default()
    };
    transport.write_file(
        &layout.manifest_path(),
        &serde_json::to_vec_pretty(&manifest)
            .map_err(|e| AppError::Invalid(format!("清单序列化失败: {e}")))?,
    )?;
    Ok(())
}

pub fn publish_raw_legacy_checkpoint(
    transport: &dyn Transport,
    layout: &ChannelLayout,
    generation: i64,
    mode: &EnvelopeMode<'_>,
    checkpoint: &Checkpoint,
) -> Result<CheckpointPointer> {
    use super::FIXED_NOW;
    assert!(
        checkpoint.sync_snapshot.is_empty(),
        "旧形态替身只承载单文件检查点（sync_snapshot 为空）",
    );
    transport.ensure_dir(&layout.checkpoint_dir())?;
    let sealed = envelope::seal(
        &ledger_sync_engine::channel::frame_checkpoint_bundle(checkpoint)?,
        mode,
        &EnvelopeParams::default(),
    )?;
    let path = layout.checkpoint_path(generation);
    transport.write_file(&path, &sealed)?;
    let pointer = CheckpointPointer {
        file: path
            .rsplit('/')
            .next()
            .ok_or_else(|| AppError::Invalid("检查点路径缺文件名".to_string()))?
            .to_string(),
        generation,
        size: sealed.len() as u64,
        sha256: sha256_hex(&sealed),
        created_at: FIXED_NOW.to_string(),
        sync_file: None,
        sync_size: None,
        sync_sha256: None,
        applied_positions: BTreeMap::new(),
    };
    let mut manifest = match transport.read_file(&layout.manifest_path())? {
        None => ChannelManifest::default(),
        Some(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| AppError::Invalid(format!("清单解析失败: {e}")))?,
    };
    manifest.checkpoint = Some(pointer.clone());
    transport.write_file(
        &layout.manifest_path(),
        &serde_json::to_vec_pretty(&manifest)
            .map_err(|e| AppError::Invalid(format!("清单序列化失败: {e}")))?,
    )?;
    Ok(pointer)
}
