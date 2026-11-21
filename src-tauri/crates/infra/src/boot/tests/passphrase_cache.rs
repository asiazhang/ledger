use crate::boot::passphrase_cache::{RememberMode, account_for, uses_biometry_gate};

#[test]
fn release_build_keeps_biometry_gate() {
    assert!(uses_biometry_gate(false));
}

#[test]
fn dev_build_drops_biometry_gate() {
    assert!(!uses_biometry_gate(true));
}

#[test]
fn mode_follows_gate_decision() {
    assert_eq!(
        RememberMode::from_gate(uses_biometry_gate(false)),
        RememberMode::Biometry
    );
    assert_eq!(
        RememberMode::from_gate(uses_biometry_gate(true)),
        RememberMode::DevFallback
    );
}

#[test]
fn mode_serializes_to_kebab_case() {
    assert_eq!(
        serde_json::to_value(RememberMode::Biometry).unwrap(),
        "biometry"
    );
    assert_eq!(
        serde_json::to_value(RememberMode::DevFallback).unwrap(),
        "dev-fallback"
    );
}

#[test]
fn account_scopes_by_book_id() {
    assert_eq!(account_for(None), "master-passphrase");
    assert_eq!(
        account_for(Some("3f2a9c4e-8b1d-4c2a-9f3e-5a7b8c9d0e1f")),
        "master-passphrase-3f2a9c4e-8b1d-4c2a-9f3e-5a7b8c9d0e1f"
    );
    assert_eq!(
        account_for(Some("ab12cd34ef56ab12")),
        "master-passphrase-ab12cd34ef56ab12"
    );
}
