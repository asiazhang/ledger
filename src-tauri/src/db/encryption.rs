//! 加密引擎基座：库文件头探测（issue #569 / ADR-0075 决策 4）、整库转换
//! 三形态与解锁（issue #570/#571 / ADR-0075 决策 5/6）、忘记口令重置
//! （issue #573 / 决策 2/5）、进程级锁定门。
//!
//! 加密状态是**库文件的属性**，随备份、恢复、复制自然流动；探测判定只读
//! 文件本身，不进任何库外引导状态（ADR-0017/0018 的「库外引导配置唯一
//! 例外」仍是 DataLocation，不再扩大）。明文库有固定文件头
//! （[`SQLITE_HEADER_MAGIC`]），SQLCipher 密文库头部为随机盐——读前
//! 16 字节即可可靠判定；空文件（不存在或不足 16 字节）按明文新装对待。
//!
//! 建连密钥缝在 [`super::open_connection_with_passphrase`]（与明文路径
//! 同点的单一注入处）；转换与解锁是文件级操作，行为语义见各函数文档。
//! IPC 壳（`commands/encryption.rs`）只做参数解包与状态编排。

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::Connection;

use crate::error::{AppError, Result};
use crate::fs_util::{cleanup, temp_sibling};

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

/// 本引擎密文库的页大小（SQLCipher 4 默认值，ADR-0075 决策 1：加密参数
/// 不调参，全库所有密文库同形）。
const SQLCIPHER_DEFAULT_PAGE_SIZE: u64 = 4096;

/// 文件是否具有本引擎密文库的落盘形态：头部非明文魔数（探测为密文），
/// 且尺寸为页大小的整数倍且不少于一个完整页。
///
/// 探测的「密文」三态把任意非明文魔数文件（含损坏的明文残留）都计为
/// `Encrypted`；启动搬迁需要区分「真密文库（等待主口令）」与「损坏垃圾」
/// （issue #570：后者必须保持既有回退行为，前者必须推迟搬迁而非回退）。
/// 两者在密码学上不可区分，用页对齐形态作实用判别：本引擎写出的密文库
/// 恒为整页对齐；随机垃圾几乎不可能恰好对齐（BDD 损坏样本为任意字节串）。
pub fn has_encrypted_file_layout(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let size = meta.len();
            size >= SQLCIPHER_DEFAULT_PAGE_SIZE && size % SQLCIPHER_DEFAULT_PAGE_SIZE == 0
        }
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// 进程级锁定门（issue #570 / ADR-0075 决策 5）
// ---------------------------------------------------------------------------

/// 进程级加密锁定门：密文库启动后、解锁成功前的进程状态标志。
///
/// 消费方有三处，共同保证「解锁先于一切业务读写」：
/// - IPC 壳门禁（`lib.rs` invoke wrapper）：锁定期间放行锁定命令白名单
///   （状态查询、解锁），其余命令一律拒绝；
/// - HTTP 壳门禁（`api_server` 中间件）：锁定期间数据端点返回码化错误；
/// - 启动编排（`lib.rs`）：锁定期间不启动自动备份调度（定时追补同轮承载）。
///
/// `Clone` 形态：同一实例在 Builder 装配期即被 invoke wrapper 捕获，setup 期经
/// `app.manage` 注册为应用状态，HTTP 壳状态（`ApiState`）持同一实例。
#[derive(Clone)]
pub struct EncryptionGate {
    locked: Arc<AtomicBool>,
}

impl EncryptionGate {
    /// 新建锁定门（默认不锁定；密文库启动路径再显式置为锁定）。
    pub fn new(locked: bool) -> Self {
        Self {
            locked: Arc::new(AtomicBool::new(locked)),
        }
    }

    /// 当前是否处于锁定（等待解锁）状态。
    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::SeqCst)
    }

    /// 切换锁定状态（解锁成功置 `false`）。
    pub fn set_locked(&self, locked: bool) {
        self.locked.store(locked, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// 整库加密转换（issue #570/#571 / ADR-0075 决策 6：开启 / 关闭 / 修改主口令三形态同机制）
// ---------------------------------------------------------------------------

/// 把明文库整库一次性转换为密文库（用户显式开启加密，issue #570）。
///
/// 流程（ADR-0075 决策 6）：ATTACH 带口令新库 + SQLCipher 标准导出
/// （`sqlcipher_export`）→ 新库用新口令试开验证（打开 + 完整性检查 +
/// `user_version` 一致）→ 原子替换启用，旧文件按既有重置命名语义
/// （`ledger.db.bak`，见 `db::reset_db_in`）保留明文副本。
/// 中途任何失败：清理临时产物、原库原样保留，应用回到明文可用状态——
/// **不存在半加密状态**。
///
/// 转换在本函数自有的裸连接上执行（不经 `db::open_connection`，不装
/// 耗时 hook）：ATTACH/导出语句文本含主口令，绝不能进入 trace 输出
/// （ADR-0075 后果条款：日志与 trace 不落主口令）。调用方（解锁后的
/// 运行中应用）可能持有主连接，故设置 busy_timeout 容让并发访问。
pub fn enable_encryption_for_file(db_path: &Path, passphrase: &str) -> Result<()> {
    if passphrase.is_empty() {
        return Err(AppError::coded(
            "encryption.passphrase-empty",
            "主口令不能为空",
        ));
    }
    // 文件即真相：只有明文库（含空文件的新装语义之外的实际库文件）可开启加密。
    match probe_file_kind(db_path)? {
        DbFileKind::Plaintext => {}
        DbFileKind::Empty => {
            return Err(AppError::coded(
                "encryption.db-missing",
                "数据库文件不存在或为空",
            ));
        }
        DbFileKind::Encrypted => {
            return Err(AppError::coded(
                "encryption.not-plaintext",
                "当前库已是加密库，无需再次开启加密",
            ));
        }
    }
    convert_db_file(db_path, None, Some(passphrase))
}

/// 把密文库整库一次性转换回明文库（用户显式关闭加密，issue #571）。
///
/// 与开启加密同一套机制（ADR-0075 决策 6）：凭当前主口令读取密文源库，
/// ATTACH 空钥匙新库（SQLCipher 语义：空钥匙 = 明文）导出 → 试开验证 →
/// 原子替换启用，旧密文库保留 `.bak` 副本。完成后重启，启动探测发现
/// 明文库、不再出现解锁屏。需当前主口令：文件级机制凭口令读取密文，
/// 先验证后转换——口令错误报 `encryption.passphrase-incorrect`，原库
/// 不动；中途任何失败原库原样保留，不存在半加密状态。
pub fn disable_encryption_for_file(db_path: &Path, current_passphrase: &str) -> Result<()> {
    require_encrypted_file(db_path)?;
    verify_source_passphrase(db_path, current_passphrase)?;
    convert_db_file(db_path, Some(current_passphrase), None)
}

/// 修改主口令：旧口令验证通过后，把密文库整库转入新口令的新库（issue #571）。
///
/// 与开启 / 关闭同一套机制（ADR-0075 决策 6：改口令 = 转入新口令的新库）：
/// 凭旧口令读取密文源库，ATTACH 带新口令新库导出 → 新库用新口令试开验证
/// → 原子替换启用，旧库保留 `.bak` 副本（仍凭旧口令可开）。旧口令错误
/// 报 `encryption.passphrase-incorrect`，原库不动；中途任何失败原库原样
/// 保留，不存在半加密状态。
pub fn change_passphrase_for_file(
    db_path: &Path,
    current_passphrase: &str,
    new_passphrase: &str,
) -> Result<()> {
    if new_passphrase.is_empty() {
        return Err(AppError::coded(
            "encryption.passphrase-empty",
            "主口令不能为空",
        ));
    }
    require_encrypted_file(db_path)?;
    verify_source_passphrase(db_path, current_passphrase)?;
    convert_db_file(db_path, Some(current_passphrase), Some(new_passphrase))
}

/// 转换核心：三形态共用的文件级机制——以 `source_passphrase` 打开源库
/// （`None` = 明文源库，开启加密形态），ATTACH 以 `target_passphrase`
/// （`None` = 空钥匙明文，关闭加密形态）命名的新库导出 → 新库试开验证
/// → 原子替换启用，旧文件保留 `.bak` 副本。调用方负责形态门禁（探测
/// 三态）与旧口令验证；中途失败清理临时产物、原库原样保留。
fn convert_db_file(
    db_path: &Path,
    source_passphrase: Option<&str>,
    target_passphrase: Option<&str>,
) -> Result<()> {
    let tmp_path = temp_sibling(db_path, "convert");
    let result = export_converted_copy(db_path, &tmp_path, source_passphrase, target_passphrase)
        .and_then(|user_version| verify_converted_copy(&tmp_path, target_passphrase, user_version))
        .and_then(|()| promote_converted_copy(db_path, &tmp_path));

    if let Err(error) = result {
        // 失败收尾：临时产物用后即清（成功时已被 rename 走，cleanup 容忍不存在）。
        cleanup(&tmp_path);
        tracing::warn!(error = %error, "整库转换失败，原库保持原样不变");
        return Err(error);
    }
    tracing::info!(bak = %bak_path(db_path).display(), "整库转换完成，原库保留为 .bak 副本");
    Ok(())
}

/// 形态门禁：只有密文库可关闭加密 / 修改主口令（issue #571，文件即真相）。
fn require_encrypted_file(db_path: &Path) -> Result<()> {
    match probe_file_kind(db_path)? {
        DbFileKind::Encrypted => Ok(()),
        DbFileKind::Empty => Err(AppError::coded(
            "encryption.db-missing",
            "数据库文件不存在或为空",
        )),
        DbFileKind::Plaintext => Err(AppError::coded(
            "encryption.not-encrypted",
            "库文件当前不是加密状态，请重启应用后再试",
        )),
    }
}

/// 验证当前主口令确实能读开密文源库（先验证后转换）：类型化读语句先行
/// 校验（错误形态可精确匹配 not-a-database），口令错误报码化错误，原库
/// 不动。转换本体在自有裸连接重开源库，验证连接即弃。
fn verify_source_passphrase(db_path: &Path, passphrase: &str) -> Result<()> {
    let conn = super::open_connection_with_passphrase(db_path, passphrase)?;
    match conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    }) {
        Ok(_) => Ok(()),
        // 合并口径（ADR-0075 决策 5 修订 / issue #603）：不误报损坏。
        Err(e) if is_not_a_database(&e) => Err(passphrase_incorrect_error()),
        Err(e) => Err(e.into()),
    }
}

/// 旧库的保留副本路径（既有重置命名语义：`ledger.db.bak` 固定名）。
/// 副本保留的是**转换前形态**：开启加密时为明文副本，关闭加密 / 修改
/// 主口令时为密文副本（后者仍凭旧口令可开）。
fn bak_path(db_path: &Path) -> std::path::PathBuf {
    db_path.with_extension("db.bak")
}

/// 在自有裸连接上导出转换副本：以源口令（如有）打开源库 → ATTACH 以
/// 目标钥匙（`None` = 空钥匙明文）命名的新库 → `sqlcipher_export` →
/// 显式对齐 `user_version`（不依赖导出函数对它的复制行为）→ DETACH。
/// 返回源库的 `user_version` 供验证比对。
fn export_converted_copy(
    source: &Path,
    target: &Path,
    source_passphrase: Option<&str>,
    target_passphrase: Option<&str>,
) -> Result<i64> {
    // 裸连接：不装耗时 hook——ATTACH 语句文本含目标主口令、PRAGMA key
    // 语句文本含源主口令，均不得进 trace。密钥注入保持首条语句纪律
    // （与 `open_connection_with_passphrase` 同序：key 先于一切库访问）。
    let conn = Connection::open(source)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    if let Some(passphrase) = source_passphrase {
        conn.pragma_update(None, "key", passphrase)?;
    }
    let user_version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    // SQLCipher 空钥匙 = 明文库（关闭加密形态的目标形态）。
    let target_key = match target_passphrase {
        Some(pass) => sql_string_literal(pass),
        None => String::from("''"),
    };
    let attach_sql = format!(
        "ATTACH DATABASE {} AS encryption_target KEY {target_key}",
        sql_string_literal(&target.to_string_lossy()),
    );
    conn.execute_batch(&attach_sql)?;
    let export_result = (|| -> Result<()> {
        conn.execute_batch("SELECT sqlcipher_export('encryption_target')")?;
        conn.execute_batch(&format!(
            "PRAGMA encryption_target.user_version = {user_version}"
        ))?;
        Ok(())
    })();
    let result = export_result.and_then(|()| {
        conn.execute_batch("DETACH DATABASE encryption_target")
            .map_err(Into::into)
    });
    result?;
    Ok(user_version)
}

/// 试开验证转换副本：凭目标钥匙（`None` = 明文打开）打开 + 完整性检查 +
/// `user_version` 一致。验证通过即证明「新库可凭目标形态在重启后重新打开」。
fn verify_converted_copy(target: &Path, passphrase: Option<&str>, user_version: i64) -> Result<()> {
    let conn = match passphrase {
        Some(pass) => super::open_connection_with_passphrase(target, pass)?,
        None => super::open_connection(target)?,
    };
    super::check_integrity(&conn)
        .map_err(|e| AppError::Io(format!("转换副本完整性检查失败: {e}")))?;
    let copied: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if copied != user_version {
        return Err(AppError::Io(format!(
            "转换副本 schema 版本不一致（{copied} ≠ {user_version}），已拒绝启用"
        )));
    }
    Ok(())
}

/// 原子替换启用：旧库改名 `.bak` 保留副本（保留转换前形态），转换副本
/// 顶替原位。第二次改名失败时尽力把副本改回原位，保证原库始终可用。
fn promote_converted_copy(db_path: &Path, tmp_path: &Path) -> Result<()> {
    let bak = bak_path(db_path);
    std::fs::rename(db_path, &bak)?;
    if let Err(e) = std::fs::rename(tmp_path, db_path) {
        // 回滚：把旧库改回原位（尽力而为，失败则保留 .bak 现场并报错）。
        std::fs::rename(&bak, db_path).ok();
        return Err(AppError::Io(format!("转换副本替换启用失败: {e}")));
    }
    Ok(())
}

/// 忘记口令逃生门（issue #573 / ADR-0075 决策 2/5）：把密文库重置为全新
/// 明文空库，旧密文库按既有重置命名语义（`ledger.db.bak`，见
/// `db::reset_db_in`）保留为**密文副本**——无密钥不可读，日后想起口令
/// 数据仍可救回。
///
/// 流程：探测确认文件确为密文库 → 旧库改名 `.bak` 保留副本 → 原位新建
/// 明文库（建连 + 迁移 + 完整性检查）。新库建失败时尽力把副本改回原位，
/// 保持锁定现场可重试；旧库本身永不删除。
///
/// 只做文件级重置，不触碰进程状态（锁定门、DbState 换连由 IPC 壳层
/// 编排）。无后门：不存在任何「验证身份后解密旧库」的路径。
pub fn reset_encrypted_db_file(db_path: &Path) -> Result<Connection> {
    match probe_file_kind(db_path)? {
        DbFileKind::Encrypted => {}
        DbFileKind::Empty => {
            return Err(AppError::coded(
                "encryption.db-missing",
                "数据库文件不存在或为空",
            ));
        }
        DbFileKind::Plaintext => {
            return Err(AppError::coded(
                "encryption.not-encrypted",
                "库文件当前不是加密状态，请重启应用后再试",
            ));
        }
    }
    let bak = bak_path(db_path);
    std::fs::rename(db_path, &bak)?;
    match open_new_plaintext_db(db_path) {
        Ok(conn) => {
            tracing::info!(bak = %bak.display(), "忘记口令重置完成：原密文库保留为 .bak 副本，新明文空库就绪");
            Ok(conn)
        }
        Err(e) => {
            // 回滚（尽力而为）：新库建失败时把密文库改回原位，锁定现场可重试。
            std::fs::rename(&bak, db_path).ok();
            tracing::error!(error = %e, "忘记口令重置失败，原密文库已改回原位");
            Err(e)
        }
    }
}

/// 新建明文空库：明文路径建连 + 迁移 + 完整性检查（重置产物验收基准）。
fn open_new_plaintext_db(db_path: &Path) -> Result<Connection> {
    let mut conn = super::open_connection(db_path)?;
    super::init_db(&mut conn)?;
    super::check_integrity(&conn)?;
    Ok(conn)
}

/// SQL 字符串字面量转义（单引号加倍）。仅用于 ATTACH 的路径与主口令注入
/// （PRAGMA 系语句不支持绑定参数）；转换连接不装耗时 hook，字面量不外泄。
fn sql_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// SQLCipher 下错误口令与损坏同为 not-a-database、运行期不可靠区分
/// （ADR-0075 决策 5 修订 / issue #603）：统一以「口令错误或文件损坏」
/// 合并口径的码化错误上报——不误报损坏、可无限重试。单一构造点避免
/// 口径文案多出漂移（zh 模板与之逐字一致，ADR-0050）。
pub(crate) fn passphrase_incorrect_error() -> AppError {
    AppError::coded(
        "encryption.passphrase-incorrect",
        "口令错误或文件损坏，请重试",
    )
}

// ---------------------------------------------------------------------------
// 解锁（issue #570 / ADR-0075 决策 5）
// ---------------------------------------------------------------------------

/// 解锁密文库：凭主口令打开库文件并完成迁移，返回可用连接。
///
/// 解锁失败提示采「口令错误或文件损坏」合并口径（ADR-0075 决策 5 修订 /
/// issue #603）：SQLCipher 下错误口令与损坏同为 not-a-database、运行期
/// 不可靠区分，打开阶段报码化错误 `encryption.passphrase-incorrect`（
/// 可无限重试，不误报损坏）；凭口令打开成功但完整性检查失败 →
/// `encryption.db-corrupt`。调用方可对同一文件无限次重试本函数，失败不
/// 改动文件任何字节。
pub fn unlock_db_file(db_path: &Path, passphrase: &str) -> Result<Connection> {
    match probe_file_kind(db_path)? {
        DbFileKind::Encrypted => {}
        DbFileKind::Empty => {
            return Err(AppError::coded(
                "encryption.db-missing",
                "数据库文件不存在或为空",
            ));
        }
        DbFileKind::Plaintext => {
            return Err(AppError::coded(
                "encryption.not-encrypted",
                "库文件当前不是加密状态，请重启应用后再试",
            ));
        }
    }
    // `PRAGMA key` 本身不校验口令；校验发生在首条读语句。用类型化读语句
    // 先行校验（错误形态可精确匹配 not-a-database），再执行迁移。
    let conn = super::open_connection_with_passphrase(db_path, passphrase)?;
    if let Err(e) = conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    }) {
        if is_not_a_database(&e) {
            return Err(passphrase_incorrect_error());
        }
        return Err(e.into());
    }
    let mut conn = conn;
    super::init_db(&mut conn)?;
    if let Err(e) = super::check_integrity(&conn) {
        tracing::error!(error = %e, "密文库完整性检查未通过");
        return Err(AppError::coded(
            "encryption.db-corrupt",
            "数据库文件损坏，无法通过完整性检查",
        ));
    }
    Ok(conn)
}

/// 错误形态判别：SQLCipher 对错误口令与损坏文件均报 not-a-database；
/// 本谓词供备份域等基础设施消费方归一错误形态（pub(crate)：勿在壳层使用）。
pub(crate) fn is_not_a_database(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(err, _) if err.code == rusqlite::ffi::ErrorCode::NotADatabase
    )
}
