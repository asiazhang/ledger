//! 共享「通道线格式替身」（issue #956，ADR-0084 准入：命令面集成测试与 BDD
//! 步骤层 ≥2 处同体消费）：把一段 op 序列按**通道线格式**成帧发布——序列化
//! payload、按信封模式封包、对**封包后字节**取尺寸与摘要、写段文件、写 manifest。
//! 测试侧只构造语义输入（op 与身份），字节级形态收归此处单点。
//!
//! **为什么共享**：产品侧 `sync_engine::channel` 的发布路径（段命名、封包、
//! 尺寸/摘要口径、清单归并）此前在命令面集成测试与 BDD 步骤里各被手工复制了
//! 一份，且两份都把尺寸与摘要算在 **payload** 上——产品算在 `envelope::seal`
//! 的**输出**上，明文模式下二者恰好相等。共享的是通道线格式契约（字节级
//! 成帧），不是工厂的建库/种子/默认值集（ADR-0086 决策 9 不破）。
//!
//! **摘要口径单一实现**：本模块不另造摘要函数，直接消费产品侧
//! [`crate::sync_engine::channel::sha256_hex`]（`pub(crate)`，见该函数注释）
//! ——测试侧从此不可能自建第二份口径。
//!
//! **可见性**：`pub` + `#[doc(hidden)]`（本模块同款纪律）——集成测试链接非
//! `#[cfg(test)]` 构建的 lib，经 `crate::test_support` 消费。
//!
//! **线程归属留调用方**：本助手只做 IO，不接管线程。命令面集成测试在独立 OS
//! 线程内调用（reqwest 阻塞客户端不得在 tokio 运行时内构造/析构），BDD 步骤
//! 在 `block_in_place` 内调用——两者各自保持原有语义。
// C 类豁免（ADR-0060）：仅测试用——本文件随 test_support 文件级放行六件套
// （见 mod.rs 豁免声明）。
use crate::error::{AppError, Result};
use crate::sync_engine::channel::{
    ChannelLayout, ChannelManifest, SegmentEntry, StreamManifest, sha256_hex,
};
use crate::sync_engine::envelope::{self, EnvelopeMode, EnvelopeParams};
use crate::sync_engine::model::SyncOp;
use crate::sync_engine::transport::Transport;

/// 单段 op 数上限：测试夹具按「一段」成帧（与通道选项默认段容量同量级；
/// 调用方传入的 op 序列超出即切多段，与产品侧同形）。
const FIXTURE_SEGMENT_MAX_OPS: usize = 2000;

/// 把 `ops` 发布上通道（段 + manifest 两条真实通道写）。
///
/// 内部口径与产品侧发布路径一致：段文件名经 [`ChannelLayout::segment_path`]
/// 派生（不硬编码格式字符串）、`size`/`sha256` 取**封包后字节**（明文模式即
/// payload）、清单版本经 [`ChannelManifest::default`] 取（不写字面量）。
///
/// **覆盖写**（与既有夹具同形）：manifest 按单一来源流整体写出，不读回既有
/// 清单归并——场景内通道为空，语义等价且更简单。
///
/// `mode` 决定封包形态：明文库传 [`EnvelopeMode::Plaintext`]，密文库传
/// [`EnvelopeMode::Encrypted`]（口令须与拉取端一致，否则对端开封失败）。
/// KDF 迭代取 [`EnvelopeParams::default`]——封包格式语义与派生成本无关。
///
/// 段区间取 op 序列首尾时钟；空序列零动作（不写段、不写清单）。
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
