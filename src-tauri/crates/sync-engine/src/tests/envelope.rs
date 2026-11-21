use crate::envelope::{
    DEFAULT_KDF_ITERATIONS, EnvelopeMode, EnvelopeParams, is_sealed, open, seal,
};

fn fast_params() -> EnvelopeParams {
    EnvelopeParams {
        kdf_iterations: 2_000,
    }
}

#[test]
fn plaintext_mode_passes_bytes_through() {
    let payload = b"[{\"op_id\":\"op-1\"}]".to_vec();
    let sealed = seal(&payload, &EnvelopeMode::Plaintext, &fast_params()).unwrap();
    assert_eq!(sealed, payload, "明文模式逐字节直通");
    assert!(!is_sealed(&sealed));
    let opened = open(&sealed, None).unwrap();
    assert_eq!(opened, payload);
}

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

    let truncated = sealed[..10].to_vec();
    let err = open(&truncated, Some("口令")).unwrap_err();
    assert!(
        err.is_code("sync-channel.envelope-corrupt"),
        "实际: {err:?}"
    );

    let mut foreign = sealed.clone();
    foreign[4] = 99;
    let err = open(&foreign, Some("口令")).unwrap_err();
    assert!(
        err.is_code("sync-channel.envelope-corrupt"),
        "实际: {err:?}"
    );
}

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
    assert_eq!(open(&sealed, Some("口令")).unwrap(), b"payload");
}
