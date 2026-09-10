//! 触发编排域单测（issue #863 / ADR-0091 决策 9）：通道配置单点、轮次编排与
//! 自动触发的零动作语义、会话信封形态——域行为的唯一断言权威层（ADR-0087）。
//!
//! 轮次协议与双端语义归 [`super::super::tests::channel`] 的既有套件；本套件只
//! 钉触发侧新增的事实：配置读取/构库单点、未配置即零动作（不触网、不落成功
//! 时刻）、一轮成功即落成功时刻、会话记忆决定信封形态。

use crate::db::passphrase_cache;
use crate::settings::{self, SettingKey};
use crate::sync_engine::EnvelopeMode;
use crate::sync_engine::SyncChannelConfig;
use crate::sync_engine::tests::common::make_expense;
use crate::sync_engine::transport::Transport;
use crate::sync_engine::trigger::{
    SessionEnvelope, build_channel, configured_channel, run_auto_round, run_round_once,
};
use crate::test_support::{self, seed_account};
use crate::transaction::behavior;

/// 通道配置单点：未配置回 `None`；保存后读回同值。
#[test]
fn channel_config_roundtrip_through_settings_single_point() {
    let conn = test_support::open();
    assert!(
        configured_channel(&conn).unwrap().is_none(),
        "新库未配置通道"
    );

    let config = SyncChannelConfig {
        base_url: "http://127.0.0.1:9/dav/".into(),
        username: "alice".into(),
        password: "app-pass".into(),
        space_id: "family".into(),
    };
    settings::set(&conn, SettingKey::SyncChannelConfig, &config).unwrap();
    assert_eq!(
        configured_channel(&conn).unwrap().as_ref(),
        Some(&config),
        "读回经同一单点，值与写入一致"
    );
}

/// 空间字段缺省回 `default`（旧配置升级路径）：反序列化缺字段不报错。
#[test]
fn missing_space_id_deserializes_to_default() {
    let config: SyncChannelConfig =
        serde_json::from_str(r#"{"base_url":"u","username":"n","password":"p"}"#).unwrap();
    assert_eq!(config.space_id, "default");
}

/// 构库单点：非法空间（清洗后为空）报码化错误；保存路径与本单点同源。
#[test]
fn build_channel_rejects_invalid_space_with_coded_error() {
    let config = SyncChannelConfig {
        base_url: "http://127.0.0.1:9/dav/".into(),
        username: "u".into(),
        password: "p".into(),
        space_id: "///".into(),
    };
    // `SyncChannel` 不实现 `Debug`（内含 reqwest 客户端），故不用 `unwrap_err`：
    // 经 match 取错误，语义等价且不引入 Debug 约束。
    let err = match build_channel(&config) {
        Ok(_) => panic!("非法空间应被拒"),
        Err(e) => e,
    };
    assert!(err.is_code("sync-channel.book-id-invalid"), "实际: {err:?}");
}

/// 会话信封形态（ADR-0098）：无会话口令 → 明文；有 → 密文并以之封包。
/// 「自动轮询不读钥匙串」由本单点保证（读会话记忆，不读钥匙串）。
#[test]
fn session_envelope_follows_session_passphrase() {
    passphrase_cache::clear_session_passphrase();
    assert_eq!(
        SessionEnvelope::current(),
        SessionEnvelope::Plaintext,
        "无会话口令 = 明文形态（明文库的正常形态）"
    );

    passphrase_cache::set_session_passphrase("master-pass");
    assert_eq!(
        SessionEnvelope::current(),
        SessionEnvelope::Encrypted("master-pass".into())
    );
    assert!(matches!(
        SessionEnvelope::current().mode(),
        EnvelopeMode::Encrypted { passphrase } if passphrase == "master-pass"
    ));
    // 收尾：进程级单例跨测试共享，复位避免污染其他用例。
    passphrase_cache::clear_session_passphrase();
}

/// 自动轮次：通道未配置时零动作（`None`，不触网、不落成功时刻）。
#[test]
fn auto_round_without_channel_is_a_noop() {
    let conn = test_support::open();
    let outcome = run_auto_round(&conn, &SessionEnvelope::Plaintext).unwrap();
    assert!(outcome.is_none(), "未配置通道：自动轮次零动作");
    assert_eq!(
        settings::get::<Option<String>>(&conn, SettingKey::SyncLastSyncAt, None).unwrap(),
        None,
        "零动作轮次不更新成功时刻"
    );
}

/// 轮次编排单点：一轮成功即更新「上次成功同步时刻」（手动与自动入口共用）。
#[test]
fn round_once_stamps_last_sync_on_success() {
    let stub = test_support::spawn_webdav_stub(Some(("alice", "app-pass")));
    let conn = test_support::open();
    seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);
    behavior::create(&conn, make_expense("acc-1", 10000, "午饭")).unwrap();
    let config = SyncChannelConfig {
        base_url: stub.base_url.clone(),
        username: "alice".into(),
        password: "app-pass".into(),
        space_id: "default".into(),
    };
    settings::set(&conn, SettingKey::SyncChannelConfig, &config).unwrap();

    let channel = build_channel(&config).unwrap();
    let report = run_round_once(&conn, &channel, &EnvelopeMode::Plaintext).unwrap();
    assert!(report.uploaded_ops >= 1, "本轮应上传本机 op");
    assert!(
        settings::get::<Option<String>>(&conn, SettingKey::SyncLastSyncAt, None)
            .unwrap()
            .is_some(),
        "成功轮次更新上次同步时刻"
    );
    assert!(
        channel
            .transport()
            .read_file(&channel.layout().manifest_path())
            .unwrap()
            .is_some(),
        "轮次应写出 manifest（编排确实跑了轮次协议，而非空转）"
    );
}

/// 写后触发的去抖合流（ADR-0091 决策 9「写 op 后即时入队上传」）：连续写入合流
/// 成一轮，且最后一次写之后仍会跑一轮（不丢最后一批 op）。
#[test]
fn write_after_sync_debounce_keeps_the_last_batch() {
    // 空窗口（无写信号）：不跑轮次。
    let empty = crate::sync_engine::trigger::WriteDebounce::default();
    assert!(!empty.should_run(), "无写信号不该起轮次");

    // 连发多次写：同一窗口内合流，收尾判定为「有活干」——恰好跑一轮。
    let mut burst = crate::sync_engine::trigger::WriteDebounce::default();
    burst.observe();
    burst.observe();
    burst.observe();
    assert!(burst.should_run(), "窗口内有写：收尾应跑一轮（合流成一轮）");
}

/// 写后触发在「调度未拉起」时是零动作（写路径对同步域无感）：投递不 panic、
/// 不触库、不触网——单测与未配置同步的进程都走这一形态。
#[test]
fn write_after_sync_without_scheduler_is_a_noop() {
    crate::sync_engine::sync_after_write();
}
