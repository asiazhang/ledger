//! SyncEnvelope 同步信封（issue #859 / ADR-0091 决策 8）：通道上传输文件的
//! 整包加密层——Checkpoint 与 OpLog 段文件逐文件封包，密钥由主口令经密钥派生
//! 导出（PBKDF2-HMAC-SHA512，迭代量级与 SQLCipher 默认 KDF 一致），主口令
//! 不随信封走；AES-256-GCM 提供机密性与密文认证。
//!
//! 自描述双形态：信封以固定魔数开头，开封按文件自身形态判定——密文信封
//! 凭口令解封（缺口令/错口令/篡改均报可重试的码化错误，与密文备份恢复同款
//! 合并口径）；无魔数的输入按明文原样直通（未开加密模式的明文同步形态）。
//! 加密模式由调用方按本库加密形态决定（复用备份域加密模式，#862 接线），
//! 明文模式逐字节直通、不产出信封结构。
//!
//! 密文粒度 = 单个文件（段/快照各自封包），同一主口令派生同一密钥；盐与
//! nonce 逐封包随机，同一原文两次封包密文不同。

use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use aws_lc_rs::pbkdf2::PBKDF2_HMAC_SHA512;
use aws_lc_rs::rand::{SecureRandom, SystemRandom};

use crate::db;
use crate::error::{AppError, Result};

/// 信封魔数（Ledger Sync Envelope v1）：密文自描述判定标记。
const MAGIC: &[u8; 4] = b"LSE1";

/// 信封格式版本（头部内自描述；非本版本拒绝解析，不做格式猜测）。
const VERSION: u8 = 1;

/// 密钥派生盐长度（字节）。
const SALT_LEN: usize = 16;

/// AES-GCM nonce 长度（字节，96 位标准）。
const NONCE_LEN: usize = 12;

/// 派生密钥长度（字节，AES-256）。
const KEY_LEN: usize = 32;

/// 头部分段偏移（组装与解析共用，杜绝魔法数字漂移）：
/// 魔数(4) | 版本(1) | 迭代次数 u32 LE(4) | 盐(16) | nonce(12)。
const VERSION_OFFSET: usize = MAGIC.len();
const ITERATIONS_OFFSET: usize = VERSION_OFFSET + 1;
const SALT_OFFSET: usize = ITERATIONS_OFFSET + 4;
const NONCE_OFFSET: usize = SALT_OFFSET + SALT_LEN;
/// 头部长度 = nonce 末尾。
const HEADER_LEN: usize = NONCE_OFFSET + NONCE_LEN;

/// GCM 认证标签长度（字节）。
const TAG_LEN: usize = 16;

/// 生产默认 KDF 迭代次数：与 SQLCipher 默认 KDF 同量级（256k）。「主口令 →
/// 密钥」的派生成本是安全量级的组成部分，与备份域加密保持同一防线；迭代
/// 次数随信封头自描述，开封按头执行，后续上调不破坏旧信封。
pub const DEFAULT_KDF_ITERATIONS: u32 = 256_000;

/// 封包参数（KDF 迭代次数）：生产走 [`EnvelopeParams::default`]；测试用低
/// 迭代验证格式语义（派生成本不是格式行为）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvelopeParams {
    pub kdf_iterations: u32,
}

impl Default for EnvelopeParams {
    fn default() -> Self {
        Self {
            kdf_iterations: DEFAULT_KDF_ITERATIONS,
        }
    }
}

/// 信封模式：同步文件的通道形态。加密与否复用备份域加密模式——本库为加密
/// 形态则封包上通道，明文库允许明文同步（界面显著提示归 #862 壳层）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeMode<'a> {
    /// 明文同步（未开加密模式）：原样字节上通道。
    Plaintext,
    /// 加密同步：主口令派生密钥整包封包。
    Encrypted { passphrase: &'a str },
}

impl EnvelopeMode<'_> {
    /// 明文模式判定（同步报告的明文提示依据）。
    pub fn is_plaintext(&self) -> bool {
        matches!(self, Self::Plaintext)
    }
}

/// 输入是否为密文信封（魔数前缀判定；空输入按明文对待）。
pub fn is_sealed(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// 封包：按模式产出通道字节。明文模式逐字节直通；加密模式产出
/// `魔数+版本+迭代次数+盐+nonce+密文(含标签)` 信封。
pub fn seal(payload: &[u8], mode: &EnvelopeMode<'_>, params: &EnvelopeParams) -> Result<Vec<u8>> {
    let EnvelopeMode::Encrypted { passphrase } = mode else {
        return Ok(payload.to_vec());
    };
    if passphrase.is_empty() {
        return Err(AppError::coded(
            "encryption.passphrase-empty",
            "主口令不能为空",
        ));
    }
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    let rng = SystemRandom::new();
    rng.fill(&mut salt)
        .map_err(|_| AppError::Invalid("信封盐随机源不可用".to_string()))?;
    rng.fill(&mut nonce_bytes)
        .map_err(|_| AppError::Invalid("信封 nonce 随机源不可用".to_string()))?;

    let key = derive_key(passphrase, &salt, params.kdf_iterations)?;
    let header = header_bytes(params.kdf_iterations, &salt, &nonce_bytes);
    let sealing_key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, &key)
            .map_err(|_| AppError::Invalid("信封密钥初始化失败".to_string()))?,
    );

    // AD 绑定整个头部：版本/迭代次数/盐/nonce 任一被改即认证失败。
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len() + TAG_LEN);
    out.extend_from_slice(&header);
    out.extend_from_slice(payload);
    let nonce = Nonce::try_assume_unique_for_key(&nonce_bytes)
        .map_err(|_| AppError::Invalid("信封 nonce 非法".to_string()))?;
    let tag = sealing_key
        .seal_in_place_separate_tag(nonce, Aad::from(&header[..]), &mut out[HEADER_LEN..])
        .map_err(|_| AppError::Invalid("信封加密失败".to_string()))?;
    out.extend_from_slice(tag.as_ref());
    Ok(out)
}

/// 开封（自描述）：密文信封凭口令解封（缺口令/错口令/篡改报码化错误）；
/// 无魔数输入按明文直通。口令由调用方来自用户输入，不落盘、不传输。
pub fn open(envelope: &[u8], passphrase: Option<&str>) -> Result<Vec<u8>> {
    if !is_sealed(envelope) {
        return Ok(envelope.to_vec());
    }
    if envelope.len() < HEADER_LEN + TAG_LEN {
        return Err(envelope_corrupt("信封长度不足"));
    }
    if envelope[VERSION_OFFSET] != VERSION {
        return Err(envelope_corrupt("信封版本未知"));
    }
    let iterations = u32::from_le_bytes(
        envelope[ITERATIONS_OFFSET..SALT_OFFSET]
            .try_into()
            .map_err(|_| envelope_corrupt("信封头损坏"))?,
    );
    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(&envelope[..HEADER_LEN]);
    let salt: &[u8] = &envelope[SALT_OFFSET..NONCE_OFFSET];
    let nonce_bytes: [u8; NONCE_LEN] = envelope[NONCE_OFFSET..HEADER_LEN]
        .try_into()
        .map_err(|_| envelope_corrupt("信封头损坏"))?;
    if iterations == 0 {
        return Err(envelope_corrupt("信封 KDF 参数非法"));
    }

    let passphrase = passphrase.ok_or_else(|| {
        AppError::coded(
            "sync-channel.passphrase-required",
            "同步通道上有加密数据，需要主口令才能读取",
        )
    })?;
    let key = derive_key(passphrase, salt, iterations)?;
    let opening_key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, &key)
            .map_err(|_| AppError::Invalid("信封密钥初始化失败".to_string()))?,
    );
    let mut sealed = envelope[HEADER_LEN..].to_vec();
    let nonce = Nonce::try_assume_unique_for_key(&nonce_bytes)
        .map_err(|_| AppError::Invalid("信封 nonce 非法".to_string()))?;
    let plaintext = opening_key
        .open_in_place(nonce, Aad::from(&header[..]), &mut sealed)
        .map_err(|_| db::encryption::passphrase_incorrect_error())?;
    Ok(plaintext.to_vec())
}

/// 密钥派生单点：主口令 → PBKDF2-HMAC-SHA512 → AES-256 密钥。
fn derive_key(passphrase: &str, salt: &[u8], iterations: u32) -> Result<[u8; KEY_LEN]> {
    // 迭代次数 0 非法（封包侧参数守卫，开封侧对头内值复验）。
    let iterations = std::num::NonZeroU32::new(iterations)
        .ok_or_else(|| envelope_corrupt("信封 KDF 参数非法"))?;
    let mut key = [0u8; KEY_LEN];
    aws_lc_rs::pbkdf2::derive(
        PBKDF2_HMAC_SHA512,
        iterations,
        salt,
        passphrase.as_bytes(),
        &mut key,
    );
    Ok(key)
}

/// 信封头组装：魔数 + 版本 + 迭代次数（u32 LE）+ 盐 + nonce。
fn header_bytes(iterations: u32, salt: &[u8; SALT_LEN], nonce: &[u8; NONCE_LEN]) -> Vec<u8> {
    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(MAGIC);
    debug_assert_eq!(header.len(), VERSION_OFFSET);
    header.push(VERSION);
    debug_assert_eq!(header.len(), ITERATIONS_OFFSET);
    header.extend_from_slice(&iterations.to_le_bytes());
    debug_assert_eq!(header.len(), SALT_OFFSET);
    header.extend_from_slice(salt);
    debug_assert_eq!(header.len(), NONCE_OFFSET);
    header.extend_from_slice(nonce);
    debug_assert_eq!(header.len(), HEADER_LEN);
    header
}

/// 信封结构损坏的码化错误单点（可重试性取决于通道侧重拉，不冒充口令错误）。
fn envelope_corrupt(detail: &str) -> AppError {
    AppError::codedp(
        "sync-channel.envelope-corrupt",
        format!("同步信封损坏，无法解析：{detail}"),
        &[detail],
    )
}
