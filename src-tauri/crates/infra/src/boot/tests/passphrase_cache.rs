//! [`passphrase_cache`](crate::boot::passphrase_cache) 单元测试：本机记忆主口令
//! 的运行形态判别（issue #662）与 wire 形态、缓存条目 account 分域（issue #836）。
//!
//! 原为生产文件内联 `#[cfg(test)] mod tests`，随 `boot/tests/` 统一外挂
//! （与 `db/tests/` 同形）迁出；形态判别与门判定配对仍直测同一纯函数接缝。

use crate::boot::passphrase_cache::{RememberMode, account_for, uses_biometry_gate};

/// 形态判别纯函数（issue #662）：发布构建（非开发态）恒过生物认证门——
/// 发布形态行为零变化（#866 起门为读取前 LocalAuthentication 应用层门）。
#[test]
fn release_build_keeps_biometry_gate() {
    assert!(uses_biometry_gate(false));
}

/// 开发/未签名构建降级为无门形态（本地 dev 免 Touch ID 自动解锁立即可用）。
#[test]
fn dev_build_drops_biometry_gate() {
    assert!(!uses_biometry_gate(true));
}

/// 形态枚举与门判定配对：过门 ↔ biometry，无门 ↔ dev-fallback（spec
/// Testing Decisions「后端分支配对」——两侧分支由同一纯函数钉住）。
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

/// wire 形态钉死（kebab-case）：码即对外契约，序列化值改名等于破坏前端。
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

/// 缓存条目 account 按账本标识分域（issue #836）：携带标识 → 按本分域；
/// 无标识（注册表不可用的回退现场，运行的是折叠默认账本）→ 历史无标识
/// account——升级用户在回退现场不丢自动解锁。
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
