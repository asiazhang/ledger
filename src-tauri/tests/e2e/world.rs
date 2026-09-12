use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use cucumber::World;

use tauri_app_lib::dashboard::DashboardOverview;
use tauri_app_lib::db::DbState;
use tauri_app_lib::db::data_location::{DataLocationChangeOutcome, DataLocationInfo};
use tauri_app_lib::item::{ItemDailyCost, ItemDailyTotal, ItemWithDailyCost};
use tauri_app_lib::transaction::amount::TransactionKind;
use tauri_app_lib::transaction::{
    CreateTransactionResult, Transaction, TransactionInput, TransactionSearchResult,
};

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
            funding_account_id: None,
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
        }
    }
}

// ---------------------------------------------------------------------------
// 快照分组（ADR-0086 决策 6）：world 快照按消费域聚合的七个分组结构。
// 本体只保留跨域基础设施（连接、名称注册表、错误状态、冻结时钟）；
// 新快照必须归入既有分组，回退本体平铺即回潮（见 CONTEXT-testing 快照分组）。
// ---------------------------------------------------------------------------

/// 交易组快照：交易写入/导入/搜索/余额场景的状态。
#[derive(Default)]
pub struct TransactionGroup {
    /// 最新创建的交易 ID（用于关联操作如退款）
    pub last_transaction_id: Option<String>,
    /// 交易列表快照（用于 Then 断言）
    pub transactions_list: Vec<Transaction>,
    /// 最近一次交易搜索结果快照（搜索场景断言用）
    pub last_search: Option<TransactionSearchResult>,
    /// 最近一次查询的账户余额快照（账户名 → (余额, is_hidden)，含黑洞账户）
    pub balances: HashMap<String, (i64, bool)>,
    /// 最近一次批量导入的原始行（重跑导入用）
    pub last_import_rows: Vec<ImportedRow>,
    /// 最近一次批量导入的逐条结果（含 `duplicate` 标记）
    pub last_batch_results: Vec<CreateTransactionResult>,
}

/// 计划组快照：定时计划三形态/期次/订阅花费/追补场景的状态。
#[derive(Default)]
pub struct PlanGroup {
    /// 最近创建的定时计划 id（定时交易场景用）
    pub last_plan_id: Option<String>,
    /// 最近尝试执行的期次 id（失败重试场景用）
    pub last_occurrence_id: Option<String>,
    /// 最近一次订阅实际花费总览快照（订阅花费场景断言用，issue #160）
    pub last_spend: Option<tauri_app_lib::scheduled_transactions::SubscriptionSpendOverview>,
    /// 最近一次追补入口执行汇总快照（自动执行追补场景断言用，issue #307）
    pub last_catch_up: Option<tauri_app_lib::scheduled_transactions::CatchUpSummary>,
    /// 最近一次定时计划详情快照（期次详情弹窗场景断言用，issue #205）
    pub last_detail: Option<tauri_app_lib::scheduled_transactions::ScheduledTransactionDetail>,
}

/// 资产组快照（投资 + 实物资产，净资产同腿 ADR-0064）：
/// 标的搜索/走势/自由度/实物资产场景的状态。
#[derive(Default)]
pub struct AssetGroup {
    /// 最近一次标的搜索结果快照（标的搜索语义场景断言用，issue #199）
    pub last_instrument_search: Option<tauri_app_lib::investment::InstrumentListResult>,
    /// 最近一次按 id 精确取标的快照（走势 focus 消费解析路径断言用，issue #709）
    pub last_instrument: Option<tauri_app_lib::investment::Instrument>,
    /// 最近一次组合走势查询快照（组合走势场景断言用，issue #248）
    pub last_portfolio_trend: Option<tauri_app_lib::investment::PortfolioValueTrend>,
    /// 最近一次单标的走势查询快照（基金净值走势场景断言用，issue #303）
    pub last_instrument_trend: Option<tauri_app_lib::investment::InstrumentPriceTrend>,
    /// 最近一次财务自由度总览快照（自由度口径场景断言用，issue #343）
    pub last_financial_freedom: Option<tauri_app_lib::investment::FinancialFreedomOverview>,
    /// 最近创建的实物资产 id（详情/后续步骤定位用，issue #466）
    pub last_physical_asset_id: Option<String>,
    /// 最近一次实物资产写入发出的失效信号次数（ledger:changed 注入 seam 断言用）
    pub physical_asset_signal_count: usize,
    /// 实物资产列表快照（列表与合计场景断言用，issue #466）
    pub physical_assets_list: Option<tauri_app_lib::physical_asset::PhysicalAssetList>,
    /// 最近一次详情读回快照（详情场景断言用，issue #466）
    pub physical_asset_detail: Option<tauri_app_lib::physical_asset::PhysicalAsset>,
}

/// 物品组快照：物品创建/修改/处置/每日成本场景的状态。
#[derive(Default)]
pub struct ItemGroup {
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
    /// 最近一次在用物品每天成本合计快照（issue #122 dashboard 汇总卡断言用）
    pub last_item_daily_total: Option<ItemDailyTotal>,
}

impl ItemGroup {
    /// 取第 n 件（1 起）物品快照（分组读取辅助，自 items_common.rs 收编）。
    pub fn nth(&self, n: usize) -> &ItemWithDailyCost {
        self.items_list
            .get(n - 1)
            .unwrap_or_else(|| panic!("物品列表第 {n} 件不存在"))
    }
}

/// 保单组快照：保单建档/统计/保司字典场景的状态。
#[derive(Default)]
pub struct PolicyGroup {
    /// 最近创建的保单 id（保单场景断言用，issue #360）
    pub last_policy_id: Option<String>,
    /// 最近一次保单写入发出的失效信号次数（ledger:changed 注入 seam 断言用）
    pub policy_signal_count: usize,
    /// 保单列表快照（保单列表场景断言用）
    pub policies_list: Vec<tauri_app_lib::policy::Policy>,
    /// 最近一次逐保单统计快照（保单视角统计场景断言用，issue #363）
    pub policy_stats_list: Vec<tauri_app_lib::policy::PolicyStats>,
    /// 记住的保单创建时间（编辑后审计字段保留断言用，issue #360）
    pub remembered_policy_created_at: Option<String>,
    /// 最近一次按名创建保司（find-or-create）返回的 id（复用断言用）；
    /// 按消费场景归保险域，故入本组（issue #712）
    pub last_insurer_by_name_id: Option<String>,
}

/// 报表组快照：跨域消费端聚合（总览/预算进度/排行/汇总/份额/日期范围）。
#[derive(Default)]
pub struct ReportGroup {
    /// 最近一次净资产总览快照（首页仪表盘场景断言用）
    pub last_overview: Option<DashboardOverview>,
    /// 最近一次预算进度快照（预算滚动窗口场景断言用，issue #182）
    pub last_budget_progress: Vec<tauri_app_lib::budget::BudgetProgress>,
    /// 最近一次商户消费排行快照（报表商户排行场景断言用，issue #192）
    pub last_merchant_shares: Vec<tauri_app_lib::reports::MerchantShare>,
    /// 最近一次月度汇总快照（报表期间过滤场景断言用，issue #411）
    pub last_monthly_summary: Vec<tauri_app_lib::reports::MonthlySummary>,
    /// 最近一次分类份额快照（报表分类份额年份联动场景断言用，issue #376）
    pub last_category_shares: Vec<tauri_app_lib::reports::CategoryShare>,
    /// 最近一次报表日期筛选范围快照（报表日期范围场景断言用，issue #266 / #389）
    pub last_date_range: Option<tauri_app_lib::reports::DateRange>,
}

/// 引导组快照：备份 + 数据位置 + 加密 + 启动失败 + 账本登记 + 同步的文件级/
/// 引导级/进程级状态——同步与备份同为整库级进程能力，归并于此（issue #955）。
#[derive(Default)]
pub struct BootGroup {
    /// 最近一次备份文件的路径（备份/恢复场景用）
    pub last_backup_path: Option<PathBuf>,
    /// 最近一次恢复出的临时数据库路径
    pub restored_db_path: Option<PathBuf>,
    /// 最近一次自动备份产物的路径（来源标记场景用）
    pub last_auto_backup_path: Option<PathBuf>,
    /// 本场景自动备份产物所在目录（日界门场景复用同一目录：同日/跨日产物计数）
    pub auto_backup_dir: Option<PathBuf>,
    /// 备份作用域（issue #836）：触发入口的账本作用域注入；默认 None = 旧命名
    /// 兼容口径（既有场景产物名不变），按账本分域的场景显式设置。
    pub backup_scope: Option<tauri_app_lib::backup::BackupScope>,
    /// 最近一次恢复的恢复安全备份目录（加密库安全备份断言用，issue #572）
    pub restore_safety_dir: Option<PathBuf>,
    /// DataLocation 引导场景：默认应用数据目录（真临时目录）
    pub dl_default_dir: Option<PathBuf>,
    /// DataLocation 引导场景：指针指向的目标目录
    pub dl_target_dir: Option<PathBuf>,
    /// 最近一次 DataLocation 引导结果（回退信号断言用）
    pub last_boot: Option<tauri_app_lib::db::data_location::Boot>,
    /// DataLocation 引导/重置场景中打开的文件库连接
    pub dl_conn: Option<DbState>,
    /// DataLocation 引导场景：默认目录库文件字节快照（原样保留断言用）
    pub dl_default_db_bytes: Option<Vec<u8>>,
    /// 最近一次 DataLocation 信息查询结果（#133 命令层断言用）
    pub dl_last_info: Option<DataLocationInfo>,
    /// 最近一次更改意图提交结果（#133 命令层断言用）
    pub dl_last_outcome: Option<DataLocationChangeOutcome>,
    /// 加密场景（issue #570）：默认数据目录（真临时目录文件库）
    pub enc_dir: Option<PathBuf>,
    /// 加密场景：搬迁意图指向的目标目录
    pub enc_target_dir: Option<PathBuf>,
    /// 加密场景：最近一次转换/解锁/搬迁的错误（码化错误断言用）
    pub enc_last_error: Option<tauri_app_lib::error::AppError>,
    /// 加密场景：解锁成功后打开的文件库连接
    pub enc_conn: Option<rusqlite::Connection>,
    /// 加密场景：库文件字节快照（失败原子性断言用）
    pub enc_db_bytes: Option<Vec<u8>>,
    /// 原位重引导计划（issue #644 / ADR-0080）：最近一次 plan_boot 的处置判定
    pub enc_last_plan: Option<Result<tauri_app_lib::db::boot::BootDisposition, String>>,
    /// 启动失败恢复场景（issue #601）：最近一次启动处置接管结果
    pub sf_last_takeover: Option<StartupTakeover>,
    /// 启动失败恢复场景（issue #601）：以未登记引导解析出的生效库目录
    pub sf_resolved_dir: Option<PathBuf>,
    /// 账本场景（issue #835）：最近一次引导处置判定（密文库落解锁屏 /
    /// 明文库就绪建连，启动与原位重引导共用序列的产物）
    pub book_last_disposition: Option<Result<tauri_app_lib::db::boot::BootDisposition, String>>,
    /// 账本场景（issue #835）：最近一次账本清单聚合（列表命令内核同款）
    pub book_last_list: Option<tauri_app_lib::db::book_registry::BookListInfo>,
    /// 同步场景的 WebDAV 通道桩（配置通道时起；Drop 清理，issue #863）
    pub sync_stub: Option<tauri_app_lib::test_support::WebDavStub>,
    /// 同步自动轮次结果（`Ok(None)` = 零动作；`Err` = 静默失败路径，issue #863）
    pub sync_last_auto_round: Option<
        Result<Option<tauri_app_lib::sync_engine::SyncRoundReport>, tauri_app_lib::error::AppError>,
    >,
    /// 同步手动轮次报告（成功路径，issue #863）
    pub sync_last_report: Option<tauri_app_lib::sync_engine::SyncRoundReport>,
    /// 同步会话信封形态快照（密文会话场景断言用，issue #863）
    pub sync_session_encrypted: bool,
}

/// Cucumber World：每个 Scenario 独立持有一个 in-memory SQLite 数据库。
/// 本体只保留跨域共享的基础设施（连接、名称注册表、错误状态、冻结时钟），
/// 域内快照一律入快照分组（ADR-0086 决策 6）。
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
    /// 保司名称到 ID 的映射（Given/When 步骤创建保司后注册，issue #712）
    pub insurer_name_to_id: HashMap<String, String>,
    /// 分类名称到 ID 的映射（Given 步骤创建分类后注册，交易列表分类过滤场景用，issue #377）
    pub category_name_to_id: HashMap<String, String>,
    /// 最近一次操作错误（检查失败场景）
    pub last_error: Option<String>,
    /// 最近一次操作的码化错误（错误码断言用，issue #572）
    pub last_app_error: Option<tauri_app_lib::error::AppError>,
    /// 场景冻结的本地今日（滚动窗口步骤口径一致用，issue #182）
    pub frozen_today: Option<chrono::NaiveDate>,
    /// 交易组快照
    pub txn: TransactionGroup,
    /// 计划组快照
    pub plan: PlanGroup,
    /// 资产组快照（投资 + 实物资产）
    pub asset: AssetGroup,
    /// 物品组快照
    pub item: ItemGroup,
    /// 保单组快照
    pub policy: PolicyGroup,
    /// 报表组快照
    pub report: ReportGroup,
    /// 引导组快照（备份 + 数据位置 + 加密 + 启动失败 + 账本登记 + 同步）
    pub boot: BootGroup,
}

/// 启动处置接管结果（issue #601，启动失败恢复场景专用）：文件判定 + 建连的
/// 复合结果，与 `lib.rs::init_database` 消费 `classify_for_boot` 的序列同型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupTakeover {
    /// 明文库/空文件：正常建连进入应用。
    Opened,
    /// 真密文库：锁定等待解锁。
    AwaitUnlock,
    /// 启动失败：损坏残留或建连失败（前端失败恢复屏接管）。
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
            .field("backup_scope", &self.boot.backup_scope)
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
    fn new() -> Self {
        // 提交点后置动作接线（spec #1086 / issue #1088）：BDD world 自建库，与
        // 生产启动/测试工厂同形先注册备份域的提交点实现，置脏语义才成立（幂等）。
        tauri_app_lib::backup::install_after_commit_hook();
        // 写后即时同步接线（#1089）：与生产启动/测试工厂同形——op 产出单点在
        // 协议 crate，响应闭包由同步域提供，此处登记（幂等）。
        tauri_app_lib::sync_engine::trigger::install_after_write_hook();
        // 写路径副作用接缝接线（issue #1090）：与生产启动/测试工厂同形——余额
        // 刷新、计划来源解析与期次落账置脏的实现注册（幂等，先装者优先）。
        tauri_app_lib::accounts::balance::install_balance_refresh_hook();
        tauri_app_lib::scheduled_transactions::install_plan_source_hook();
        tauri_app_lib::backup::install_occurrence_dirty_hook();
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

    /// 获取保司 ID，按名称查找（issue #712）
    pub fn insurer_id(&self, name: &str) -> String {
        self.insurer_name_to_id
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("保司 '{}' 不存在", name))
    }

    /// 获取分类 ID，按名称查找（软删分类的名称→ID 映射刻意保留，历史引用语义）
    pub fn category_id(&self, name: &str) -> String {
        self.category_name_to_id
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("分类 '{}' 不存在", name))
    }
}
