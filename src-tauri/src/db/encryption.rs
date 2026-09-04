//! 加密引擎基座：库文件头探测（issue #569 / ADR-0075 决策 4）。
//!
//! 加密状态是**库文件的属性**，随备份、恢复、复制自然流动；探测判定只读
//! 文件本身，不进任何库外引导状态（ADR-0017/0018 的「库外引导配置唯一
//! 例外」仍是 DataLocation，不再扩大）。明文库有固定文件头
//! （[`SQLITE_HEADER_MAGIC`]），SQLCipher 密文库头部为随机盐——读前
//! 16 字节即可可靠判定；空文件（不存在或不足 16 字节）按明文新装对待。
//!
//! 建连密钥缝在 [`super::open_connection_with_key`]（与明文路径同点的
//! 单一注入处）；本模块只负责「文件即真相」的判定，不打开库文件。

use std::path::Path;

use crate::error::Result;

/// SQLite 明文库固定文件头（16 字节魔数，SQLite 文件格式规范）。
pub const SQLITE_HEADER_MAGIC: [u8; 16] = *b"SQLite format 3\0";

/// 库文件头探测的三态结果（issue #569）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DbFileKind {
    /// 明文库：文件头为明文魔数。
    Plaintext,
    /// 密文库：文件头不是明文魔数（SQLCipher 以随机盐开头）。
    Encrypted,
    /// 空文件：文件不存在或可读字节不足一个文件头，按明文新装对待。
    Empty,
}

/// 探测库文件的明文/密文三态。文件即真相：只读该文件头 16 字节，
/// 不依赖任何库外引导状态（ADR-0075 决策 4）。
///
/// 文件不存在，或可读字节不足 16（含 0 字节）→ [`DbFileKind::Empty`]
/// （不足以构成任何一种库文件，按新装语义解释）；其余读取失败
/// （权限等）原样上抛。
pub fn probe_file_kind(path: &Path) -> Result<DbFileKind> {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(DbFileKind::Empty),
        Err(e) => return Err(e.into()),
    };
    let mut header = [0u8; 16];
    let mut filled = 0;
    while filled < header.len() {
        let n = file.read(&mut header[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    if filled < header.len() {
        return Ok(DbFileKind::Empty);
    }
    Ok(if header == SQLITE_HEADER_MAGIC {
        DbFileKind::Plaintext
    } else {
        DbFileKind::Encrypted
    })
}
