//! 账本注册表引导内核（issue #832 / ADR-0089 决策 2，显式修订 ADR-0018 后果条款）。
//!
//! 库外引导指针文件（[`REGISTRY_FILE_NAME`]，文件名沿用 `data_location.json`
//! 不改——存量安装原地升级）由单字段数据位置指针泛化为**账本注册表**：账本清单
//! （id、展示名、目录）+ 活动账本指针。账本（Book）= 一个库目录（内含固定名
//! `ledger.db`，文件名不可配置）+ 注册表内的展示名与 id，**身份即目录**，同一
//! 目录不得重复登记。
//!
//! 双格式兼容：
//! - 旧格式 `{"data_dir": …}` 永远可读——读作唯一**默认账本**（兼活动账本），
//!   升级无感、零数据移动，**读取绝不回写文件**；
//! - 新格式 `{"version", "books", "active"}` 只在显式写入（[`write_registry`]）时
//!   落盘——「首次写入时落新格式」，读取路径从不升级落盘。
//!
//! 损坏即回退：注册表无法读取、无法解析或校验不通过（缺清单、缺活动指针、
//! 指针悬空、条目无效、目录重复登记、版本未知）一律 [`RegistryRead::Corrupt`]
//! 返回原因，由引导（`data_location::boot`）按既有回退原则回退默认目录并经
//! `Boot::fallback_reason` 暴露提示；任何路径绝不删除或修改既有库文件。
//!
//! 写入时机契约（供命令面消费）：[`crate::db::data_location::Boot::registry`]
//! 为 `Some` 时登记信息可用、可展示；**变更登记（新建/切换/改名/移除）还须
//! `Boot::deferred_relocation` 为 `None`**——旧格式指针的搬迁意图（含密文库
//! 推迟搬迁窗口）未执行完之前落新格式会把意图一并抹掉，活动账本目录随之落空。
//! `registry` 为 `None`（注册表损坏）时同样禁止写入：损坏文件可能是自定义位置
//! 的唯一记录，覆盖前须走既有「恢复默认位置」逃生舱。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::fs_util::atomic_write;

/// 注册表文件名：沿用旧数据位置指针文件名，存量安装原地升级、旧文件原地可读。
pub const REGISTRY_FILE_NAME: &str = "data_location.json";

/// 新格式注册表的格式版本。读取时携带其它版本的注册表按损坏回退（不猜测语义）。
pub const REGISTRY_FORMAT_VERSION: u32 = 1;

/// 旧格式折叠出的唯一默认账本展示名（随首次登记写入落盘；改名命令可改）。
pub const DEFAULT_BOOK_NAME: &str = "默认账本";

/// 账本注册表：账本清单 + 活动账本指针 + 来源形态（内存形态）。
///
/// 来源形态决定引导进入活动账本目录的语义（[`RegistryOrigin`]），随注册表
/// 一同解析、只存内存，不参与写入格式（落盘恒为新格式，见 [`write_registry`]）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookRegistry {
    /// 全部已登记账本。
    pub books: Vec<Book>,
    /// 活动账本 id（必须指向 [`BookRegistry::books`] 中的一项）。
    pub active_id: String,
    /// 来源形态：旧格式指针 / 新格式注册表。
    pub origin: RegistryOrigin,
}

impl BookRegistry {
    /// 活动账本（`active_id` 解析结果；校验通过的注册表恒为 `Some`）。
    pub fn active(&self) -> Option<&Book> {
        self.books.iter().find(|book| book.id == self.active_id)
    }

    /// 活动账本目录（建连解析的最终依据）。
    pub fn active_dir(&self) -> Option<&Path> {
        self.active().map(|book| book.dir.as_path())
    }

    /// 出厂/旧格式形态：唯一默认账本位于 `dir`，兼活动账本。id 现铸（仅内存，
    /// 随首次登记写入落盘——读取路径不回写文件）。
    pub fn single_default(dir: &Path) -> Self {
        let book = Book {
            id: super::new_uuid(),
            name: DEFAULT_BOOK_NAME.to_string(),
            dir: dir.to_path_buf(),
        };
        Self {
            active_id: book.id.clone(),
            books: vec![book],
            origin: RegistryOrigin::LegacyPointer,
        }
    }

    /// 把唯一默认账本的目录改写为 `dir`（仅内存、不落盘）：旧格式恒为单默认
    /// 账本，供密文库推迟搬迁窗口内把登记信息指向源目录的物理真值
    ///（`data_location::relocate_or_adopt` 消费）。
    pub fn redirect_default_book(&mut self, dir: &Path) {
        if let Some(book) = self.books.first_mut() {
            book.dir = dir.to_path_buf();
        }
    }
}

/// 注册表来源形态：决定引导进入活动账本目录的语义。
///
/// 旧格式指针保留启动期搬迁语义（默认目录 → 指针目录的三分支，ADR-0018）；
/// 新格式登记**绝不搬迁**——多本世界里默认目录中的库属于默认账本，向空的活动
/// 账本目录搬迁会造成跨本数据复制。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryOrigin {
    /// 旧格式单字段指针（含出厂未配置形态）：唯一默认账本，带搬迁语义。
    LegacyPointer,
    /// 新格式注册表。
    NewFormat,
}

/// 注册表文件读取结果。
#[derive(Clone, Debug, PartialEq)]
pub enum RegistryRead {
    /// 文件缺失 → 未配置（出厂行为，无回退信号）。
    Unconfigured,
    /// 文件存在但无法读取/解析/校验 → 视同未配置使用默认目录，但需回退原因。
    Corrupt(String),
    /// 解析成功（旧格式已折叠为单默认账本注册表）。
    Resolved(BookRegistry),
}

/// 一本账：身份即目录（内含固定名 `ledger.db`），id 与展示名是注册表元数据。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Book {
    /// 账本标识（注册表内的稳定句柄，供活动指针引用；UUID）。
    pub id: String,
    /// 展示名。
    pub name: String,
    /// 库目录（内含固定名 `ledger.db`）。
    pub dir: PathBuf,
}

/// 注册表文件的反序列化形状：新旧字段并存于一个结构，按内容判别形态；
/// 未知字段一律容忍（前向兼容——未来新增字段不破坏旧读取）。
#[derive(Deserialize)]
struct RegistryFile {
    /// 新格式版本；旧格式无此字段。
    version: Option<u32>,
    /// 旧格式唯一字段：库所在目录。
    data_dir: Option<String>,
    /// 新格式账本清单。
    books: Option<Vec<BookEntry>>,
    /// 新格式活动账本指针。
    active: Option<String>,
}

/// 注册表账本条目的反序列化形状（字段缺失在校验层报无效，不在解析层报错）。
#[derive(Deserialize)]
struct BookEntry {
    id: Option<String>,
    name: Option<String>,
    dir: Option<String>,
}

/// 注册表文件的序列化形状（新格式，[`write_registry`] 落盘的唯一形态）。
#[derive(Serialize)]
struct RegistryFileOut {
    version: u32,
    books: Vec<BookOut>,
    active: String,
}

/// 序列化账本条目。
#[derive(Serialize)]
struct BookOut {
    id: String,
    name: String,
    dir: String,
}

/// 读取账本注册表。缺失视同未配置；损坏是常态输入而非异常（一律不 panic、
/// 不报错上抛，原因随 [`RegistryRead::Corrupt`] 返回供引导回退时提示）。
pub fn read_registry(default_dir: &Path) -> RegistryRead {
    let path = default_dir.join(REGISTRY_FILE_NAME);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return RegistryRead::Unconfigured,
        Err(e) => {
            tracing::warn!(registry = %path.display(), error = %e, "无法读取数据位置指针文件");
            return RegistryRead::Corrupt(format!("无法读取数据位置指针文件：{e}"));
        }
    };
    let file: RegistryFile = match serde_json::from_str(&raw) {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!(registry = %path.display(), error = %e, "数据位置指针文件无法解析，视同未配置");
            return RegistryRead::Corrupt(format!(
                "数据位置指针文件无法解析（{}），已回退默认位置",
                path.display()
            ));
        }
    };
    resolve_file(&path, file)
}

/// 按字段内容判别注册表形态并解析：`books`/`active` 齐备走新格式校验；
/// 只出现其一按损坏；两者皆无回退旧格式 `data_dir`（沿用旧指针语义，损坏
/// 消息与升级前逐字一致）。
fn resolve_file(path: &Path, file: RegistryFile) -> RegistryRead {
    let invalid = |detail: String| {
        tracing::warn!(registry = %path.display(), detail = %detail, "账本注册表无效，视同未配置");
        RegistryRead::Corrupt(format!("账本注册表无效（{detail}），已回退默认位置"))
    };
    if let Some(version) = file.version
        && version != REGISTRY_FORMAT_VERSION
    {
        return invalid(format!("注册表版本 {version} 不受支持"));
    }
    match (file.books.as_deref(), file.active.as_deref()) {
        (Some(entries), Some(active)) => match build_registry(entries, active) {
            Ok(registry) => RegistryRead::Resolved(registry),
            Err(detail) => invalid(detail),
        },
        (Some(_), None) => invalid("缺少活动账本指针".into()),
        (None, Some(_)) => invalid("缺少账本清单".into()),
        (None, None) => match file.data_dir.as_deref().map(str::trim) {
            Some(dir) if !dir.is_empty() => {
                RegistryRead::Resolved(BookRegistry::single_default(Path::new(dir)))
            }
            _ => {
                tracing::warn!(registry = %path.display(), "数据位置指针文件无法解析，视同未配置");
                RegistryRead::Corrupt(format!(
                    "数据位置指针文件无法解析（{}），已回退默认位置",
                    path.display()
                ))
            }
        },
    }
}

/// 新格式条目装配：把反序列化形状折叠为内存注册表，再走与写入侧同一份
/// 校验（[`validate_registry`]）。任何违例整体按损坏回退（注册表是机器写出
/// 的，手工损坏走出厂逃生舱，不做局部抢救）。
fn build_registry(
    entries: &[BookEntry],
    active: &str,
) -> std::result::Result<BookRegistry, String> {
    let registry = BookRegistry {
        active_id: active.trim().to_string(),
        books: entries
            .iter()
            .map(|entry| Book {
                id: entry.id.as_deref().unwrap_or_default().trim().to_string(),
                name: entry.name.as_deref().unwrap_or_default().trim().to_string(),
                dir: PathBuf::from(entry.dir.as_deref().unwrap_or_default().trim()),
            })
            .collect(),
        origin: RegistryOrigin::NewFormat,
    };
    validate_registry(&registry)?;
    Ok(registry)
}

/// 注册表完整性校验（读取与写入共用的唯一不变量集）：清单非空、条目三字段
/// 齐备、id 与目录全库唯一（目录按路径身份判定）、活动指针必达。防止把引导
/// 必然回退的注册表落盘，也把手工损坏的注册表在读入时挡在门外。
fn validate_registry(registry: &BookRegistry) -> std::result::Result<(), String> {
    if registry.books.is_empty() {
        return Err("账本清单为空".into());
    }
    if registry.active_id.trim().is_empty() {
        return Err("活动账本指针为空".into());
    }
    let mut seen_ids: HashSet<&str> = HashSet::new();
    let mut seen_dirs: HashSet<&Path> = HashSet::new();
    for book in &registry.books {
        if book.id.trim().is_empty() || book.name.trim().is_empty() {
            return Err("存在缺少 id/展示名的账本条目".into());
        }
        if book.dir.as_os_str().is_empty() {
            return Err("存在缺少目录的账本条目".into());
        }
        if !seen_ids.insert(book.id.as_str()) {
            return Err(format!("账本标识重复（{}）", book.id));
        }
        if !seen_dirs.insert(book.dir.as_path()) {
            return Err("同一目录登记了多个账本".into());
        }
    }
    if registry.active().is_none() {
        return Err("活动账本指针指向未登记的账本".into());
    }
    Ok(())
}

/// 把账本注册表写入引导文件（新格式，原子写：先写唯一临时名再替换启用）。
/// 供命令面登记变更（新建/切换/改名/移除）与首次升级落盘使用。
pub fn write_registry(default_dir: &Path, registry: &BookRegistry) -> Result<()> {
    validate_registry(registry).map_err(|detail| {
        crate::error::AppError::codedp(
            "book-registry.invalid",
            format!("账本注册表校验失败：{detail}"),
            &[&detail],
        )
    })?;
    std::fs::create_dir_all(default_dir)?;
    let path = default_dir.join(REGISTRY_FILE_NAME);
    let content = serde_json::to_string_pretty(&RegistryFileOut {
        version: REGISTRY_FORMAT_VERSION,
        books: registry
            .books
            .iter()
            .map(|book| BookOut {
                id: book.id.clone(),
                name: book.name.clone(),
                dir: book.dir.to_string_lossy().into_owned(),
            })
            .collect(),
        active: registry.active_id.clone(),
    })?;
    atomic_write(&path, content.as_bytes())
}

#[cfg(test)]
mod tests;
