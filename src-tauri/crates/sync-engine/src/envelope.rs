use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use aws_lc_rs::pbkdf2::PBKDF2_HMAC_SHA512;
use aws_lc_rs::rand::{SecureRandom, SystemRandom};

use ledger_infra::db;
use ledger_infra::error::{AppError, Result};

const MAGIC: &[u8; 4] = b"LSE1";

const VERSION: u8 = 1;

const SALT_LEN: usize = 16;

const NONCE_LEN: usize = 12;

const KEY_LEN: usize = 32;

const VERSION_OFFSET: usize = MAGIC.len();
const ITERATIONS_OFFSET: usize = VERSION_OFFSET + 1;
const SALT_OFFSET: usize = ITERATIONS_OFFSET + 4;
const NONCE_OFFSET: usize = SALT_OFFSET + SALT_LEN;
const HEADER_LEN: usize = NONCE_OFFSET + NONCE_LEN;

const TAG_LEN: usize = 16;

pub const DEFAULT_KDF_ITERATIONS: u32 = 256_000;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeMode<'a> {
    Plaintext,
    Encrypted { passphrase: &'a str },
}

impl EnvelopeMode<'_> {
    pub fn is_plaintext(&self) -> bool {
        matches!(self, Self::Plaintext)
    }
}

pub fn is_sealed(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

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

fn derive_key(passphrase: &str, salt: &[u8], iterations: u32) -> Result<[u8; KEY_LEN]> {
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

fn envelope_corrupt(detail: &str) -> AppError {
    AppError::codedp(
        "sync-channel.envelope-corrupt",
        format!("同步信封损坏，无法解析：{detail}"),
        &[detail],
    )
}
