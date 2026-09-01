use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use cucumber::World;

use tauri_app_lib::commands::data_location::{DataLocationChangeOutcome, DataLocationInfo};
use tauri_app_lib::db::DbState;
use tauri_app_lib::models::{
    CreateTransactionResult, DashboardOverview, ItemDailyCost, ItemDailyTotal, ItemWithDailyCost,
    Transaction, TransactionInput, TransactionSearchResult,
};
use tauri_app_lib::transaction::amount::TransactionKind;

/// 连接守卫（BDD 步骤专用宏）：取连接锁，展开为字段访问链——
/// 借用发生在 `world.db.conn` 字段路径上而非整个 world，与步骤内对
/// world 其他字段的赋值共存（disjoint borrow）。守卫需跨语句存活时
/// 先绑定局部变量（`let conn = world_conn!(world);`）。迁移期过渡形态
/// （ADR-0032）：置脏语义相关写路径应优先走 `world.db.write` 写入口。
macro_rules! world_conn {
    ($world:expr) => {
        $world.db.conn.lock().unwrap_or_else(|e| e.into_inner())
    };
}

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
    /// 商户名字符串（AI 导入契约，issue #194）：后端精确匹配复用或即建，AI 不负责去重。
    pub merchant_name: Option<String>,
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
            merchant_id: None,
            merchant_name: self.merchant_name.clone(),
            policy_id: None,
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
    /// 数据库连接（写入口形态，ADR-0032）：断言/读路径经 [`LedgerWorld::conn`]
    /// 取守卫，置脏语义相关写路径经 `db.write` 走连接层统一写入口。
    pub db: DbState,
    /// 账户名称到 ID 的映射（Given 步骤插入账户后注册，含种子黑洞账户）
    pub account_name_to_id: HashMap<String, String>,
    /// 商户名称到 ID 的映射（Given/When 步骤创建商户后注册）
    pub merchant_name_to_id: HashMap<String, String>,
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
    /// 投资迁移链路（issue #297）：已导入 buy 交易 id 按导入先后累积
    /// （「持仓批次按导入先后锚定顺序」步骤据此回填批次 created_at）
    pub imported_buy_txn_ids: Vec<String>,
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
    /// 最近一次标的搜索结果快照（标的搜索语义场景断言用，issue #199）
    pub last_instrument_search: Option<tauri_app_lib::models::InstrumentListResult>,
    /// 最近一次组合走势查询快照（组合走势场景断言用，issue #248）
    pub last_portfolio_trend: Option<tauri_app_lib::models::PortfolioValueTrend>,
    /// 最近一次单标的走势查询快照（基金净值走势场景断言用，issue #303）
    pub last_instrument_trend: Option<tauri_app_lib::models::InstrumentPriceTrend>,
    /// 最近创建的物品 id（物品场景断言用）
    pub last_item_id: Option<String>,
    /// 最近一次物品写入发出的失效信号次数（ledger:changed 注入 seam 断言用）
    pub item_signal_count: usize,
    /// 物品列表快照（物品列表场景断言用）
    pub items_list: Vec<ItemWithDailyCost>,
    /// 最近创建的保单 id（保单场景断言用，issue #360）
    pub last_policy_id: Option<String>,
    /// 最近一次保单写入发出的失效信号次数（ledger:changed 注入 seam 断言用）
    pub policy_signal_count: usize,
    /// 保单列表快照（保单列表场景断言用）
    pub policies_list: Vec<tauri_app_lib::models::Policy>,
    /// 记住的保单创建时间（编辑后审计字段保留断言用，issue #360）
    pub remembered_policy_created_at: Option<String>,
    /// 记住的物品创建时间（修改后审计字段保留断言用，issue #117）
    pub remembered_item_created_at: Option<String>,
    /// 记住的关联购买交易 id（issue #119 自动带出/溯源断言用）
    pub remembered_purchase_transaction_id: Option<String>,
    /// 最近一次自选参考日重算的结果快照（issue #121 断言用）
    pub last_item_cost: Option<ItemDailyCost>,
    /// 最近一次在用物品每天成本合计快照（issue #122 dashboard 汇总卡断言用）
    pub last_item_daily_total: Option<ItemDailyTotal>,
    /// 最近一次净资产总览快照（首页仪表盘场景断言用）
    pub last_overview: Option<DashboardOverview>,
    /// 最近一次财务自由度总览快照（自由度口径场景断言用，issue #343）
    pub last_financial_freedom: Option<tauri_app_lib::models::FinancialFreedomOverview>,
    /// 最近一次订阅实际花费总览快照（订阅花费场景断言用，issue #160）
    pub last_spend: Option<tauri_app_lib::scheduled_transactions::SubscriptionSpendOverview>,
    /// 最近一次预算进度快照（预算滚动窗口场景断言用，issue #182）
    pub last_budget_progress: Vec<tauri_app_lib::models::BudgetProgress>,
    /// 最近一次商户消费排行快照（报表商户排行场景断言用，issue #192）
    pub last_merchant_shares: Vec<tauri_app_lib::models::MerchantShare>,
    /// 最近一次报表年份筛选范围快照（报表年份范围场景断言用，issue #266）
    pub last_year_range: Option<tauri_app_lib::models::YearRange>,
    /// 最近一次追补入口执行汇总快照（自动执行追补场景断言用，issue #307）
    pub last_catch_up: Option<tauri_app_lib::scheduled_transactions::CatchUpSummary>,
    /// 最近一次定时计划详情快照（期次详情弹窗场景断言用，issue #205）
    pub last_detail: Option<tauri_app_lib::scheduled_transactions::ScheduledTransactionDetail>,
    /// 场景冻结的本地今日（滚动窗口步骤口径一致用，issue #182）
    pub frozen_today: Option<chrono::NaiveDate>,
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
        let mut world = Self {
            db: DbState::open_in_memory().expect("数据库初始化失败"),
            account_name_to_id: HashMap::new(),
            merchant_name_to_id: HashMap::new(),
            last_transaction_id: None,
            last_error: None,
            transactions_list: Vec::new(),
            last_import_rows: Vec::new(),
            last_batch_results: Vec::new(),
            imported_buy_txn_ids: Vec::new(),
            balances: HashMap::new(),
            last_backup_path: None,
            restored_db_path: None,
            last_auto_backup_path: None,
            last_plan_id: None,
            last_occurrence_id: None,
            last_search: None,
            last_instrument_search: None,
            last_portfolio_trend: None,
            last_instrument_trend: None,
            last_overview: None,
            last_financial_freedom: None,
            last_spend: None,
            last_budget_progress: Vec::new(),
            last_merchant_shares: Vec::new(),
            last_year_range: None,
            last_catch_up: None,
            last_detail: None,
            frozen_today: None,
            last_item_id: None,
            item_signal_count: 0,
            items_list: Vec::new(),
            last_policy_id: None,
            policy_signal_count: 0,
            policies_list: Vec::new(),
            remembered_policy_created_at: None,
            remembered_item_created_at: None,
            remembered_purchase_transaction_id: None,
            last_item_cost: None,
            last_item_daily_total: None,
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

    /// 获取账户 ID，按名称查找
    pub fn account_id(&self, name: &str) -> String {
        self.account_name_to_id
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("账户 '{}' 不存在", name))
    }

    /// 获取商户 ID，按名称查找
    pub fn merchant_id(&self, name: &str) -> String {
        self.merchant_name_to_id
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("商户 '{}' 不存在", name))
    }
}
