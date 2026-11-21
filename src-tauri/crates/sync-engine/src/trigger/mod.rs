mod channel;
mod round_gate;
mod scheduler;
mod session;

pub use channel::DEFAULT_SPACE_ID;
pub use channel::{
    SyncChannel, SyncChannelConfig, build_channel, configured_channel, probe_channel,
};
pub use scheduler::{
    TriggerTimings, book_unavailable_error, install_after_write_hook, not_configured_error,
    run_auto_round, run_round_once, start_sync_scheduler, start_sync_scheduler_with,
    start_triggers, sync_after_write, sync_on_start,
};
#[cfg(test)]
pub(crate) use scheduler::{drain_write_signals, notify_write_signal};
pub use session::SessionEnvelope;

#[cfg(test)]
mod tests;
