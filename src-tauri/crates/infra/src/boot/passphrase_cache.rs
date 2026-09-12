//! 主口令本机缓存（issue #574 / ADR-0075 决策 3）：把**主口令本身**（非派生密钥）
//! 缓存于系统钥匙串，启动有缓存时自动通过进入应用；生物认证取消、不可用或钥匙串
//! 被清时回退手输——只损失便利，不损失数据。缓存内容为主口令本身，密钥仍由口令
//! 派生，备份跨设备可移植性不受影响（ADR-0075 决策 3 否决「钥匙串存随机密钥」的
//! 决定性理由：备份是 `VACUUM INTO` 文件级快照，密文随密钥走；缓存口令方案下
//! 密钥永远由口令派生，恢复时输口令即验证，备份天然跨设备可恢复）。
//!
//! 生物门形态（issue #866，修订 ADR-0075）：item 级 ACL 生物门对 Developer ID
//! 分发不可用（#657 证据矩阵实测，macOS 26.6.2 arm64：不挂 entitlements 报
//! -34018；挂 `keychain-access-groups` 被 AMFI SIGKILL；只挂
//! `application-identifier` 钥匙串不认），生物门改用 **LocalAuthentication
//! 应用层门**：钥匙串条目普通写入（无 ACL），读取（[`load`]）前先经
//! `LAContext.evaluatePolicy(.deviceOwnerAuthenticationWithBiometrics)` 弹
//! Touch ID，验证通过才读条目；用户取消映射 [`CacheLoad::Cancelled`]，生物
//! 认证不可用映射 [`CacheLoad::NotFound`]，与既有回退语义一致（均回退手输，
//! 缓存保留）。Rust 侧 LocalAuthentication 绑定用 objc2 系 crate
//! （`objc2-local-authentication`，仅 macOS），hardened runtime 下 LAContext
//! 可用已被 #657 证据矩阵 4 实证（Developer ID + hardened runtime 完整走通）。
//! 相对 ACL 门的已知行为差异：生物特征重录**不再使缓存失效**（门在读取时
//! 评估当前生物特征，不与条目绑定）。
//!
//! 运行形态分叉（issue #662「开发态回退」）：形态判别收口 `uses_biometry_gate`（本模块
//! 私有纯函数，输入构建 profile）：**发布构建读取前过 LA 门**；开发/未签名构建
//! （`tauri dev` / debug）降级为无门形态——写入与发布形态完全相同（普通条目），
//! 读取不弹生物认证直接读出，本地 dev 的「自动解锁」不依赖签名基建进度立即
//! 可用。两形态共用同一 service/account 的传统 file-based 钥匙串，`store`
//! 先删后建，形态切换不留混合条目。
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
//! 真实存储经 `security-framework` 的 macOS Keychain generic password（两种
//! 形态均为无 ACL 普通条目）；生物门在读取路径由 LocalAuthentication 承担。
//! 钥匙串/生物认证的运行期行为以手动冒烟清单验收（issue #574 acceptance
//! criteria：钥匙串/生物认证部分不写 CI 自动化测试），本模块保持薄封装、把
//! 可测的编排逻辑留在命令壳层与前端。
//!
//! 本模块是纯基础设施（无领域语义），只被命令壳层经 `commands/encryption.rs`
//! 消费。主口令不落盘于应用可控存储（ADR-0075 后果条款）：仅透明经 IPC 参数
//! 到达（lib.rs `redact_passphrase_payload` 遮蔽），本体存于系统钥匙串。
//!
//! 多端同步自动轮次的「本会话密钥形态」记忆**不在此**（归同步域
//! `sync_engine::SessionEnvelope`）：它是同步触发的工程决策，除同步轮次外无
//! 消费者，放基础设施会平白造出基础设施→域的依赖。

use crate::error::Result;

/// 非 macOS 不支持桩需要码化错误构造器；macOS 构建下该模块被编译出去，
/// `AppError` 只在 [`imp`] 与下方桩里使用（分层隔离，避免平台条件导入污染）。
#[cfg(not(target_os = "macos"))]
use crate::error::AppError;

/// 钥匙串 service（应用标识符，与 DataLocation 派生同源，见 CONTEXT-reference-settings）。
/// 仅 macOS 的 `imp` 实现消费；非 macOS 桩编译掉该实现，故为 dead code，豁免之。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) const KEYCHAIN_SERVICE: &str = "com.zhangheng.ledger";
/// 钥匙串 account 前缀。多账本（issue #836 / ADR-0089 决策 3「自动解锁缓存按
/// 账本区分」）后 account 携带账本标识：`master-passphrase[-<账本标识>]`——
/// 每本独立缓存与清除，切到无缓存的密文库仍落解锁屏。主口令是**库文件的
/// 属性**，不随位移/搬迁变化，故 account 不含库路径（改口令时同条目覆写）。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) const KEYCHAIN_ACCOUNT_PREFIX: &str = "master-passphrase";
/// 多账本之前的历史 account（无账本标识）。升级后不再读写，仅在建立新条目时
/// 顺手清除（幂等 best-effort，见 [`store`]），避免遗留不可达的口令缓存。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) const KEYCHAIN_LEGACY_ACCOUNT: &str = "master-passphrase";

/// 缓存条目的 account（纯函数）：`Some(账本标识)` → 按本分域；`None`（注册表
/// 不可用，应用处于回退默认目录的现场）→ 历史无标识 account——回退现场运行
/// 的正是折叠默认账本，历史条目本就属于它，升级用户在此现场不丢自动解锁。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn account_for(book: Option<&str>) -> String {
    match book {
        Some(id) => format!("{KEYCHAIN_ACCOUNT_PREFIX}-{id}"),
        None => KEYCHAIN_LEGACY_ACCOUNT.to_string(),
    }
}

/// 平台是否支持本机记住主口令（v1 仅 macOS）。
pub fn supported() -> bool {
    cfg!(target_os = "macos")
}

/// 当前进程是否为开发/未签名构建（issue #662）：`tauri dev` 与 debug 构建均属
/// 开发态（`cfg!(debug_assertions)`）；release 构建为发布态。
pub(crate) fn is_dev_build() -> bool {
    cfg!(debug_assertions)
}

/// 形态判别纯函数（issue #662，spec Testing Decisions「后端分支配对」）：输入
/// 开发态布尔（构建 profile，见 [`is_dev_build`]），输出读取是否先过生物认证
/// 门（issue #866 起为 LocalAuthentication 应用层门，不再写条目 ACL）。
/// 发布构建恒带门（生物认证语义不变）；开发/未签名构建恒无门（开发态回退）。
/// `pub(super)`：外挂单测 `boot/tests/passphrase_cache.rs` 直测本接缝——私有项
/// 仅定义模块及其后代可见，测试平移到兄弟目录后需放宽到 boot 子树。
pub(super) fn uses_biometry_gate(is_dev_build: bool) -> bool {
    !is_dev_build
}

/// 「本机记住主口令」的运行形态（issue #662）：读取是否先过生物认证门。
/// 命令壳层经 `RememberPassphraseSupport.mode` 暴露给前端（wire 形态 kebab-case：
/// `"biometry"` / `"dev-fallback"`），前端据此区分「平台不支持」与「开发构建回退」。
/// pub 与同层 `data_location` 的 wire 类型先例一致（命令面序列化所需）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RememberMode {
    /// 发布构建：读取前先过 LocalAuthentication 应用层门（弹 Touch ID；issue
    /// #866 起条目本身无 ACL，门由读取路径承担）。
    Biometry,
    /// 开发/未签名构建：无门缓存回退（读取不弹生物认证，仍仅本机可读）。
    DevFallback,
}

impl RememberMode {
    /// 由「读取是否过门」判定得出形态（与 [`uses_biometry_gate`] 配对成对
    /// 分支，两侧改动必须同步）。`pub(super)` 同 [`uses_biometry_gate`]：
    /// 外挂单测直测形态判定接缝。
    pub(super) fn from_gate(gated: bool) -> Self {
        if gated {
            RememberMode::Biometry
        } else {
            RememberMode::DevFallback
        }
    }
}

/// 当前运行形态（命令壳层经 `RememberPassphraseSupport.mode` 暴露给前端）。
pub fn current_mode() -> RememberMode {
    RememberMode::from_gate(uses_biometry_gate(is_dev_build()))
}

/// 缓存读取结果（区分「有值 / 无缓存 / 生物认证取消」三态，供命令层映射回退路径）。
/// `Found`/`Cancelled` 仅 macOS 的 `load` 构造；非 macOS 桩恒返回 `NotFound`，故豁免死代码。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, PartialEq, Eq)]
pub enum CacheLoad {
    /// 读到缓存的主口令。
    Found(String),
    /// 钥匙串无条目（从未缓存或被清）——回退手输，不弹生物认证。
    NotFound,
    /// 条目存在但生物认证被取消——回退手输（缓存保留，下次仍可再试）。
    Cancelled,
}

// ---------------------------------------------------------------------------
// macOS 真实实现（security-framework 的 Keychain generic password + 形态分叉；
// 生物门 = LocalAuthentication 应用层门，issue #866）
// ---------------------------------------------------------------------------

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

    /// Apple Security 框架的 `errSecUserCanceled`（OSStatus = -128），
    /// `security-framework-sys` 未暴露该常量，按既定系统常量定义。
    const ERR_SEC_USER_CANCELED: i32 = -128;

    /// Apple Security 框架的 `errSecMissingEntitlement`（OSStatus = -34018）：
    /// 钥匙串受限形态（如历史 ACL 条目、数据保护钥匙串）在 ad-hoc 签名进程下
    /// 报此错。`security-framework-sys` 未暴露该常量，按既定系统常量定义。
    const ERR_SEC_MISSING_ENTITLEMENT: i32 = -34018;

    /// LocalAuthentication 门的弹窗说明语（系统 Touch ID 弹窗内的用户可见文案）。
    /// 已知取舍（code review 议定）：系统弹窗不经前端渲染，后端也没有用户语言
    /// 接缝（界面语言是前端轻量设置项），故与其它后端用户可见消息同一静态
    /// zh-CN 口径；将来若要本地化，接缝是前端经命令面传入 localizedReason。
    const LA_PROMPT_REASON: &str = "验证以自动解锁（读取已记住的主口令）";

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
    /// 读写删除共用同一字典：两种形态的条目同址，形态切换不留混合条目；
    /// account 由账本标识派生（[`super::account_for`]，issue #836 按本分域）。
    fn entry_options(book: Option<&str>) -> PasswordOptions {
        PasswordOptions::new_generic_password(super::KEYCHAIN_SERVICE, &super::account_for(book))
    }

    /// LocalAuthentication 应用层门的评估结局（issue #866）。
    #[derive(Debug, PartialEq, Eq)]
    enum BiometryOutcome {
        /// 生物认证通过，可以读取条目。
        Passed,
        /// 用户取消/认证未通过/会话被中断——回退手输，缓存保留。
        Cancelled,
        /// 生物认证不可用（未启用/不支持/被锁定/未设密码）——读取在门前止步，
        /// 归入「无可达缓存」回退手输。
        Unavailable,
    }

    /// 把 LAError 码映射为门结局：用户发起或会话中断属「取消」；生物认证
    /// 本身不可用属「不可用」（未知码保守归入不可用，宁多退手输不误进）。
    /// `AuthenticationFailed`（多次认证未通过）归「取消」：两条回退路径相同，
    /// 复用既有「生物认证已取消」模板就近提示，不为它新增错误码/模板（
    /// code review 议定：文案不完全贴切但优于「没有缓存」的误导）。
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

    /// 弹 Touch ID 并阻塞到系统回执（issue #866）：
    /// `LAContext.evaluatePolicy(.deviceOwnerAuthenticationWithBiometrics)`。
    /// 生成绑定只含异步 reply 变体，用堆上 block + 通道桥接成同步（本函数仅经
    /// 连接层 `run_db` 在阻塞线程池执行，不占用界面事件循环线程；解锁屏对等待
    /// 有界，issue #644）。Developer ID + hardened runtime 下可用已被 #657 证据
    /// 矩阵 4 实证。`LAContext` 存活至回执到达后才析构，不会 mid-evaluation
    /// 失效（`LAErrorInvalidContext`）。
    fn evaluate_biometrics() -> BiometryOutcome {
        let context = unsafe { LAContext::new() };
        // 载荷 = (是否通过，LAError 码)；不用 crate 的 `Result` 别名以免混淆。
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
            // 回执通道断裂（理论上不可能：block 持发送端、本函数持接收端且
            // 阻塞等待）。保守按不可用处理，回退手输。
            Err(_) => BiometryOutcome::Unavailable,
        }
    }

    /// 存储（建/更）缓存的入口令：先删除既有条目（`SecItemUpdate` 不改 access
    /// control，复用 update 路径可能留下旧的异形态条目；`delete` 对不存在条目
    /// 幂等成功，故此处上抛的才是真实失败），再普通新建——issue #866 起两种
    /// 形态写入完全相同（无 ACL 普通条目，生物门改由读取路径承担）；`gated`
    /// 形态参数保留为形态接缝，与 [`load`] 的门判定同源（[`super::uses_biometry_gate`]）。
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

    /// 读取缓存的入口令：生物门形态先过 LocalAuthentication 应用层门——验证
    /// 通过才读条目，取消/不可用按门结局返回（均回退手输，条目保留）；无门
    /// 形态（开发回退）直接读出、不弹生物认证。查询字典与形态无关（同
    /// service + account 同址，account 由账本标识派生）。历史 ACL 条目（#866
    /// 之前发布形态建立）由 `store` 先删后建自然迁移为普通条目。
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

    /// 删除缓存的入口令（幂等：条目不存在视为成功）。
    pub(super) fn delete(book: Option<&str>) -> Result<()> {
        match delete_generic_password_options(entry_options(book)) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == errSecItemNotFound => Ok(()),
            Err(e) => Err(AppError::Io(format!("钥匙串删除失败：{}", e.code()))),
        }
    }
}

/// 存储（建/更）缓存的入口令（形态由构建 profile 判定，issue #662：发布构建
/// 读取前过生物认证门，开发/未签名构建无门回退；两形态写入相同，issue #866）。
/// `book`：条目按账本标识分域（issue #836）；携带标识时顺手清除历史无标识
/// 条目（幂等 best-effort，失败不阻断——升级后遗留的旧条目不再被任何路径
/// 消费，清不掉也无害，仅少一次钥匙串清理）。
#[cfg(target_os = "macos")]
pub fn store(passphrase: &str, book: Option<&str>) -> Result<()> {
    if book.is_some() {
        let _ = imp::delete(None);
    }
    imp::store(passphrase, book, uses_biometry_gate(is_dev_build()))
}

/// 读取缓存的入口令（生物门形态先过 LocalAuthentication 应用层门再读条目，
/// 三态见 [`CacheLoad`]；开发态无门直接读出，issue #866）。`book` 见 [`store`]。
#[cfg(target_os = "macos")]
pub fn load(book: Option<&str>) -> Result<CacheLoad> {
    imp::load(book, uses_biometry_gate(is_dev_build()))
}

/// 删除缓存的入口令（幂等）。`book` 见 [`store`]。
#[cfg(target_os = "macos")]
pub fn delete(book: Option<&str>) -> Result<()> {
    imp::delete(book)
}

// ---------------------------------------------------------------------------
// 非 macOS 不支持桩（前端据 [`supported`] 隐藏选项，这些路径实际不被触达）
// ---------------------------------------------------------------------------

/// 存储（建/更）缓存的入口令：不支持平台统一报码化错误（前端隐藏选项即不会触达）。
#[cfg(not(target_os = "macos"))]
pub fn store(_passphrase: &str, _book: Option<&str>) -> Result<()> {
    Err(AppError::coded(
        "encryption.remember-unsupported",
        "当前平台不支持本机记住主口令",
    ))
}

/// 读取缓存的入口令：不支持平台视为无缓存（回退手输）。
#[cfg(not(target_os = "macos"))]
pub fn load(_book: Option<&str>) -> Result<CacheLoad> {
    Ok(CacheLoad::NotFound)
}

/// 删除缓存的入口令：不支持平台幂等成功。
#[cfg(not(target_os = "macos"))]
pub fn delete(_book: Option<&str>) -> Result<()> {
    Ok(())
}
