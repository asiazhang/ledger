use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::channel::{
    ChannelLayout, ChannelOptions, CheckpointPointer, FetchedCheckpoint, RoundConn,
    SyncRoundReport, fetch_checkpoint, peek_checkpoint_pointer, run_round_with, upload_checkpoint,
};
use crate::checkpoint::Checkpoint;
use crate::envelope::EnvelopeMode;
use crate::transport::Transport;
use crate::transport::s3::{S3Config, S3Transport};
use ledger_infra::error::Result;
use ledger_infra::settings::{self, SettingKey};

pub const DEFAULT_SPACE_ID: &str = "default";

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SyncChannelConfig {
    #[serde(default = "default_space_id")]
    pub space_id: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub access_key: String,
    #[serde(default)]
    pub secret_key: String,
    #[serde(default)]
    pub path_style: bool,
}

fn default_space_id() -> String {
    DEFAULT_SPACE_ID.to_string()
}

pub struct SyncChannel {
    transport: Box<dyn Transport>,
    layout: ChannelLayout,
}

impl std::fmt::Debug for SyncChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncChannel")
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

impl SyncChannel {
    #[cfg(test)]
    pub(crate) fn from_parts(transport: Box<dyn Transport>, layout: ChannelLayout) -> Self {
        Self { transport, layout }
    }

    pub fn transport(&self) -> &dyn Transport {
        self.transport.as_ref()
    }

    pub fn layout(&self) -> &ChannelLayout {
        &self.layout
    }

    pub fn run_round<S: RoundConn>(
        &self,
        locks: &S,
        mode: &EnvelopeMode<'_>,
        options: &ChannelOptions,
    ) -> Result<SyncRoundReport> {
        run_round_with(locks, self.transport.as_ref(), &self.layout, mode, options)
    }

    pub fn checkpoint_pointer(&self) -> Result<Option<CheckpointPointer>> {
        peek_checkpoint_pointer(self.transport.as_ref(), &self.layout)
    }

    pub fn probe_connection(&self) -> Result<()> {
        self.transport
            .read_file(&self.layout.probe_path())
            .map(|_| ())
    }

    pub fn upload_checkpoint(
        &self,
        checkpoint: &Checkpoint,
        mode: &EnvelopeMode<'_>,
    ) -> Result<CheckpointPointer> {
        upload_checkpoint(self.transport.as_ref(), &self.layout, mode, checkpoint)
    }

    pub fn fetch_checkpoint(&self, passphrase: Option<&str>) -> Result<FetchedCheckpoint> {
        fetch_checkpoint(self.transport.as_ref(), &self.layout, passphrase)
    }
}

pub fn configured_channel(conn: &Connection) -> Result<Option<SyncChannelConfig>> {
    settings::get(conn, SettingKey::SyncChannelConfig, None)
}

pub fn build_channel(config: &SyncChannelConfig) -> Result<SyncChannel> {
    let transport: Box<dyn Transport> = Box::new(S3Transport::new(S3Config {
        endpoint: config.endpoint.clone(),
        region: config.region.clone(),
        bucket: config.bucket.clone(),
        access_key: config.access_key.clone(),
        secret_key: config.secret_key.clone(),
        prefix: config.prefix.clone(),
        path_style: config.path_style,
    })?);
    let layout = ChannelLayout::new(&config.space_id)?;
    Ok(SyncChannel { transport, layout })
}

pub fn probe_channel(config: &SyncChannelConfig) -> Result<()> {
    build_channel(config)?.probe_connection()
}
