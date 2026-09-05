//! 主口令本机缓存（issue #574 / ADR-0075 决策 3）：把**主口令本身**（非派生密钥）
//! 缓存于系统钥匙串，启动有缓存时自动通过进入应用；生物认证取消、不可用或钥匙串
//! 被清时回退手输——只损失便利，不损失数据。缓存内容为主口令本身，密钥仍由口令
//! 派生，备份跨设备可移植性不受影响（ADR-0075 决策 3 否决「钥匙串存随机密钥」的
//! 决定性理由：备份是 `VACUUM INTO` 文件级快照，密文随密钥走；缓存口令方案下
//! 密钥永远由口令派生，恢复时输口令即验证，备份天然跨设备可恢复）。
//!
//! 运行形态分叉（issue #662「开发态回退」）：钥匙串生物门条目要求 Apple 证书
//! 背书的 `keychain-access-groups` / `application-identifier` entitlement，
//! ad-hoc 签名（含 `tauri dev`）建立/读取必失败 `errSecMissingEntitlement`
//! （-34018）。形态判别收口 [`uses_biometry_gate`] 纯函数（输入构建 profile）：
//! **发布构建维持生物门不变**；开发/未签名构建（`tauri dev` / debug）降级为
//! 无门条目——仍存本机钥匙串、仅本机可读，读取不弹生物认证，本地 dev 的
//! 「自动解锁」不依赖签名基建进度立即可用。两形态共用同一 service/account 的
//! 传统 file-based 钥匙串，`store` 先删后建，形态切换不留混合条目。
//!
//! 数据保护钥匙串（`kSecUseDataProtectionKeychain`）**不采用**（#645 调研评论
//! 的旧实证已过时）：macOS 26 对未签名/ad-hoc 进程的无门 DPK 写入同样报
//! -34018（#662 本机探针实证 macOS 26.6.2 arm64；先例 block/buzz#1266），
//! 开发回退在新增 mac 上唯一可用的存储是无门传统钥匙串条目。代价：ad-hoc
//! 重签（重编译）后条目 ACL 的 cdhash 失配，首次读取可能弹一次钥匙串批准——
//! 仅开发者本机场景，已知可接受。
//!
//! v1 仅 macOS 支持（Windows Hello 视后续调研另接；Linux 及不支持平台回退每次
//! 手输）。非 macOS 平台的实现一律是不支持桩，前端据此隐藏「记住」选项。
//!
//! 真实存储经 `security-framework` 的 macOS Keychain generic password，生物门
//! 形态条目以 `kSecAttrAccessControl` + `BIOMETRY_CURRENT_SET`（Touch ID /
//! 当前生物特征集）保护——读取（`load`）时 macOS 自动弹 Touch ID 门。钥匙串/
//! 生物认证的运行期行为以手动冒烟清单验收（issue #574 acceptance criteria：
//! 钥匙串/生物认证部分不写 CI 自动化测试），本模块保持薄封装、把可测的编排
//! 逻辑留在命令壳层与前端。
//!
//! 本模块是纯基础设施（无领域语义），只被命令壳层经 `commands/encryption.rs`
//! 消费。主口令不落盘于应用可控存储（ADR-0075 后果条款）：仅透明经 IPC 参数
//! 到达（lib.rs `redact_passphrase_payload` 遮蔽），本体存于系统钥匙串。

use crate::error::Result;

/// 非 macOS 不支持桩需要码化错误构造器；macOS 构建下该模块被编译出去，
/// `AppError` 只在 [`imp`] 与下方桩里使用（分层隔离，避免平台条件导入污染）。
#[cfg(not(target_os = "macos"))]
use crate::error::AppError;

/// 钥匙串 service（应用标识符，与 DataLocation 派生同源，见 CONTEXT-reference-settings）。
/// 仅 macOS 的 `imp` 实现消费；非 macOS 桩编译掉该实现，故为 dead code，豁免之。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) const KEYCHAIN_SERVICE: &str = "com.zhangheng.ledger";
/// 钥匙串 account：单一主口令缓存条目。密文库的主口令是**库文件的属性**，不随
/// 位移/搬迁变化，故固定 account、不按 db 路径分键（改口令时同条目覆写）。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) const KEYCHAIN_ACCOUNT: &str = "master-passphrase";

/// 平台是否支持本机记住主口令（v1 仅 macOS）。
pub(crate) fn supported() -> bool {
    cfg!(target_os = "macos")
}

/// 当前进程是否为开发/未签名构建（issue #662）：`tauri dev` 与 debug 构建均属
/// 开发态（`cfg!(debug_assertions)`）；release 构建为发布态。
pub(crate) fn is_dev_build() -> bool {
    cfg!(debug_assertions)
}

/// 形态判别纯函数（issue #662，spec Testing Decisions「后端分支配对」）：输入
/// 开发态布尔（构建 profile，见 [`is_dev_build`]），输出条目是否带生物认证门。
/// 发布构建恒带门（生物认证语义不变）；开发/未签名构建恒无门（开发态回退）。
fn uses_biometry_gate(is_dev_build: bool) -> bool {
    !is_dev_build
}

/// 「本机记住主口令」的运行形态（issue #662）：缓存条目是否带生物认证门。
/// 命令壳层经 `RememberPassphraseSupport.mode` 暴露给前端（wire 形态 kebab-case：
/// `"biometry"` / `"dev-fallback"`），前端据此区分「平台不支持」与「开发构建回退」。
/// pub 与同层 `data_location` 的 wire 类型先例一致（命令面序列化所需）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RememberMode {
    /// 发布构建：条目带 Touch ID 生物认证门（读取弹生物认证）。
    Biometry,
    /// 开发/未签名构建：无门缓存回退（读取不弹生物认证，仍仅本机可读）。
    DevFallback,
}

impl RememberMode {
    /// 由「是否带生物认证门」判定得出形态（与 [`uses_biometry_gate`] 配对成对
    /// 分支，两侧改动必须同步）。
    fn from_gate(gated: bool) -> Self {
        if gated {
            RememberMode::Biometry
        } else {
            RememberMode::DevFallback
        }
    }
}

/// 当前运行形态（命令壳层经 `RememberPassphraseSupport.mode` 暴露给前端）。
pub(crate) fn current_mode() -> RememberMode {
    RememberMode::from_gate(uses_biometry_gate(is_dev_build()))
}

/// 缓存读取结果（区分「有值 / 无缓存 / 生物认证取消」三态，供命令层映射回退路径）。
/// `Found`/`Cancelled` 仅 macOS 的 `load` 构造；非 macOS 桩恒返回 `NotFound`，故豁免死代码。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CacheLoad {
    /// 读到缓存的主口令。
    Found(String),
    /// 钥匙串无条目（从未缓存或被清）——回退手输，不弹生物认证。
    NotFound,
    /// 条目存在但生物认证被取消——回退手输（缓存保留，下次仍可再试）。
    Cancelled,
}

// ---------------------------------------------------------------------------
// macOS 真实实现（security-framework 的 Keychain generic password + 形态分叉）
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod imp {
    use crate::error::{AppError, Result};
    use security_framework::passwords::{
        AccessControlOptions, PasswordOptions, delete_generic_password_options, generic_password,
        set_generic_password_options,
    };
    use security_framework_sys::base::errSecItemNotFound;

    /// Apple Security 框架的 `errSecUserCanceled`（OSStatus = -128），
    /// `security-framework-sys` 未暴露该常量，按既定系统常量定义。
    const ERR_SEC_USER_CANCELED: i32 = -128;

    /// Apple Security 框架的 `errSecMissingEntitlement`（OSStatus = -34018）：
    /// 生物门条目要求 Apple 证书背书的钥匙串 entitlement，ad-hoc 签名进程持无。
    /// `security-framework-sys` 未暴露该常量，按既定系统常量定义。
    const ERR_SEC_MISSING_ENTITLEMENT: i32 = -34018;

    /// -34018 的专属码化错误（issue #662）：不再误报「生物认证未启用或不支持」，
    /// 如实报告「当前构建的签名形态缺少钥匙串 entitlement」。静态消息：本码仅在
    /// -34018 单一条件下触发，状态码数字不进句子（zh 模板与后端消息同形，
    /// ADR-0050）。
    fn entitlement_restricted() -> AppError {
        AppError::coded(
            "encryption.remember-entitlement-restricted",
            "系统钥匙串访问受当前构建签名形态限制（缺少 Apple 证书背书的钥匙串权限）",
        )
    }

    /// 条目查询/属性字典（service + account，传统 file-based 钥匙串）。
    /// 读写删除共用同一字典：两种形态的条目同址，形态切换不留混合条目。
    fn entry_options() -> PasswordOptions {
        PasswordOptions::new_generic_password(super::KEYCHAIN_SERVICE, super::KEYCHAIN_ACCOUNT)
    }

    /// 存储（建/更）缓存的入口令：先删除既有条目（保证 access control 一致——
    /// `SecItemUpdate` 不改 access control，复用 update 路径可能留下旧的异形态
    /// 条目；`delete` 对不存在条目幂等成功，故此处上抛的才是真实失败），再按
    /// 形态新建——生物门形态设 `BIOMETRY_CURRENT_SET`（读取弹 Touch ID）；无门
    /// 形态（开发回退）不设 access control，条目仍存本机钥匙串、仅本机可读。
    pub(super) fn store(passphrase: &str, gated: bool) -> Result<()> {
        delete()?;
        let mut options = entry_options();
        if gated {
            options.set_access_control_options(AccessControlOptions::BIOMETRY_CURRENT_SET);
        }
        set_generic_password_options(passphrase.as_bytes(), options).map_err(|e| {
            if e.code() == ERR_SEC_MISSING_ENTITLEMENT {
                entitlement_restricted()
            } else {
                AppError::coded(
                    "encryption.remember-biometric-unavailable",
                    format!(
                        "无法把主口令存入系统钥匙串（生物认证未启用或该设备不支持）：{}",
                        e.code()
                    ),
                )
            }
        })
    }

    /// 读取缓存的入口令：生物门形态的条目在此弹 Touch ID 门；无门形态
    /// （开发回退）直接读出、不弹生物认证。查询字典与形态无关（同 service +
    /// account 同址）。
    pub(super) fn load() -> Result<super::CacheLoad> {
        match generic_password(entry_options()) {
            Ok(bytes) => Ok(super::CacheLoad::Found(
                String::from_utf8_lossy(&bytes).into_owned(),
            )),
            Err(e) if e.code() == errSecItemNotFound => Ok(super::CacheLoad::NotFound),
            Err(e) if e.code() == ERR_SEC_USER_CANCELED => Ok(super::CacheLoad::Cancelled),
            Err(e) if e.code() == ERR_SEC_MISSING_ENTITLEMENT => Err(entitlement_restricted()),
            Err(e) => Err(AppError::Io(format!("钥匙串读取失败：{}", e.code()))),
        }
    }

    /// 删除缓存的入口令（幂等：条目不存在视为成功）。
    pub(super) fn delete() -> Result<()> {
        match delete_generic_password_options(entry_options()) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == errSecItemNotFound => Ok(()),
            Err(e) => Err(AppError::Io(format!("钥匙串删除失败：{}", e.code()))),
        }
    }
}

/// 存储（建/更）缓存的入口令（形态由构建 profile 判定，issue #662：发布构建
/// 带生物认证门，开发/未签名构建无门回退）。
#[cfg(target_os = "macos")]
pub(crate) fn store(passphrase: &str) -> Result<()> {
    imp::store(passphrase, uses_biometry_gate(is_dev_build()))
}

/// 读取缓存的入口令（生物门形态触发生物认证，三态见 [`CacheLoad`]；开发态
/// 无门直接读出）。
#[cfg(target_os = "macos")]
pub(crate) fn load() -> Result<CacheLoad> {
    imp::load()
}

/// 删除缓存的入口令（幂等）。
#[cfg(target_os = "macos")]
pub(crate) fn delete() -> Result<()> {
    imp::delete()
}

// ---------------------------------------------------------------------------
// 非 macOS 不支持桩（前端据 [`supported`] 隐藏选项，这些路径实际不被触达）
// ---------------------------------------------------------------------------

/// 存储（建/更）缓存的入口令：不支持平台统一报码化错误（前端隐藏选项即不会触达）。
#[cfg(not(target_os = "macos"))]
pub(crate) fn store(_passphrase: &str) -> Result<()> {
    Err(AppError::coded(
        "encryption.remember-unsupported",
        "当前平台不支持本机记住主口令",
    ))
}

/// 读取缓存的入口令：不支持平台视为无缓存（回退手输）。
#[cfg(not(target_os = "macos"))]
pub(crate) fn load() -> Result<CacheLoad> {
    Ok(CacheLoad::NotFound)
}

/// 删除缓存的入口令：不支持平台幂等成功。
#[cfg(not(target_os = "macos"))]
pub(crate) fn delete() -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 形态判别纯函数（issue #662）：发布构建（非开发态）恒带生物认证门——
    /// 发布形态行为零变化。
    #[test]
    fn release_build_keeps_biometry_gate() {
        assert!(uses_biometry_gate(false));
    }

    /// 开发/未签名构建降级为无门条目（本地 dev 免 Touch ID 自动解锁立即可用）。
    #[test]
    fn dev_build_drops_biometry_gate() {
        assert!(!uses_biometry_gate(true));
    }

    /// 形态枚举与门判定配对：带门 ↔ biometry，无门 ↔ dev-fallback（spec
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
}
