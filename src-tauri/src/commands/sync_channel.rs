use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Manager, Runtime};

use crate::commands::encryption::{active_book_id, active_db_path};
use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, SegmentLock, write_entry_segmented};
use ledger_infra::db::encryption::{DbFileKind, probe_file_kind, verify_source_passphrase};
use ledger_infra::db::passphrase_cache::{self, CacheLoad};
use ledger_infra::db::{DbState, probe_lock_hold, run_db};
use ledger_infra::error::{AppError, Result};
use ledger_infra::settings::{self, SettingKey};
use ledger_infra::signals::{WriteEvidence, WriteOp};
use ledger_sync_engine::channel::{ConnSegment, RoundConn};
use ledger_sync_engine::trigger::{
    DEFAULT_SPACE_ID, book_unavailable_error, build_channel, configured_channel,
    not_configured_error, probe_channel, run_round_once,
};
use ledger_sync_engine::{
    EnvelopeMode, SessionEnvelope, SyncChannelConfig, SyncRoundReport, bootstrap_from_channel,
    connection_round_key, create_checkpoint, parked_ops,
};
use ledger_sync_protocol::device::device_id;

#[derive(Debug, Serialize)]
pub struct SyncChannelConfigState {
    pub space_id: String,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub access_key: String,
    pub secret_key: String,
    pub path_style: bool,
    pub configured: bool,
}

#[derive(Debug, Default, Deserialize)]
pub struct SyncChannelConfigInput {
    pub space_id: Option<String>,
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

#[derive(Debug, Serialize)]
pub struct SyncStatusState {
    pub device_id: String,
    pub channel_configured: bool,
    pub last_sync_at: Option<String>,
    pub parked_count: usize,
    pub library_encrypted: bool,
}

#[tauri::command]
pub async fn get_sync_status<R: Runtime>(app: AppHandle<R>) -> Result<SyncStatusState> {
    let db = app.state::<DbState>().write_handle();
    let db_path = active_db_path(&app)?;
    db.run_raw("get_sync_status", move |conn| {
        Ok(SyncStatusState {
            device_id: device_id(conn)?,
            channel_configured: configured_channel(conn)?.is_some(),
            last_sync_at: settings::get(conn, SettingKey::SyncLastSyncAt, None)?,
            parked_count: parked_ops(conn)?.len(),
            library_encrypted: probe_file_kind(&db_path)? == DbFileKind::Encrypted,
        })
    })
    .await
}

#[tauri::command]
pub async fn sync_now<R: Runtime>(
    app: AppHandle<R>,
    passphrase: Option<String>,
) -> Result<SyncRoundReport> {
    let conn = app.state::<DbState>().conn.clone();
    let db_path = active_db_path(&app)?;
    let book = active_book_id(&app);
    let round_key = connection_round_key(&conn);
    let db = app.state::<DbState>().write_handle();
    write_entry_segmented(
        "sync_now",
        db,
        Some(&app),
        WriteOp::SyncRound,
        move |lock| {
            let config = lock
                .with_connection(configured_channel)?
                .ok_or_else(not_configured_error)?;
            if book.is_none() {
                return Err(book_unavailable_error().into());
            }
            let channel = build_channel(&config)?;
            let passphrase_holder = resolve_passphrase(&db_path, book.as_deref(), passphrase)?;
            let mode = match passphrase_holder {
                Some(ref passphrase) => EnvelopeMode::Encrypted { passphrase },
                None => EnvelopeMode::Plaintext,
            };
            let locks = SegmentRoundConn {
                lock,
                key: round_key,
            };
            let report = run_round_once(&locks, &channel, &mode)?;
            match &passphrase_holder {
                Some(passphrase) => {
                    SessionEnvelope::remember(SessionEnvelope::Encrypted(passphrase.clone()))
                }
                None => SessionEnvelope::remember(SessionEnvelope::Plaintext),
            }
            Ok(Outcome::Evidenced(
                report,
                WriteEvidence::LedgerApplied(report.applied > 0),
            ))
        },
    )
    .await
}

struct SegmentRoundConn<'a> {
    lock: &'a SegmentLock<'a>,
    key: u64,
}

impl RoundConn for SegmentRoundConn<'_> {
    fn with_connection<R, F>(&self, _segment: ConnSegment, use_connection: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R>,
    {
        self.lock.with_connection(use_connection)
    }

    fn round_key(&self) -> u64 {
        self.key
    }
}

#[tauri::command]
pub async fn get_parked_ops<R: Runtime>(app: AppHandle<R>) -> Result<Vec<ParkedOpState>> {
    let conn = app.state::<DbState>().read_handle();
    read_entry("get_parked_ops", conn, move |conn| {
        Ok(parked_ops(conn)?
            .into_iter()
            .map(ParkedOpState::from)
            .collect())
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct ParkedOpState {
    pub op_id: String,
    pub device_id: String,
    pub entity: String,
    pub entity_id: String,
    pub code: String,
    pub params: Vec<String>,
    pub message: String,
    pub parked_at: String,
}

impl From<ledger_sync_engine::ParkedOp> for ParkedOpState {
    fn from(op: ledger_sync_engine::ParkedOp) -> Self {
        Self {
            op_id: op.op_id,
            device_id: op.device_id,
            entity: op.entity,
            entity_id: op.entity_id,
            code: op.code,
            params: op.params,
            message: op.message,
            parked_at: op.parked_at,
        }
    }
}

#[tauri::command]
pub async fn get_sync_channel_config<R: Runtime>(
    app: AppHandle<R>,
) -> Result<SyncChannelConfigState> {
    let conn = app.state::<DbState>().read_handle();
    read_entry("get_sync_channel_config", conn, move |conn| {
        Ok(match configured_channel(conn)? {
            Some(config) => SyncChannelConfigState {
                space_id: config.space_id,
                endpoint: config.endpoint,
                region: config.region,
                bucket: config.bucket,
                prefix: config.prefix,
                access_key: config.access_key,
                secret_key: config.secret_key,
                path_style: config.path_style,
                configured: true,
            },
            None => SyncChannelConfigState {
                space_id: String::new(),
                endpoint: String::new(),
                region: String::new(),
                bucket: String::new(),
                prefix: String::new(),
                access_key: String::new(),
                secret_key: String::new(),
                path_style: false,
                configured: false,
            },
        })
    })
    .await
}

#[tauri::command]
pub async fn set_sync_channel_config<R: Runtime>(
    app: AppHandle<R>,
    config: SyncChannelConfigInput,
) -> Result<()> {
    let db = app.state::<DbState>().write_handle();
    run_db("set_sync_channel_config", move || {
        let config = normalize_channel_config(config);
        ensure_secure_endpoint(&config)?;
        build_channel(&config)?;
        db.run_raw_blocking("set_sync_channel_config", move |conn| {
            settings::set(conn, SettingKey::SyncChannelConfig, &config)
        })
    })
    .await
}

#[tauri::command]
pub async fn test_sync_channel_connection(config: SyncChannelConfigInput) -> Result<()> {
    run_db("test_sync_channel_connection", move || {
        let config = normalize_channel_config(config);
        ensure_secure_endpoint(&config)?;
        probe_channel(&config)
    })
    .await
}

fn normalize_channel_config(input: SyncChannelConfigInput) -> SyncChannelConfig {
    SyncChannelConfig {
        space_id: input
            .space_id
            .unwrap_or_else(|| DEFAULT_SPACE_ID.to_string()),
        endpoint: input.endpoint,
        region: input.region,
        bucket: input.bucket,
        prefix: input.prefix,
        access_key: input.access_key,
        secret_key: input.secret_key,
        path_style: input.path_style,
    }
}

fn ensure_secure_endpoint(config: &SyncChannelConfig) -> Result<()> {
    let endpoint = config.endpoint.trim();
    if endpoint.is_empty() || endpoint.to_ascii_lowercase().starts_with("https://") {
        return Ok(());
    }
    Err(AppError::coded(
        "sync-channel.endpoint-insecure",
        "同步通道端点必须使用 https 地址",
    ))
}

fn resolve_passphrase(
    db_path: &std::path::Path,
    book: Option<&str>,
    passphrase: Option<String>,
) -> Result<Option<String>> {
    if probe_file_kind(db_path)? != DbFileKind::Encrypted {
        return Ok(None);
    }
    let passphrase = match passphrase {
        Some(passphrase) => passphrase,
        None => match passphrase_cache::load(book)? {
            CacheLoad::Found(passphrase) => passphrase,
            CacheLoad::NotFound | CacheLoad::Cancelled => {
                return Err(AppError::coded(
                    "sync-channel.passphrase-required",
                    "同步通道上有加密数据，需要主口令才能读取",
                ));
            }
        },
    };
    verify_source_passphrase(db_path, &passphrase)?;
    Ok(Some(passphrase))
}

#[derive(Debug, Serialize)]
pub struct SyncCheckpointInfoState {
    pub generation: i64,
    pub size: u64,
    pub created_at: String,
}

#[tauri::command]
pub async fn get_sync_channel_checkpoint<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<SyncCheckpointInfoState>> {
    let read_conn = app.state::<DbState>().read_handle();
    let config = read_entry("get_sync_channel_checkpoint", read_conn, move |conn| {
        configured_channel(conn)?.ok_or_else(not_configured_error)
    })
    .await?;
    run_db("get_sync_channel_checkpoint", move || {
        let channel = build_channel(&config)?;
        Ok(channel
            .checkpoint_pointer()?
            .map(|p| SyncCheckpointInfoState {
                generation: p.generation,
                size: p.size,
                created_at: p.created_at,
            }))
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct SyncCheckpointPublished {
    pub generation: i64,
    pub size: u64,
    pub plaintext_mode: bool,
}

#[tauri::command]
pub async fn publish_sync_checkpoint<R: Runtime>(
    app: AppHandle<R>,
    passphrase: Option<String>,
) -> Result<SyncCheckpointPublished> {
    let read_conn = app.state::<DbState>().read_handle();
    let db = app.state::<DbState>().write_handle();
    let db_path = active_db_path(&app)?;
    let book = active_book_id(&app);

    let config = read_entry("publish_sync_checkpoint", read_conn, |conn| {
        configured_channel(conn)?.ok_or_else(not_configured_error)
    })
    .await?;

    run_db("publish_sync_checkpoint", move || {
        if book.is_none() {
            return Err(book_unavailable_error());
        }
        let channel = build_channel(&config)?;
        let passphrase_holder = resolve_passphrase(&db_path, book.as_deref(), passphrase)?;
        let mode = match &passphrase_holder {
            Some(passphrase) => EnvelopeMode::Encrypted { passphrase },
            None => EnvelopeMode::Plaintext,
        };
        let checkpoint = db.run_raw_blocking("publish_sync_checkpoint", create_checkpoint)?;
        let pointer = channel.upload_checkpoint(&checkpoint, &mode)?;
        Ok(SyncCheckpointPublished {
            generation: pointer.generation,
            size: pointer.size,
            plaintext_mode: mode.is_plaintext(),
        })
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct SyncBootstrapOutcome {
    pub generation: i64,
    pub size: u64,
    pub reencrypted: bool,
}

#[tauri::command]
pub async fn bootstrap_sync_from_channel<R: Runtime>(
    app: AppHandle<R>,
    passphrase: Option<String>,
) -> Result<SyncBootstrapOutcome> {
    let read_conn = app.state::<DbState>().read_handle();
    let config = read_entry("bootstrap_sync_from_channel", read_conn, move |conn| {
        configured_channel(conn)?.ok_or_else(not_configured_error)
    })
    .await?;
    let conn = app.state::<DbState>().conn.clone();
    let db_path = active_db_path(&app)?;
    run_db("bootstrap_sync_from_channel", move || {
        let channel = build_channel(&config)?;
        let segments = MainConnSegments { conn };
        let outcome = bootstrap_from_channel(&segments, &db_path, &channel, passphrase.as_deref())?;
        Ok(SyncBootstrapOutcome {
            generation: outcome.generation,
            size: outcome.size,
            reencrypted: outcome.reencrypted,
        })
    })
    .await
}

struct MainConnSegments {
    conn: Arc<Mutex<Connection>>,
}

impl ledger_sync_engine::BootstrapConnSegments for MainConnSegments {
    fn with_conn<T>(&self, use_conn: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let hold_started = Instant::now();
        let mut conn = self.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let result = use_conn(&mut conn);
        probe_lock_hold(hold_started.elapsed());
        result
    }
}
