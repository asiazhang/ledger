use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::fs_util::atomic_write;

pub const BOOKS_DIR_NAME: &str = "books";

pub const REGISTRY_FILE_NAME: &str = "data_location.json";

pub const REGISTRY_FORMAT_VERSION: u32 = 1;

pub const DEFAULT_BOOK_NAME: &str = "默认账本";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookRegistry {
    pub books: Vec<Book>,
    pub active_id: String,
    pub pending_relocation: Option<PendingRelocation>,
    pub origin: RegistryOrigin,
}

impl BookRegistry {
    pub fn active(&self) -> Option<&Book> {
        self.books.iter().find(|book| book.id == self.active_id)
    }

    pub fn active_dir(&self) -> Option<&Path> {
        self.active().map(|book| book.dir.as_path())
    }

    pub fn single_default(dir: &Path) -> Self {
        let book = Book {
            id: stable_book_id(dir),
            name: DEFAULT_BOOK_NAME.to_string(),
            dir: dir.to_path_buf(),
        };
        Self {
            active_id: book.id.clone(),
            books: vec![book],
            pending_relocation: None,
            origin: RegistryOrigin::LegacyPointer,
        }
    }

    pub fn redirect_default_book(&mut self, dir: &Path) {
        if let Some(book) = self.books.first_mut() {
            book.dir = dir.to_path_buf();
        }
    }

    pub(crate) fn redirect_active_book(&mut self, dir: &Path) {
        if let Some(book) = self.active_mut() {
            book.dir = dir.to_path_buf();
        }
    }

    pub(crate) fn active_mut(&mut self) -> Option<&mut Book> {
        let active_id = self.active_id.clone();
        self.books.iter_mut().find(|book| book.id == active_id)
    }
}

pub(crate) fn stable_book_id(dir: &Path) -> String {
    let digest = Sha256::digest(dir.to_string_lossy().as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRelocation {
    pub book_id: String,
    pub from_dir: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryOrigin {
    LegacyPointer,
    NewFormat,
}

#[derive(Clone, Debug, Serialize)]
pub struct BookListInfo {
    pub books: Vec<Book>,
    pub active_id: Option<String>,
    pub mutable: bool,
    pub fallback_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RegistryRead {
    Unconfigured,
    Corrupt(String),
    Resolved(BookRegistry),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Book {
    pub id: String,
    pub name: String,
    pub dir: PathBuf,
}

#[derive(Deserialize)]
struct RegistryFile {
    version: Option<u32>,
    data_dir: Option<String>,
    books: Option<Vec<BookEntry>>,
    active: Option<String>,
    relocation: Option<PendingRelocationEntry>,
}

#[derive(Deserialize)]
struct PendingRelocationEntry {
    book_id: Option<String>,
    from_dir: Option<String>,
}

#[derive(Deserialize)]
struct BookEntry {
    id: Option<String>,
    name: Option<String>,
    dir: Option<String>,
}

#[derive(Serialize)]
struct RegistryFileOut {
    version: u32,
    books: Vec<BookOut>,
    active: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    relocation: Option<PendingRelocation>,
}

#[derive(Serialize)]
struct BookOut {
    id: String,
    name: String,
    dir: String,
}

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
        (Some(entries), Some(active)) => match build_registry(entries, active, file.relocation) {
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

fn build_registry(
    entries: &[BookEntry],
    active: &str,
    relocation: Option<PendingRelocationEntry>,
) -> std::result::Result<BookRegistry, String> {
    let pending = match relocation {
        None => None,
        Some(entry) => Some(PendingRelocation {
            book_id: entry
                .book_id
                .as_deref()
                .unwrap_or_default()
                .trim()
                .to_string(),
            from_dir: PathBuf::from(
                entry
                    .from_dir
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            ),
        }),
    };
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
        pending_relocation: pending,
        origin: RegistryOrigin::NewFormat,
    };
    validate_registry(&registry)?;
    Ok(registry)
}

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
    if let Some(pending) = &registry.pending_relocation {
        if pending.book_id.trim().is_empty() {
            return Err("搬迁意图缺少账本标识".into());
        }
        if pending.from_dir.as_os_str().is_empty() {
            return Err("搬迁意图缺少来源目录".into());
        }
        if registry.books.iter().all(|book| book.id != pending.book_id) {
            return Err("搬迁意图指向未登记的账本".into());
        }
    }
    Ok(())
}

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
        relocation: registry.pending_relocation.clone(),
    })?;
    atomic_write(&path, content.as_bytes())
}

pub(crate) fn registry_corrupt_error(reason: &str) -> crate::error::AppError {
    crate::error::AppError::codedp(
        "book.registry-corrupt",
        format!("账本注册表损坏，已回退默认账本；请先在数据设置中恢复默认位置（{reason}）"),
        &[reason],
    )
}

pub(crate) fn registry_busy_error() -> crate::error::AppError {
    crate::error::AppError::coded(
        "book.registry-busy",
        "数据搬迁尚未完成，暂无法变更账本登记；请重启应用完成搬迁后再试",
    )
}

pub(crate) fn settle_pending_relocation(registry: &mut BookRegistry) -> Result<()> {
    let Some(pending) = registry.pending_relocation.clone() else {
        return Ok(());
    };
    let Some(book) = registry.books.iter().find(|b| b.id == pending.book_id) else {
        return Err(registry_busy_error());
    };
    let target_has_db = book.dir.join(super::data_location::DB_FILE_NAME).exists();
    let source_has_db = pending
        .from_dir
        .join(super::data_location::DB_FILE_NAME)
        .exists();
    if !target_has_db && source_has_db {
        tracing::info!(
            book = %pending.book_id,
            from = %pending.from_dir.display(),
            to = %book.dir.display(),
            "搬迁意图尚未生效，拒绝变更账本登记"
        );
        return Err(registry_busy_error());
    }
    registry.pending_relocation = None;
    Ok(())
}

pub fn create_book_entry(default_dir: &Path, name: &str) -> Result<Book> {
    let name = required_name(name)?;
    let mut registry = read_for_mutation(default_dir)?;
    let id = crate::ids::new_uuid();
    let dir = default_dir.join(BOOKS_DIR_NAME).join(&id);
    ensure_dir_available(&dir)?;
    if let Err(e) = ensure_dir_not_registered(&registry, &dir) {
        let _ = std::fs::remove_dir(&dir);
        return Err(e);
    }
    let book = Book {
        id,
        name: name.to_string(),
        dir,
    };
    registry.books.push(book.clone());
    write_registry(default_dir, &registry)?;
    Ok(book)
}

pub fn switch_active_book(default_dir: &Path, id: &str) -> Result<Book> {
    let mut registry = read_for_mutation(default_dir)?;
    let book = registered_book(&registry, id)?.clone();
    if registry.active_id == id {
        return Err(crate::error::AppError::coded(
            "book.already-active",
            "该账本已是当前账本",
        ));
    }
    ensure_dir_available(&book.dir)?;
    registry.active_id = id.to_string();
    write_registry(default_dir, &registry)?;
    Ok(book)
}

pub fn rename_book_entry(default_dir: &Path, id: &str, name: &str) -> Result<Book> {
    let name = required_name(name)?;
    let mut registry = read_for_mutation(default_dir)?;
    registered_book_mut(&mut registry, id)?.name = name.to_string();
    write_registry(default_dir, &registry)?;
    Ok(registered_book(&registry, id)?.clone())
}

pub fn remove_book_entry(default_dir: &Path, id: &str) -> Result<()> {
    let mut registry = read_for_mutation(default_dir)?;
    registered_book(&registry, id)?;
    if registry.active_id == id {
        return Err(crate::error::AppError::coded(
            "book.remove-active",
            "不能移除当前账本，请先切换到其他账本",
        ));
    }
    registry.books.retain(|book| book.id != id);
    write_registry(default_dir, &registry)
}

pub(super) fn ensure_dir_not_registered(registry: &BookRegistry, dir: &Path) -> Result<()> {
    let canonical = dir.canonicalize()?;
    for book in &registry.books {
        let existing = book.dir.canonicalize().unwrap_or_else(|_| book.dir.clone());
        if existing == canonical {
            return Err(crate::error::AppError::codedp(
                "book.dir-exists",
                format!("该目录已登记为账本「{}」", book.name),
                &[&book.name],
            ));
        }
    }
    Ok(())
}

fn ensure_dir_available(target: &Path) -> Result<()> {
    std::fs::create_dir_all(target).map_err(|e| {
        crate::error::AppError::codedp(
            "book.dir-unavailable",
            format!("账本目录不可用（{}）：{e}", target.display()),
            &[&target.to_string_lossy(), &e.to_string()],
        )
    })
}

fn read_current(default_dir: &Path) -> Result<BookRegistry> {
    match read_registry(default_dir) {
        RegistryRead::Resolved(registry) => Ok(registry),
        RegistryRead::Unconfigured => Ok(BookRegistry::single_default(default_dir)),
        RegistryRead::Corrupt(reason) => Err(registry_corrupt_error(&reason)),
    }
}

fn read_for_mutation(default_dir: &Path) -> Result<BookRegistry> {
    let mut registry = read_current(default_dir)?;
    settle_pending_relocation(&mut registry)?;
    Ok(registry)
}

fn required_name(name: &str) -> Result<&str> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(crate::error::AppError::coded(
            "book.name-required",
            "账本名称不能为空",
        ));
    }
    Ok(trimmed)
}

fn registered_book<'a>(registry: &'a BookRegistry, id: &str) -> Result<&'a Book> {
    registry
        .books
        .iter()
        .find(|book| book.id == id)
        .ok_or_else(|| unregistered_book_error(id))
}

fn registered_book_mut<'a>(registry: &'a mut BookRegistry, id: &str) -> Result<&'a mut Book> {
    registry
        .books
        .iter_mut()
        .find(|book| book.id == id)
        .ok_or_else(|| unregistered_book_error(id))
}

fn unregistered_book_error(id: &str) -> crate::error::AppError {
    crate::error::AppError::codedp_not_found("book.not-found", format!("账本不存在: {id}"), &[id])
}
