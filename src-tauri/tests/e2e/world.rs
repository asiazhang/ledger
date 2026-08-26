use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use cucumber::World;
use rusqlite::Connection;

use tauri_app_lib::db::{init_db, open_in_memory};
use tauri_app_lib::models::{
    CreateTransactionResult, Transaction, TransactionInput, TransactionSearchResult,
};

/// 批量导入的一行（重跑导入时据此重建 `TransactionInput`）。
/// 记录账户/转入账户的**名称**而非 ID，保证重跑时重新解析（与真实导入流程一致）。
#[derive(Clone, Debug)]
pub struct ImportedRow {
    pub kind: String,
    pub amount_cents: i64,
    pub currency_code: String,
    pub account_name: String,
    pub to_account_name: Option<String>,
    pub note: Option<String>,
    pub date: String,
    /// 客户端提供的导入幂等键（内容无关身份，重跑时保持不变）。
    pub idempotency_key: Option<String>,
}

impl ImportedRow {
    pub fn to_input(&self, world: &LedgerWorld) -> TransactionInput {
        TransactionInput {
            kind: self.kind.clone(),
            amount_cents: self.amount_cents,
            currency_code: self.currency_code.clone(),
            account_id: world.account_id(&self.account_name),
            to_account_id: self
                .to_account_name
                .as_deref()
                .map(|name| world.account_id(name)),
            category_id: None,
            refund_of_transaction_id: None,
            note: self.note.clone(),
            date: self.date.clone(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: self.idempotency_key.clone(),
        }
    }
}

/// Cucumber World：每个 Scenario 独立持有一个 in-memory SQLite 数据库。
#[derive(World)]
#[world(init = Self::new)]
pub struct LedgerWorld {
    /// In-memory 数据库连接（每个 scenario 独立）
    pub conn: Connection,
    /// 账户名称到 ID 的映射（Given 步骤插入账户后注册，含种子黑洞账户）
    pub account_name_to_id: HashMap<String, String>,
    /// 最新创建的交易 ID（用于关联操作如退款）
    pub last_transaction_id: Option<String>,
    /// 最近一次操作错误（检查失败场景）
    pub last_error: Option<String>,
    /// 交易列表快照（用于 Then 断言）
    pub transactions_list: Vec<Transaction>,
    /// 最近一次批量导入的原始行（重跑导入用）
    pub last_import_rows: Vec<ImportedRow>,
    /// 最近一次批量导入的逐条结果（含 `duplicate` 标记）
    pub last_batch_results: Vec<CreateTransactionResult>,
    /// 最近一次查询的账户余额快照（账户名 → (余额, is_hidden)，含黑洞账户）
    pub balances: HashMap<String, (i64, bool)>,
    /// 最近一次备份文件的路径（备份/恢复场景用）
    pub last_backup_path: Option<PathBuf>,
    /// 最近一次恢复出的临时数据库路径
    pub restored_db_path: Option<PathBuf>,
    /// 最近创建的定时计划 id（定时交易场景用）
    pub last_plan_id: Option<String>,
    /// 最近尝试执行的期次 id（失败重试场景用）
    pub last_occurrence_id: Option<String>,
    /// 最近一次交易搜索结果快照（搜索场景断言用）
    pub last_search: Option<TransactionSearchResult>,
}

impl fmt::Debug for LedgerWorld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LedgerWorld")
            .field("account_count", &self.account_name_to_id.len())
            .field("last_transaction_id", &self.last_transaction_id)
            .field("last_error", &self.last_error)
            .field("transactions_count", &self.transactions_list.len())
            .field("balances_count", &self.balances.len())
            .field("last_backup", &self.last_backup_path)
            .field(
                "last_search_total",
                &self.last_search.as_ref().map(|s| s.total),
            )
            .field("last_plan_id", &self.last_plan_id)
            .field("last_occurrence_id", &self.last_occurrence_id)
            .finish()
    }
}

impl LedgerWorld {
    fn new() -> Self {
        let mut conn = open_in_memory().expect("无法创建内存数据库");
        init_db(&mut conn).expect("数据库初始化失败");
        let mut world = Self {
            conn,
            account_name_to_id: HashMap::new(),
            last_transaction_id: None,
            last_error: None,
            transactions_list: Vec::new(),
            last_import_rows: Vec::new(),
            last_batch_results: Vec::new(),
            balances: HashMap::new(),
            last_backup_path: None,
            restored_db_path: None,
            last_plan_id: None,
            last_occurrence_id: None,
            last_search: None,
        };
        // 注册种子黑洞账户（V004 预置 无(CNY)/无(HKD)），供迁移场景按名称引用。
        let hidden: Vec<(String, String)> = {
            let mut stmt = world
                .conn
                .prepare("SELECT id, name FROM accounts WHERE is_hidden=1")
                .expect("查询黑洞账户失败");
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .expect("查询黑洞账户失败");
            rows.flatten().collect()
        };
        for (id, name) in hidden {
            world.account_name_to_id.insert(name, id);
        }
        world
    }

    /// 获取账户 ID，按名称查找
    pub fn account_id(&self, name: &str) -> String {
        self.account_name_to_id
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("账户 '{}' 不存在", name))
    }
}
