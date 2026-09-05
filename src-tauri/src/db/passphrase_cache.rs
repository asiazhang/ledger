//! 主口令本机缓存（issue #574 / ADR-0075 决策 3）：把**主口令本身**（非派生密钥）
//! 缓存于系统钥匙串，macOS 挂生物认证门（Touch ID）。此后启动有缓存时静默或经
//! 生物认证自动通过进入应用；生物认证取消、不可用或钥匙串被清时回退手输——
//! 只损失便利，不损失数据。缓存内容为主口令本身，密钥仍由口令派生，备份跨
//! 设备可移植性不受影响（ADR-0075 决策 3 否决「钥匙串存随机密钥」的决定性理由：
//! 备份是 `VACUUM INTO` 文件级快照，密文随密钥走；缓存口令方案下密钥永远由口令
//! 派生，恢复时输口令即验证，备份天然跨设备可恢复）。
//!
//! v1 仅 macOS 支持（Windows Hello 视后续调研另接；Linux 及不支持平台回退每次
//! 手输）。非 macOS 平台的实现一律是不支持桩，前端据此隐藏「记住」选项。
//!
//! 真实存储经 `security-framework` 的 macOS Keychain generic password，条目以
//! `kSecAttrAccessControl` + `BIOMETRY_CURRENT_SET`（Touch ID / 当前生物特征集）
//! 保护——读取（`load`）时 macOS 自动弹 Touch ID 门。钥匙串/生物认证的运行期
//! 行为以手动冒烟清单验收（issue #574 acceptance criteria：钥匙串/生物认证部分
//! 不写 CI 自动化测试），本模块保持薄封装、把可测的编排逻辑留在命令壳层与前端。
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
// macOS 真实实现（security-framework 的 Keychain generic password + 生物门）
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

    /// 存储（建/更）缓存的入口令：先删除既有条目（保证 biometric access
    /// control 一致——`SecItemUpdate` 不改 access control，复用 update 路径可能
    /// 留下旧的无门条目；`delete` 对不存在条目幂等成功，故此处上抛的才是真实失败），
    /// 再以生物认证门新建。
    pub(super) fn store(passphrase: &str) -> Result<()> {
        delete()?;
        let mut options =
            PasswordOptions::new_generic_password(super::KEYCHAIN_SERVICE, super::KEYCHAIN_ACCOUNT);
        options.set_access_control_options(AccessControlOptions::BIOMETRY_CURRENT_SET);
        set_generic_password_options(passphrase.as_bytes(), options).map_err(|e| {
            AppError::coded(
                "encryption.remember-biometric-unavailable",
                format!(
                    "无法把主口令存入系统钥匙串（生物认证未启用或该设备不支持）：{}",
                    e.code()
                ),
            )
        })
    }

    /// 读取缓存的入口令：条目存在且有生物门时，macOS 在此弹 Touch ID 门。
    pub(super) fn load() -> Result<super::CacheLoad> {
        let options =
            PasswordOptions::new_generic_password(super::KEYCHAIN_SERVICE, super::KEYCHAIN_ACCOUNT);
        match generic_password(options) {
            Ok(bytes) => Ok(super::CacheLoad::Found(
                String::from_utf8_lossy(&bytes).into_owned(),
            )),
            Err(e) if e.code() == errSecItemNotFound => Ok(super::CacheLoad::NotFound),
            Err(e) if e.code() == ERR_SEC_USER_CANCELED => Ok(super::CacheLoad::Cancelled),
            Err(e) => Err(AppError::Io(format!("钥匙串读取失败：{}", e.code()))),
        }
    }

    /// 删除缓存的入口令（幂等：条目不存在视为成功）。
    pub(super) fn delete() -> Result<()> {
        let options =
            PasswordOptions::new_generic_password(super::KEYCHAIN_SERVICE, super::KEYCHAIN_ACCOUNT);
        match delete_generic_password_options(options) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == errSecItemNotFound => Ok(()),
            Err(e) => Err(AppError::Io(format!("钥匙串删除失败：{}", e.code()))),
        }
    }
}

/// 存储（建/更）缓存的入口令（macOS 以生物认证门保护）。
#[cfg(target_os = "macos")]
pub(crate) fn store(passphrase: &str) -> Result<()> {
    imp::store(passphrase)
}

/// 读取缓存的入口令（macOS 触发生物认证门；三态见 [`CacheLoad`]）。
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
