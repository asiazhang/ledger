use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use cucumber::World;
use rusqlite::Connection;

use tauri_app_lib::commands::data_location::{DataLocationChangeOutcome, DataLocationInfo};
use tauri_app_lib::db::{init_db, open_in_memory};
use tauri_app_lib::models::{
    CreateTransactionResult, DashboardOverview, ItemDailyCost, ItemWithDailyCost, Transaction,
    TransactionInput, TransactionSearchResult,
};
use tauri_app_lib::transaction::amount::TransactionKind;

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
            kind: TransactionKind::parse(&self.kind)
                .unwrap_or_else(|e| panic!("非法 kind: {}（{e}）", self.kind)),
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
    /// 最近一次自动备份产物的路径（来源标记场景用）
    pub last_auto_backup_path: Option<PathBuf>,
    /// 最近创建的定时计划 id（定时交易场景用）
    pub last_plan_id: Option<String>,
    /// 最近尝试执行的期次 id（失败重试场景用）
    pub last_occurrence_id: Option<String>,
    /// 最近一次交易搜索结果快照（搜索场景断言用）
    pub last_search: Option<TransactionSearchResult>,
    /// 最近创建的物品 id（物品场景断言用）
    pub last_item_id: Option<String>,
    /// 最近一次物品写入发出的失效信号次数（ledger:changed 注入 seam 断言用）
    pub item_signal_count: usize,
    /// 物品列表快照（物品列表场景断言用）
    pub items_list: Vec<ItemWithDailyCost>,
    /// 记住的物品创建时间（修改后审计字段保留断言用，issue #117）
    pub remembered_item_created_at: Option<String>,
    /// 记住的关联购买交易 id（issue #119 自动带出/溯源断言用）
    pub remembered_purchase_transaction_id: Option<String>,
    /// 最近一次自选参考日重算的结果快照（issue #121 断言用）
    pub last_item_cost: Option<ItemDailyCost>,
    /// 最近一次净资产总览快照（首页仪表盘场景断言用）
    pub last_overview: Option<DashboardOverview>,
    /// 最近一次订阅实际花费总览快照（订阅花费场景断言用，issue #160）
    pub last_spend: Option<tauri_app_lib::scheduled_transactions::SubscriptionSpendOverview>,
    /// DataLocation 引导场景：默认应用数据目录（真临时目录）
    pub dl_default_dir: Option<PathBuf>,
    /// DataLocation 引导场景：指针指向的目标目录
    pub dl_target_dir: Option<PathBuf>,
    /// 最近一次 DataLocation 引导结果（回退信号断言用）
    pub last_boot: Option<tauri_app_lib::db::data_location::Boot>,
    /// DataLocation 引导/重置场景中打开的文件库连接
    pub dl_conn: Option<tauri_app_lib::db::DbState>,
    /// DataLocation 引导场景：默认目录库文件字节快照（原样保留断言用）
    pub dl_default_db_bytes: Option<Vec<u8>>,
    /// 最近一次 DataLocation 信息查询结果（#133 命令层断言用）
    pub dl_last_info: Option<DataLocationInfo>,
    /// 最近一次更改意图提交结果（#133 命令层断言用）
    pub dl_last_outcome: Option<DataLocationChangeOutcome>,
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
            .field("last_auto_backup", &self.last_auto_backup_path)
            .field(
                "last_search_total",
                &self.last_search.as_ref().map(|s| s.total),
            )
            .field("last_overview", &self.last_overview.is_some())
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
            last_auto_backup_path: None,
            last_plan_id: None,
            last_occurrence_id: None,
            last_search: None,
            last_overview: None,
            last_spend: None,
            last_item_id: None,
            item_signal_count: 0,
            items_list: Vec::new(),
            remembered_item_created_at: None,
            remembered_purchase_transaction_id: None,
            last_item_cost: None,
            dl_default_dir: None,
            dl_target_dir: None,
            last_boot: None,
            dl_conn: None,
            dl_default_db_bytes: None,
            dl_last_info: None,
            dl_last_outcome: None,
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
