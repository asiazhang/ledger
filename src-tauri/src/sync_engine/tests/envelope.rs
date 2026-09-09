//! SyncEnvelope 信封加密（issue #859 / ADR-0091 决策 8）：整包加密往返、
//! 密钥派生自主口令且不随信封走、错误口令/缺口令/篡改的码化错误、明文模式
//! 直通与密文自描述判定。
//!
//! 测试统一用低迭代次数参数（`fast_params`）——迭代次数是安全成本参数而非
//! 格式语义，生产默认经 [`DEFAULT_KDF_ITERATIONS`] 常量断言钉住。

use crate::sync_engine::envelope::{
    DEFAULT_KDF_ITERATIONS, EnvelopeMode, EnvelopeParams, is_sealed, open, seal,
};

/// 测试用低迭代参数：信封格式往返逻辑与 KDF 成本无关（迭代次数随信封头
/// 自描述），生产默认值单独断言。
fn fast_params() -> EnvelopeParams {
    EnvelopeParams {
        kdf_iterations: 2_000,
    }
}

/// 明文模式：原样字节直通，不加密、不自描述。
#[test]
fn plaintext_mode_passes_bytes_through() {
    let payload = b"[{\"op_id\":\"op-1\"}]".to_vec();
    let sealed = seal(&payload, &EnvelopeMode::Plaintext, &fast_params()).unwrap();
    assert_eq!(sealed, payload, "明文模式逐字节直通");
    assert!(!is_sealed(&sealed));
    let opened = open(&sealed, None).unwrap();
    assert_eq!(opened, payload);
}

/// 加密模式往返：密文 ≠ 原文；凭同一主口令解开原文；口令不随信封走
/// （信封内不存在可还原口令的路径，错误口令打不开）。
#[test]
fn encrypted_roundtrip_recovers_payload_with_passphrase() {
    let payload = b"ledger-sync-payload-\xFF\xFE-binary".to_vec();
    let sealed = seal(
        &payload,
        &EnvelopeMode::Encrypted {
            passphrase: "主口令-正确",
        },
        &fast_params(),
    )
    .unwrap();
    assert_ne!(sealed, payload, "通道上只有密文");
    assert!(is_sealed(&sealed));
    let opened = open(&sealed, Some("主口令-正确")).unwrap();
    assert_eq!(opened, payload);
}

/// 每次加密的盐与 nonce 均随机：同一原文两次封包得到不同密文。
#[test]
fn sealing_is_randomized_per_envelope() {
    let payload = b"same-payload".to_vec();
    let mode = EnvelopeMode::Encrypted {
        passphrase: "口令"
    };
    let a = seal(&payload, &mode, &fast_params()).unwrap();
    let b = seal(&payload, &mode, &fast_params()).unwrap();
    assert_ne!(a, b);
    assert_eq!(open(&a, Some("口令")).unwrap(), payload);
    assert_eq!(open(&b, Some("口令")).unwrap(), payload);
}

/// 缺口令：密文信封无法开封，报码化错误（可重试——凭口令重试即可）。
#[test]
fn sealed_envelope_without_passphrase_is_rejected() {
    let sealed = seal(
        b"payload",
        &EnvelopeMode::Encrypted {
            passphrase: "口令"
        },
        &fast_params(),
    )
    .unwrap();
    let err = open(&sealed, None).unwrap_err();
    assert!(
        err.is_code("sync-channel.passphrase-required"),
        "实际: {err:?}"
    );
}

/// 错误口令：报 `encryption.passphrase-incorrect`（与密文备份恢复同款合并
/// 口径：口令错误或文件损坏，不误报、可就地重输）。
#[test]
fn wrong_passphrase_reports_merged_incorrect_passphrase_code() {
    let sealed = seal(
        b"payload",
        &EnvelopeMode::Encrypted {
            passphrase: "正确口令",
        },
        &fast_params(),
    )
    .unwrap();
    let err = open(&sealed, Some("错误口令")).unwrap_err();
    assert!(
        err.is_code("encryption.passphrase-incorrect"),
        "实际: {err:?}"
    );
}

/// 密文被篡改（通道上的位翻转/截断替换）：AEAD 认证失败归同一合并口径，
/// 不静默接受、不误报为程序缺陷。
#[test]
fn tampered_ciphertext_is_rejected() {
    let mut sealed = seal(
        b"payload-that-is-long-enough-to-tamper",
        &EnvelopeMode::Encrypted {
            passphrase: "口令"
        },
        &fast_params(),
    )
    .unwrap();
    let last = sealed.len() - 1;
    sealed[last] ^= 0x01;
    let err = open(&sealed, Some("口令")).unwrap_err();
    assert!(
        err.is_code("encryption.passphrase-incorrect"),
        "实际: {err:?}"
    );
}

/// 信封结构损坏（截断/未知版本）：报信封损坏，不冒充口令错误。
#[test]
fn structurally_broken_envelopes_report_corrupt_code() {
    let sealed = seal(
        b"payload",
        &EnvelopeMode::Encrypted {
            passphrase: "口令"
        },
        &fast_params(),
    )
    .unwrap();

    // 截断到头部之内：长度不足以构成信封。
    let truncated = sealed[..10].to_vec();
    let err = open(&truncated, Some("口令")).unwrap_err();
    assert!(
        err.is_code("sync-channel.envelope-corrupt"),
        "实际: {err:?}"
    );

    // 头部完整但版本未知：拒绝解析（前向兼容由未来版本号协商，不猜格式）。
    let mut foreign = sealed.clone();
    foreign[4] = 99;
    let err = open(&foreign, Some("口令")).unwrap_err();
    assert!(
        err.is_code("sync-channel.envelope-corrupt"),
        "实际: {err:?}"
    );
}

/// 空口令在封包时即拒绝（与加密引擎开启加密同款守卫，不产出打不开的信封）。
#[test]
fn empty_passphrase_is_rejected_at_seal() {
    let err = seal(
        b"payload",
        &EnvelopeMode::Encrypted { passphrase: "" },
        &fast_params(),
    )
    .unwrap_err();
    assert!(err.is_code("encryption.passphrase-empty"), "实际: {err:?}");
}

/// 生产默认 KDF 迭代次数钉在与 SQLCipher 默认 KDF 同量级（256k）：
/// 派生成本是「主口令 → 密钥」安全量级的组成部分，不得静默下调。
#[test]
fn default_kdf_iterations_stay_at_sqlcipher_scale() {
    assert_eq!(DEFAULT_KDF_ITERATIONS, 256_000);
    let sealed = seal(
        b"payload",
        &EnvelopeMode::Encrypted {
            passphrase: "口令"
        },
        &EnvelopeParams::default(),
    )
    .unwrap();
    // 默认参数产出的信封可凭同一口令开封（迭代次数随头自描述）。
    assert_eq!(open(&sealed, Some("口令")).unwrap(), b"payload");
}
