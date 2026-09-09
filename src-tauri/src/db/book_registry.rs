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

//! 活动账本搬迁意图（issue #836 / ADR-0089 决策 5「搬迁收窄为当前活动账本搬
//! 目录」）：注册表新格式携带可选 `relocation` 意图（目标账本 + 来源目录），
//! 由「更改数据位置」命令落盘、引导在下次启动时消费（三分支：目标已有库直接
//! 接管 / 来源有库则整库搬迁 / 皆无则直接启用，与旧格式指针同语义）。意图
//! 未消费前（目标尚无库而来源仍有库）登记变更被拒（[`settle_pending_relocation`]）
//! ——此刻改清单会让意图落空、账本目录悬空；意图已完成（目标已有库或双方皆
//! 无库）时由下一次登记变更就地消费，从文件中自愈清除。任何路径绝不删除或
//! 修改既有库文件；搬迁完成后旧位置库永久保留。
//!
//! 默认账本的稳定标识（issue #836）：备份产物命名与钥匙串条目按账本标识分域
//! （ADR-0089 决策 5），标识必须跨启动稳定；折叠默认账本的 id 若每次读取现铸
//! 随机值，其历史备份与缓存会随启动漂移。故折叠默认账本的 id 由其目录派生
//!（[`stable_book_id`]，确定性、跨进程稳定），首次登记写入落盘后与随机登记
//! 的账本标识无差别；「身份即目录」语义下目录即身份，派生标识随目录而变正是
//! 语义本身。新登记账本仍用随机 UUID。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::fs_util::atomic_write;

/// 登记账本子目录名（[`default_dir`] 下自动创建的账本目录父目录）。
pub const BOOKS_DIR_NAME: &str = "books";

/// 注册表文件名：沿用旧数据位置指针文件名，存量安装原地升级、旧文件原地可读。
pub const REGISTRY_FILE_NAME: &str = "data_location.json";

/// 新格式注册表的格式版本。读取时携带其它版本的注册表按损坏回退（不猜测语义）。
pub const REGISTRY_FORMAT_VERSION: u32 = 1;

/// 旧格式折叠出的唯一默认账本展示名（随首次登记写入落盘；改名命令可改）。
pub const DEFAULT_BOOK_NAME: &str = "默认账本";

/// 账本注册表：账本清单 + 活动账本指针 + 搬迁意图 + 来源形态（内存形态）。
///
/// 来源形态决定引导进入活动账本目录的语义（[`RegistryOrigin`]），随注册表
/// 一同解析；搬迁意图（[`PendingRelocation`]）随读写往返保留，由引导消费。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookRegistry {
    /// 全部已登记账本。
    pub books: Vec<Book>,
    /// 活动账本 id（必须指向 [`BookRegistry::books`] 中的一项）。
    pub active_id: String,
    /// 活动账本搬迁意图（issue #836）：`Some` = 已提交、待引导在下次启动消费。
    pub pending_relocation: Option<PendingRelocation>,
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

    /// 出厂/旧格式形态：唯一默认账本位于 `dir`，兼活动账本。id 由目录派生
    ///（[`stable_book_id`]）——跨启动稳定，备份命名与钥匙串条目得以按本分域。
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

    /// 把唯一默认账本的目录改写为 `dir`（仅内存、不落盘）：旧格式恒为单默认
    /// 账本，供密文库推迟搬迁窗口内把登记信息指向源目录的物理真值
    ///（`data_location::relocate_or_adopt` 消费）。
    pub fn redirect_default_book(&mut self, dir: &Path) {
        if let Some(book) = self.books.first_mut() {
            book.dir = dir.to_path_buf();
        }
    }

    /// 把活动账本的目录改写为 `dir`（仅内存、不落盘）：新格式密文库推迟搬迁
    /// 窗口内把登记信息指向源目录的物理真值（`data_location::enter_registry`
    /// 消费），与 [`Self::redirect_default_book`] 同型。
    pub(crate) fn redirect_active_book(&mut self, dir: &Path) {
        if let Some(book) = self.active_mut() {
            book.dir = dir.to_path_buf();
        }
    }

    /// 活动账本（可变）；校验通过的注册表恒为 `Some`。
    pub(crate) fn active_mut(&mut self) -> Option<&mut Book> {
        let active_id = self.active_id.clone();
        self.books.iter_mut().find(|book| book.id == active_id)
    }
}

/// 折叠默认账本的稳定标识：目录路径的 SHA-256 截取 16 位十六进制。确定性
///（同目录同标识、跨进程跨版本稳定）是唯一要求，非机密用途；新登记账本不经
/// 此函数（随机 UUID，见 [`create_book_entry`]）。
pub(crate) fn stable_book_id(dir: &Path) -> String {
    let digest = Sha256::digest(dir.to_string_lossy().as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// 活动账本搬迁意图（issue #836 / ADR-0089 决策 5）：由「更改数据位置」命令
/// 写入（活动账本目录改指目标 + 意图携带来源目录），引导在下次启动消费——
/// 目标已有库直接接管；目标为空而来源有库则整库搬迁；皆无则直接启用。消费
/// 完成后意图随下一次登记变更从文件中清除（自愈，引导自身不回写文件）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRelocation {
    /// 待搬迁的账本（提交时的活动账本；引导只消费匹配当前活动账本的意图）。
    pub book_id: String,
    /// 搬迁来源目录（库文件当前位置；目标 = 该账本登记目录）。
    pub from_dir: PathBuf,
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

/// 账本清单信息（列表命令的聚合形态，issue #833）：登记清单 + 活动指针 +
/// 登记变更可用性 + 回退警示。聚合由引导层完成
/// （`data_location::gather_book_list`），本类型与 [`Book`] 同家定义。
#[derive(Clone, Debug, Serialize)]
pub struct BookListInfo {
    /// 全部已登记账本（注册表损坏时为空——清单不可信，不展示残片）。
    pub books: Vec<Book>,
    /// 活动账本 id；注册表损坏时 `None`（引导已回退默认目录建连）。
    pub active_id: Option<String>,
    /// 登记变更（新建/切换/改名/移除）当前是否可用（写入时机契约：注册表
    /// 可读且无推迟搬迁窗口）。`false` 时前端禁用变更入口并展示回退原因。
    pub mutable: bool,
    /// 引导期回退原因（供界面显著提示）；`None` = 未回退。
    pub fallback_reason: Option<String>,
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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
    /// 新格式活动账本搬迁意图（issue #836；可缺省——无意图时字段缺席）。
    relocation: Option<PendingRelocationEntry>,
}

/// 搬迁意图条目的反序列化形状（字段缺失在校验层报无效，不在解析层报错）。
#[derive(Deserialize)]
struct PendingRelocationEntry {
    book_id: Option<String>,
    from_dir: Option<String>,
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
    /// 搬迁意图：无意图时不落字段（保持登记态文件的常态形状最小）。
    #[serde(skip_serializing_if = "Option::is_none")]
    relocation: Option<PendingRelocation>,
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

/// 新格式条目装配：把反序列化形状折叠为内存注册表，再走与写入侧同一份
/// 校验（[`validate_registry`]）。任何违例整体按损坏回退（注册表是机器写出
/// 的，手工损坏走出厂逃生舱，不做局部抢救）。
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
    // 搬迁意图的引用完整性（issue #836）：意图必须指向已登记账本、来源目录
    // 齐备。悬空意图与悬空活动指针同等对待——注册表是机器写出的，出现悬空
    // 即手工损坏，整体按损坏走出厂逃生舱，不做局部抢救。
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
        relocation: registry.pending_relocation.clone(),
    })?;
    atomic_write(&path, content.as_bytes())
}

// -------------------------------------------------------------------------
// 登记变更命令内核（issue #833）：新建 / 切换 / 改名 / 移除。每个入口都是
// 「读注册表现场 → 变更 → 原子写」的完整事务：基于文件最新态而非引导快照
// 变更（出厂未配置形态在此折叠默认账本，首次登记即落新格式）；注册表损坏
// 时拒绝变更（[`RegistryRead::Corrupt`] → 码化错误，不覆盖损坏文件——它可能
// 是自定义位置的唯一记录，覆盖前须走既有「恢复默认位置」逃生舱）。写入
// 时机契约的另一面（推迟搬迁窗口禁止变更）由引导态承载，在命令壳
// （`data_location::mutable_registry`）预检，文件态无从得知。
// -------------------------------------------------------------------------

/// 账本注册表损坏（变更拒绝路径的统一错误构造：码 + 消息模板单一来源，
/// 引导态预检与文件态读取共用，防同码消息漂移；原因随参数携带）。
pub(crate) fn registry_corrupt_error(reason: &str) -> crate::error::AppError {
    crate::error::AppError::codedp(
        "book.registry-corrupt",
        format!("账本注册表损坏，已回退默认账本；请先在数据设置中恢复默认位置（{reason}）"),
        &[reason],
    )
}

/// 搬迁未完成（变更拒绝路径的统一错误构造：码 + 消息模板单一来源）。两个
/// 独立条件共用同码：引导态的密文库推迟搬迁窗口（`mutable_registry` 预检）
/// 与文件态的未消费搬迁意图（[`settle_pending_relocation`]）——条件同属
/// 「搬迁尚未完成，登记变更会让目录变更落空」。
pub(crate) fn registry_busy_error() -> crate::error::AppError {
    crate::error::AppError::coded(
        "book.registry-busy",
        "数据搬迁尚未完成，暂无法变更账本登记；请重启应用完成搬迁后再试",
    )
}

/// 登记变更前的搬迁意图守卫（写入时机契约的多账本延伸，issue #836）：
/// - 无意图 → 直接放行；
/// - 意图未完成（目标目录尚无库、来源目录仍有库）→ 拒绝变更（同码
///   `book.registry-busy`）——此刻基于清单变更会让意图落空、账本目录悬空；
/// - 意图已完成（目标已有库，或来源与目标皆无库）→ 就地消费（清除意图）
///   后放行，随后的登记写入自然把已消费的意图从文件中一并带走（自愈）。
pub(crate) fn settle_pending_relocation(registry: &mut BookRegistry) -> Result<()> {
    let Some(pending) = registry.pending_relocation.clone() else {
        return Ok(());
    };
    // 校验已保证意图不悬空（读取与写入共用同一份校验）；防御性兜底按未完成拒绝。
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

/// 新建账本：在应用数据目录下自动创建子目录（`books/<id>`）并登记。
/// 只登记不建库——空目录由既有建连迁移在首次进入时建出全新空库（默认种子
/// 照常），不新建造库逻辑。返回登记后的账本条目。
pub fn create_book_entry(default_dir: &Path, name: &str) -> Result<Book> {
    let name = required_name(name)?;
    let mut registry = read_for_mutation(default_dir)?;
    let id = super::new_uuid();
    let dir = default_dir.join(BOOKS_DIR_NAME).join(&id);
    ensure_dir_available(&dir)?;
    if let Err(e) = ensure_dir_not_registered(&registry, &dir) {
        // 查重拒绝时清理刚创建的空目录（remove_dir 只删空目录、best-effort，
        // 清理对象是本函数刚建的现场而非任何既有用户文件）。
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

/// 切换活动账本：校验目标可用后改写活动指针落盘，文件与清单零变化。
/// 重引导（进目标账本）由前端在命令成功后复用原位重引导（ADR-0080）完成。
/// 返回目标账本条目。
pub fn switch_active_book(default_dir: &Path, id: &str) -> Result<Book> {
    let mut registry = read_for_mutation(default_dir)?;
    let book = registered_book(&registry, id)?.clone();
    if registry.active_id == id {
        return Err(crate::error::AppError::coded(
            "book.already-active",
            "该账本已是当前账本",
        ));
    }
    // 目标可用性与引导同口径：目录可创建即可用（引导只 ensure 目录存在，
    // 空目录由建连迁移建出新库）；不可用在此拒绝，活动指针保持不变。
    ensure_dir_available(&book.dir)?;
    registry.active_id = id.to_string();
    write_registry(default_dir, &registry)?;
    Ok(book)
}

/// 改账本展示名：只动注册表元数据，目录与库文件零变化。返回更新后的条目。
pub fn rename_book_entry(default_dir: &Path, id: &str, name: &str) -> Result<Book> {
    let name = required_name(name)?;
    let mut registry = read_for_mutation(default_dir)?;
    registered_book_mut(&mut registry, id)?.name = name.to_string();
    write_registry(default_dir, &registry)?;
    Ok(registered_book(&registry, id)?.clone())
}

/// 移除账本：只摘登记，目录与库文件原样保留（文件生命周期归用户，可重新
/// 登记找回）。活动账本不可移除——先切换到其他账本；唯一账本必是活动账本，
/// 自然被同一条规则保护。
pub fn remove_book_entry(default_dir: &Path, id: &str) -> Result<()> {
    let mut registry = read_for_mutation(default_dir)?;
    // 判序：不存在先于活动本（not-found 优先，避免悬空指针场景错位报码）。
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

/// 待登记目录与既有条目的物理目录重复时拒绝（同一目录不得重复登记）。
/// 路径身份按物理位置折叠：既有条目 best-effort canonicalize（目录可能已
/// 不存在，失败用原样），待登记目录必须已存在（create 刚创建）。create 的
/// 目录名是现铸 uuid、天然全新，此查重是登记接缝的纵深防御。
fn ensure_dir_not_registered(registry: &BookRegistry, dir: &Path) -> Result<()> {
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

/// 目标目录可用（可创建/已存在）；不可用返回码化错误，供切换前拦截。
fn ensure_dir_available(target: &Path) -> Result<()> {
    std::fs::create_dir_all(target).map_err(|e| {
        crate::error::AppError::codedp(
            "book.dir-unavailable",
            format!("账本目录不可用（{}）：{e}", target.display()),
            &[&target.to_string_lossy(), &e.to_string()],
        )
    })
}

/// 读注册表现场（变更入口共用的第一步）：未配置折叠为出厂默认账本（首次
/// 登记落新格式），损坏拒绝变更（不覆盖损坏文件，与引导态预检同一条件
/// 同一码同一消息模板——码即条件，前端按码稳定命中）。
fn read_current(default_dir: &Path) -> Result<BookRegistry> {
    match read_registry(default_dir) {
        RegistryRead::Resolved(registry) => Ok(registry),
        RegistryRead::Unconfigured => Ok(BookRegistry::single_default(default_dir)),
        RegistryRead::Corrupt(reason) => Err(registry_corrupt_error(&reason)),
    }
}

/// 读注册表现场并消费已完成/拦截未完成的搬迁意图（四个变更内核共用的第一步，
/// issue #836）：在 [`read_current`] 之上叠加 [`settle_pending_relocation`]——
/// 变更基于文件最新态，意图的消费与拒绝也以文件态为准（引导态预检
/// `mutable_registry` 只是前置过滤，此处是权威判定）。
fn read_for_mutation(default_dir: &Path) -> Result<BookRegistry> {
    let mut registry = read_current(default_dir)?;
    settle_pending_relocation(&mut registry)?;
    Ok(registry)
}

/// 展示名参数校验：trim 后非空，返回 trim 结果。
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

/// 按 id 取已登记账本（只读）；未登记返回码化错误。
fn registered_book<'a>(registry: &'a BookRegistry, id: &str) -> Result<&'a Book> {
    registry
        .books
        .iter()
        .find(|book| book.id == id)
        .ok_or_else(|| unregistered_book_error(id))
}

/// 按 id 取已登记账本（可变）；未登记返回码化错误。
fn registered_book_mut<'a>(registry: &'a mut BookRegistry, id: &str) -> Result<&'a mut Book> {
    registry
        .books
        .iter_mut()
        .find(|book| book.id == id)
        .ok_or_else(|| unregistered_book_error(id))
}

/// 未登记账本的码化错误（not-found，参数携带 id）。
fn unregistered_book_error(id: &str) -> crate::error::AppError {
    crate::error::AppError::codedp_not_found("book.not-found", format!("账本不存在: {id}"), &[id])
}

#[cfg(test)]
mod tests;
