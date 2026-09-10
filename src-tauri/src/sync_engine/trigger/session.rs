//! 本机会话密钥形态（issue #958 拆分自 `trigger.rs`；#863 / ADR-0098 决策 3）：
//! 解锁密文库或一次成功的手动同步后记入，同步轮次据此判定信封模式；进程级单例，
//! 与解锁态同生命周期。
//!
//! 变更原因单一：改「口令从哪来」或失效时机只动本文件。轮次编排对它的消费见
//! [`super::scheduler`]，通道配置见 [`super::channel`]。
//!
//! 归本域而非基础设施：它是**同步触发**的工程决策（自动轮询不得弹生物认证），
//! 除同步轮次外无消费者。口令本体的钥匙串缓存仍归备份域基础设施
//! （`db::passphrase_cache`），本单例只持「本会话已知的形态」。

use crate::sync_engine::envelope::EnvelopeMode;

/// 本会话密钥记忆（ADR-0098）：解锁密文库或一次成功的手动同步后记入，
/// 同步轮次据此判定信封模式。进程级单例，与解锁态同生命周期。
static SESSION_ENVELOPE: std::sync::Mutex<Option<SessionEnvelope>> = std::sync::Mutex::new(None);

/// 本机会话的密钥形态：解锁密文库或一次成功的手动同步后记入，同步轮次
/// 据此判定信封模式。
///
/// 自动轮询**不读钥匙串**：钥匙串读取在发布构建下先过 LocalAuthentication 门
/// （弹 Touch ID，ADR-0075 决策 3 / issue #866），后台轮询不得弹交互；本会话
/// 没有密钥知识时按明文形态跑（明文库的正常形态），密文库未解锁时业务 IPC
/// 本就不可达（门禁拦截）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEnvelope {
    /// 明文库会话：同步明文直通（界面显著提示，ADR-0091 决策 8）。
    Plaintext,
    /// 密文库会话：会话口令在场，同步以之封包。
    Encrypted(String),
}

impl SessionEnvelope {
    /// 记入本会话形态（解锁成功 / 一次成功的手动同步 / 用户设置记住口令）。
    pub fn remember(session: SessionEnvelope) {
        *SESSION_ENVELOPE.lock().unwrap_or_else(|e| e.into_inner()) = Some(session);
    }

    /// 清空本会话记忆（引导换库、忘记口令重置、关闭加密等改变库身份的路径）：
    /// 新库形态未知，等下一次解锁/手动同步重新记入——避免拿旧库口令去封新库的段
    /// （密文/明文错配会让对端无法开封）。
    pub fn forget() {
        *SESSION_ENVELOPE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// 读取当前会话形态（未记入回 [`SessionEnvelope::Plaintext`]：明文库形态）。
    pub fn current() -> Self {
        SESSION_ENVELOPE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or(SessionEnvelope::Plaintext)
    }

    /// 对应的信封模式（借出形态，轮次消费）。
    pub fn mode(&self) -> EnvelopeMode<'_> {
        match self {
            Self::Plaintext => EnvelopeMode::Plaintext,
            Self::Encrypted(passphrase) => EnvelopeMode::Encrypted { passphrase },
        }
    }
}
