use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use cucumber::World;

use ledger_dashboard::DashboardOverview;
use ledger_infra::db::DbState;
use ledger_infra::db::data_location::{DataLocationChangeOutcome, DataLocationInfo};
use ledger_item::{ItemDailyCost, ItemWithDailyCost};
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{
    CreateTransactionResult, Transaction, TransactionInput, TransactionSearchResult,
};

macro_rules! world_conn {
    ($world:expr) => {
        $world.db.conn.lock().unwrap_or_else(|e| e.into_inner())
    };
}

macro_rules! world_write {
    ($world:expr, $f:expr) => {{
        let conn = $world.db.conn.lock().unwrap_or_else(|e| e.into_inner());
        ledger_infra::db::write_locked(&conn, $f)
    }};
}

#[derive(Clone, Debug)]
pub struct ImportedRow {
    pub kind: String,
    pub amount_cents: i64,
    pub currency_code: String,
    pub account_name: String,
    pub to_account_name: Option<String>,
    pub note: Option<String>,
    pub date: String,
    pub merchant_name: Option<String>,
    pub idempotency_key: Option<String>,
}

impl ImportedRow {
    pub fn to_input(&self, world: &LedgerWorld) -> TransactionInput {
        TransactionInput {
            funding: Vec::new(),
            kind: TransactionKind::parse(&self.kind)
                .unwrap_or_else(|e| panic!("非法 kind: {}（{e}）", self.kind)),
            amount_cents: self.amount_cents,
            currency_code: self.currency_code.clone(),
            account_id: Some(world.account_id(&self.account_name)),
            to_account_id: self
                .to_account_name
                .as_deref()
                .map(|name| world.account_id(name)),
            category_id: None,
            merchant_id: None,
            merchant_name: self.merchant_name.clone(),
            policy_id: None,
            refund_of_transaction_id: None,
            funding_account_id: None,
            source_order_no: None,
            note: self.note.clone(),
            date: self.date.clone(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            to_instrument_id: None,
            to_quantity: None,
            out_amount_cents: None,
            in_amount_cents: None,
            idempotency_key: self.idempotency_key.clone(),
            origin: None,
            fx_rate: None,
            purchases: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct TransactionGroup {
    pub last_transaction_id: Option<String>,
    pub transactions_list: Vec<Transaction>,
    pub last_search: Option<TransactionSearchResult>,
    pub balances: HashMap<String, (i64, bool)>,
    pub last_import_rows: Vec<ImportedRow>,
    pub last_batch_results: Vec<CreateTransactionResult>,
}

#[derive(Default)]
pub struct PlanGroup {
    pub last_plan_id: Option<String>,
    pub last_occurrence_id: Option<String>,
    pub last_spend: Option<ledger_scheduled::SubscriptionSpendOverview>,
    pub last_catch_up: Option<ledger_scheduled::CatchUpSummary>,
    pub last_detail: Option<ledger_scheduled::ScheduledTransactionDetail>,
}

#[derive(Default)]
pub struct AssetGroup {
    pub last_instrument_search: Option<ledger_investment::InstrumentListResult>,
    pub last_instrument_list: Option<ledger_investment::InstrumentListResult>,
    pub last_portfolio_trend: Option<ledger_investment::PortfolioValueTrend>,
    pub last_financial_freedom: Option<ledger_investment::FinancialFreedomOverview>,
    pub last_investment_transactions: Option<ledger_investment::InvestmentTransactionListResult>,
    pub last_physical_asset_id: Option<String>,
    pub physical_asset_signal_count: usize,
    pub physical_assets_list: Option<ledger_physical_asset::PhysicalAssetList>,
    pub physical_asset_detail: Option<ledger_physical_asset::PhysicalAsset>,
}

#[derive(Default)]
pub struct ItemGroup {
    pub last_item_id: Option<String>,
    pub item_signal_count: usize,
    pub items_list: Vec<ItemWithDailyCost>,
    pub remembered_item_created_at: Option<String>,
    pub remembered_purchase_transaction_id: Option<String>,
    pub last_item_cost: Option<ItemDailyCost>,
}

impl ItemGroup {
    pub fn nth(&self, n: usize) -> &ItemWithDailyCost {
        self.items_list
            .get(n - 1)
            .unwrap_or_else(|| panic!("物品列表第 {n} 件不存在"))
    }
}

#[derive(Default)]
pub struct PolicyGroup {
    pub last_policy_id: Option<String>,
    pub policy_signal_count: usize,
    pub policies_list: Vec<ledger_policy::Policy>,
    pub policy_stats_list: Vec<ledger_policy::PolicyStats>,
    pub remembered_policy_created_at: Option<String>,
    pub last_insurer_by_name_id: Option<String>,
}

#[derive(Default)]
pub struct ReportGroup {
    pub last_overview: Option<DashboardOverview>,
    pub last_budget_progress: Vec<ledger_budget::BudgetProgress>,
    pub last_merchant_shares: Vec<ledger_reports::MerchantShare>,
    pub last_monthly_summary: Vec<ledger_reports::MonthlySummary>,
    pub last_category_shares: Vec<ledger_reports::CategoryShare>,
    pub last_savings_goal_progress: Option<Vec<ledger_savings_goal::SavingsGoalProgress>>,
    pub last_date_range: Option<ledger_reports::DateRange>,
}

#[derive(Default)]
pub struct BootGroup {
    pub last_backup_path: Option<PathBuf>,
    pub restored_db_path: Option<PathBuf>,
    pub last_auto_backup_path: Option<PathBuf>,
    pub last_prune: Option<ledger_backup::PruneResult>,
    pub auto_backup_dir: Option<PathBuf>,
    pub restore_safety_dir: Option<PathBuf>,
    pub dl_default_dir: Option<PathBuf>,
    pub dl_target_dir: Option<PathBuf>,
    pub last_boot: Option<ledger_infra::db::data_location::Boot>,
    pub dl_conn: Option<DbState>,
    pub dl_default_db_bytes: Option<Vec<u8>>,
    pub dl_last_info: Option<DataLocationInfo>,
    pub dl_last_outcome: Option<DataLocationChangeOutcome>,
    pub enc_dir: Option<PathBuf>,
    pub enc_target_dir: Option<PathBuf>,
    pub enc_last_error: Option<ledger_infra::error::AppError>,
    pub enc_conn: Option<rusqlite::Connection>,
    pub enc_db_bytes: Option<Vec<u8>>,
    pub enc_last_plan: Option<Result<ledger_infra::db::boot::BootDisposition, String>>,
    pub sf_last_takeover: Option<StartupTakeover>,
    pub sf_resolved_dir: Option<PathBuf>,
    pub book_last_disposition: Option<Result<ledger_infra::db::boot::BootDisposition, String>>,
    pub book_last_list: Option<ledger_infra::db::book_registry::BookListInfo>,
    pub sync_stub: Option<tauri_app_lib::test_support::S3Stub>,
    pub sync_last_auto_round:
        Option<Result<Option<ledger_sync_engine::SyncRoundReport>, ledger_infra::error::AppError>>,
    pub sync_last_report: Option<ledger_sync_engine::SyncRoundReport>,
    pub sync_session_encrypted: bool,
    pub scratch: Vec<tauri_app_lib::test_support::ScratchDir>,
}

#[derive(World)]
#[world(init = Self::new)]
pub struct LedgerWorld {
    pub db: DbState,
    pub account_name_to_id: HashMap<String, String>,
    pub merchant_name_to_id: HashMap<String, String>,
    pub insurer_name_to_id: HashMap<String, String>,
    pub category_name_to_id: HashMap<String, String>,
    pub last_error: Option<String>,
    pub last_app_error: Option<ledger_infra::error::AppError>,
    pub frozen_today: Option<chrono::NaiveDate>,
    pub txn: TransactionGroup,
    pub plan: PlanGroup,
    pub asset: AssetGroup,
    pub item: ItemGroup,
    pub policy: PolicyGroup,
    pub report: ReportGroup,
    pub boot: BootGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupTakeover {
    Opened,
    AwaitUnlock,
    Failed,
}

impl fmt::Debug for LedgerWorld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LedgerWorld")
            .field("account_count", &self.account_name_to_id.len())
            .field("last_transaction_id", &self.txn.last_transaction_id)
            .field("last_error", &self.last_error)
            .field("transactions_count", &self.txn.transactions_list.len())
            .field("balances_count", &self.txn.balances.len())
            .field("last_backup", &self.boot.last_backup_path)
            .field("last_auto_backup", &self.boot.last_auto_backup_path)
            .field("auto_backup_dir", &self.boot.auto_backup_dir)
            .field(
                "last_search_total",
                &self.txn.last_search.as_ref().map(|s| s.total),
            )
            .field("last_overview", &self.report.last_overview.is_some())
            .field("last_plan_id", &self.plan.last_plan_id)
            .field("last_occurrence_id", &self.plan.last_occurrence_id)
            .field("sf_last_takeover", &self.boot.sf_last_takeover)
            .finish()
    }
}

impl LedgerWorld {
    pub fn scratch_dir(&mut self, tag: &str) -> PathBuf {
        let guard = tauri_app_lib::test_support::ScratchDir::new(tag);
        let path = guard.path().to_path_buf();
        self.boot.scratch.push(guard);
        path
    }

    fn new() -> Self {
        ledger_backup::install_after_commit_hook();
        ledger_sync_engine::trigger::install_after_write_hook();
        ledger_accounts::balance::install_balance_refresh_hook();
        ledger_scheduled::install_plan_source_hook();
        ledger_scheduled::auto_run::register_after_occurrence_hook(
            ledger_backup::occurrence_dirty_hook,
        );
        ledger_backup::register_catch_up_hook(ledger_scheduled::auto_run::catch_up_hook);
        tauri_app_lib::transaction_wiring::install_all();
        let mut world = Self {
            db: DbState::open_in_memory().expect("数据库初始化失败"),
            account_name_to_id: HashMap::new(),
            merchant_name_to_id: HashMap::new(),
            insurer_name_to_id: HashMap::new(),
            category_name_to_id: HashMap::new(),
            last_error: None,
            last_app_error: None,
            frozen_today: None,
            txn: TransactionGroup::default(),
            plan: PlanGroup::default(),
            asset: AssetGroup::default(),
            item: ItemGroup::default(),
            policy: PolicyGroup::default(),
            report: ReportGroup::default(),
            boot: BootGroup::default(),
        };
        let hidden: Vec<(String, String)> = {
            let conn = world_conn!(world);
            let mut stmt = conn
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

    pub fn account_id(&self, name: &str) -> String {
        self.account_name_to_id
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("账户 '{}' 不存在", name))
    }

    pub fn merchant_id(&self, name: &str) -> String {
        self.merchant_name_to_id
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("商户 '{}' 不存在", name))
    }

    pub fn insurer_id(&self, name: &str) -> String {
        self.insurer_name_to_id
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("保司 '{}' 不存在", name))
    }

    pub fn category_id(&self, name: &str) -> String {
        self.category_name_to_id
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("分类 '{}' 不存在", name))
    }
}
