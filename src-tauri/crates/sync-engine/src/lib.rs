#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

pub mod channel;
pub mod checkpoint;
pub mod command;
pub mod engine;
pub mod envelope;
pub mod model;
pub mod ops;
pub mod parked;
pub mod registry;
pub mod transport;
pub mod trigger;

pub use channel::{
    ChannelLayout, ChannelManifest, ChannelOptions, CheckpointPointer, ConnSegment, DirectConn,
    FetchedCheckpoint, RoundConn, SegmentEntry, StreamManifest, SyncRoundReport,
    connection_round_key, fetch_checkpoint, run_round, run_round_with, upload_checkpoint,
    upload_checkpoint_with,
};
pub use checkpoint::{
    BootstrapConnSegments, BootstrapOutcome, Checkpoint, bootstrap_from_channel,
    bootstrap_from_checkpoint, create_checkpoint, truncate_stream_before,
};
pub use command::DomainCommand;
pub use engine::{
    ApplyReport, OpOutcome, apply_ops, ingest_ops, ops_after_positions, parked_ops, read_ops,
    stream_positions, total_order,
};
pub use envelope::EnvelopeMode;
pub use ledger_sync_protocol::position::StreamPosition;
pub use model::SyncOp;
pub use parked::ParkedOp;
pub use transport::{
    Transport,
    s3::{S3Config, S3Transport},
};
pub use trigger::{
    SessionEnvelope, SyncChannel, SyncChannelConfig, TriggerTimings, build_channel,
    configured_channel, probe_channel, run_auto_round, run_round_once, start_sync_scheduler,
    start_sync_scheduler_with, start_triggers, sync_after_write, sync_on_start,
};

#[cfg(test)]
mod tests;
