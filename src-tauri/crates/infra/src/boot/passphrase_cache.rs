use crate::error::Result;

#[cfg(not(target_os = "macos"))]
use crate::error::AppError;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) const KEYCHAIN_SERVICE: &str = "com.zhangheng.ledger";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) const KEYCHAIN_ACCOUNT_PREFIX: &str = "master-passphrase";
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) const KEYCHAIN_LEGACY_ACCOUNT: &str = "master-passphrase";

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn account_for(book: Option<&str>) -> String {
    match book {
        Some(id) => format!("{KEYCHAIN_ACCOUNT_PREFIX}-{id}"),
        None => KEYCHAIN_LEGACY_ACCOUNT.to_string(),
    }
}

pub fn supported() -> bool {
    cfg!(target_os = "macos")
}

pub(crate) fn is_dev_build() -> bool {
    cfg!(debug_assertions)
}

pub(super) fn uses_biometry_gate(is_dev_build: bool) -> bool {
    !is_dev_build
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RememberMode {
    Biometry,
    DevFallback,
}

impl RememberMode {
    pub(super) fn from_gate(gated: bool) -> Self {
        if gated {
            RememberMode::Biometry
        } else {
            RememberMode::DevFallback
        }
    }
}

pub fn current_mode() -> RememberMode {
    RememberMode::from_gate(uses_biometry_gate(is_dev_build()))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, PartialEq, Eq)]
pub enum CacheLoad {
    Found(String),
    NotFound,
    Cancelled,
}

#[cfg(target_os = "macos")]
mod imp {
    use block2::RcBlock;
    use objc2::ffi::NSInteger;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSError, NSString};
    use objc2_local_authentication::{LAContext, LAError, LAPolicy};

    use crate::error::{AppError, Result};
    use security_framework::passwords::{
        PasswordOptions, delete_generic_password_options, generic_password,
        set_generic_password_options,
    };
    use security_framework_sys::base::errSecItemNotFound;

    const ERR_SEC_USER_CANCELED: i32 = -128;

    const ERR_SEC_MISSING_ENTITLEMENT: i32 = -34018;

    const LA_PROMPT_REASON: &str = "验证以自动解锁（读取已记住的主口令）";

    fn entitlement_restricted() -> AppError {
        AppError::coded(
            "encryption.remember-entitlement-restricted",
            "系统钥匙串访问受当前构建签名形态限制（缺少 Apple 证书背书的钥匙串权限）",
        )
    }

    fn entry_options(book: Option<&str>) -> PasswordOptions {
        PasswordOptions::new_generic_password(super::KEYCHAIN_SERVICE, &super::account_for(book))
    }

    #[derive(Debug, PartialEq, Eq)]
    enum BiometryOutcome {
        Passed,
        Cancelled,
        Unavailable,
    }

    fn map_la_error(code: NSInteger) -> BiometryOutcome {
        match LAError(code) {
            LAError::AuthenticationFailed
            | LAError::UserCancel
            | LAError::UserFallback
            | LAError::SystemCancel
            | LAError::AppCancel
            | LAError::InvalidContext => BiometryOutcome::Cancelled,
            LAError::PasscodeNotSet
            | LAError::BiometryNotAvailable
            | LAError::BiometryNotEnrolled
            | LAError::BiometryLockout
            | LAError::NotInteractive => BiometryOutcome::Unavailable,
            _ => BiometryOutcome::Unavailable,
        }
    }

    fn evaluate_biometrics() -> BiometryOutcome {
        let context = unsafe { LAContext::new() };
        let (tx, rx) = std::sync::mpsc::channel::<(bool, NSInteger)>();
        let reply = RcBlock::new(move |success: Bool, error: *mut NSError| {
            let result = if success.as_bool() {
                (true, 0)
            } else if error.is_null() {
                (false, 0)
            } else {
                (false, unsafe { (*error).code() })
            };
            let _ = tx.send(result);
        });
        unsafe {
            context.evaluatePolicy_localizedReason_reply(
                LAPolicy::DeviceOwnerAuthenticationWithBiometrics,
                &NSString::from_str(LA_PROMPT_REASON),
                &reply,
            );
        }
        match rx.recv() {
            Ok((true, _)) => BiometryOutcome::Passed,
            Ok((false, code)) => map_la_error(code),
            Err(_) => BiometryOutcome::Unavailable,
        }
    }

    pub(super) fn store(passphrase: &str, book: Option<&str>, _gated: bool) -> Result<()> {
        delete(book)?;
        let options = entry_options(book);
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

    pub(super) fn load(book: Option<&str>, gated: bool) -> Result<super::CacheLoad> {
        if gated {
            match evaluate_biometrics() {
                BiometryOutcome::Passed => {}
                BiometryOutcome::Cancelled => return Ok(super::CacheLoad::Cancelled),
                BiometryOutcome::Unavailable => return Ok(super::CacheLoad::NotFound),
            }
        }
        match generic_password(entry_options(book)) {
            Ok(bytes) => Ok(super::CacheLoad::Found(
                String::from_utf8_lossy(&bytes).into_owned(),
            )),
            Err(e) if e.code() == errSecItemNotFound => Ok(super::CacheLoad::NotFound),
            Err(e) if e.code() == ERR_SEC_USER_CANCELED => Ok(super::CacheLoad::Cancelled),
            Err(e) if e.code() == ERR_SEC_MISSING_ENTITLEMENT => Err(entitlement_restricted()),
            Err(e) => Err(AppError::Io(format!("钥匙串读取失败：{}", e.code()))),
        }
    }

    pub(super) fn delete(book: Option<&str>) -> Result<()> {
        match delete_generic_password_options(entry_options(book)) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == errSecItemNotFound => Ok(()),
            Err(e) => Err(AppError::Io(format!("钥匙串删除失败：{}", e.code()))),
        }
    }
}

#[cfg(target_os = "macos")]
pub fn store(passphrase: &str, book: Option<&str>) -> Result<()> {
    if book.is_some() {
        let _ = imp::delete(None);
    }
    imp::store(passphrase, book, uses_biometry_gate(is_dev_build()))
}

#[cfg(target_os = "macos")]
pub fn load(book: Option<&str>) -> Result<CacheLoad> {
    imp::load(book, uses_biometry_gate(is_dev_build()))
}

#[cfg(target_os = "macos")]
pub fn delete(book: Option<&str>) -> Result<()> {
    imp::delete(book)
}

#[cfg(not(target_os = "macos"))]
pub fn store(_passphrase: &str, _book: Option<&str>) -> Result<()> {
    Err(AppError::coded(
        "encryption.remember-unsupported",
        "当前平台不支持本机记住主口令",
    ))
}

#[cfg(not(target_os = "macos"))]
pub fn load(_book: Option<&str>) -> Result<CacheLoad> {
    Ok(CacheLoad::NotFound)
}

#[cfg(not(target_os = "macos"))]
pub fn delete(_book: Option<&str>) -> Result<()> {
    Ok(())
}
