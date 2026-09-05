//! 失效信号映射单点（ADR-0044，spec #311 / issue #330）：「写操作 → 失效信号」的
//! 唯一判定知识。
//!
//! 职责分界（ADR-0044 决策 7）：本模块承载**知识**（谁发什么）——强类型写操作身份
//! [`WriteOp`]、结果证据 [`WriteEvidence`]、信号 [`Signal`] 与纯函数 [`signals_for`]；
//! **机制**（怎么发）留在 `events.rs`（事件名常量 + 发射器接缝 `SignalEmitter` +
//! `EVENT_APP` 镜像句柄 + 主线程非阻塞投递，spec #364/#366），本模块经 [`emit_all`] /
//! [`emit_for`] 遍历信号集把事件名投递给发射器。
//!
//! - **键是强类型写操作身份**，不沿用命令名字符串（ADR-0044 决策 2）：HTTP handler
//!   无命令名（axum），且两壳命令面不对称，字符串键必然漂移；IPC 命令与 HTTP 端点
//!   写同一数据时共享同一 [`WriteOp`]（如账户删除命令与 `DELETE /api/v1/accounts/{id}`）。
//! - **映射闭集穷举**（ADR-0044 决策 3）：[`signals_for`] 对 [`WriteOp`] 穷尽 `match`
//!   （编译期防「enum 新增变体漏改映射」），「零信号」是显式登记行而非缺行——
//!   「不发」是决策（附动机注释），不是遗漏。
//! - **条件信号三类归一化**（ADR-0044 决策 4）：价格实际写入 / 黑洞即建 / 商户即建
//!   统一到 [`WriteEvidence`] 形状，映射表只保留一份「实际写入」判定
//!   （见 [`WriteEvidence::price_written`]），调用方把各自域内结果归一化为证据即可。
//! - **自动备份深路径**（ADR-0044 决策 5）：无命令身份、经 `events::EVENT_APP` 镜像
//!   句柄发射，登记为映射表特例条目 [`WriteOp::AutoBackupDeepPath`]，不做命令键——
//!   三个 `ledger:*` 信号的生产者清单由此单点可查。
//!
//! 旧机制已随 #335 收缩删除：`events::REFERENCE_WRITE_COMMANDS` /
//! `is_reference_write` / `emit_reference_changed` 不再存在，「谁发什么」的
//! 判定知识唯一载体是本模块。壳侧接线由源码扫描守门测试（`signals_cross_check`，
//! ADR-0073 决策 5）兜底：从两壳 `write_entry` 调用点扫描提取「声明壳, 身份」
//! 派生表（例外白名单登记不经入口的声明写命令）+ 反向守门（`db::write`/发射
//! 调用必经写入口）——「新写命令忘了声明身份」「绕开入口写库」均在测试期即红。
//! 手写声明表（IPC/HTTP 两张，约 190 行）已随 ADR-0073 消亡为扫描派生物。
//!
//! 写操作边界：本闭集收录「以写为意图」的操作（DB 行写入、KV / 指针文件写入、
//! 备份产物、进程级设置镜像推送）；纯读命令与控制类命令（`restart_app` /
//! `cancel_sync_instruments` / `open_log_dir`）不是写操作，不入集。

use crate::events;

/// 写操作身份（ADR-0044 决策 2）：跨 IPC 壳与 HTTP 壳共享的强类型键，闭集。
///
/// 变体按域分组；每个变体注释标明对应的 IPC 命令与/或 HTTP 端点，以及
/// 预期携带的结果证据（条件信号操作）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WriteOp {
    // ── 参考数据四表（ADR-0012）：写入成功发 `ledger:changed` ──
    /// 创建账户（IPC `create_account`；HTTP `POST /api/v1/accounts`）。
    CreateAccount,
    /// 编辑账户（IPC `update_account`；HTTP `PUT /api/v1/accounts/{id}`）。
    UpdateAccount,
    /// 删除账户（软删除；IPC `delete_account`；HTTP `DELETE /api/v1/accounts/{id}`）。
    DeleteAccount,
    /// 创建分类（IPC `create_category`；HTTP `POST /api/v1/categories`）。
    CreateCategory,
    /// 编辑分类（IPC `update_category`）。
    UpdateCategory,
    /// 分类重排（IPC `reorder_categories`）。
    ReorderCategories,
    /// 删除分类（软删除；IPC `delete_category`；HTTP `DELETE /api/v1/categories/{id}`）。
    DeleteCategory,
    /// 创建商户（第四张参考表，ADR-0028；IPC `create_merchant`）。
    CreateMerchant,
    /// 商户改名（IPC `update_merchant`）。
    UpdateMerchant,
    /// 删除商户（软删除；IPC `delete_merchant`）。
    DeleteMerchant,

    // ── 物品域（ADR-0014：独立领域，复用 `ledger:changed` 同名事件）──
    /// 创建物品（IPC `create_item`）。
    CreateItem,
    /// 编辑物品（IPC `update_item`）。
    UpdateItem,
    /// 处置物品（IPC `dispose_item`）。
    DisposeItem,
    /// 删除物品（IPC `delete_item`）。
    DeleteItem,

    // ── 保单域（ADR-0051：独立领域，复用 `ledger:changed` 同名事件）──
    /// 创建保单（IPC `create_policy`）。
    CreatePolicy,
    /// 编辑保单（IPC `update_policy`）。
    UpdatePolicy,
    /// 删除保单（软删除；IPC `delete_policy`）。
    DeletePolicy,

    // ── 实物资产域（ADR-0064：独立领域，复用 `ledger:changed` 同名事件）──
    /// 建档实物资产（IPC `create_physical_asset`；资产行 + 首条估值行同事务）。
    CreatePhysicalAsset,
    /// 编辑实物资产档案（IPC `update_physical_asset`，issue #467 T2）：
    /// 仅名称 / 购买信息，估值不经本入口变更。
    UpdatePhysicalAsset,
    /// 更新实物资产估值（IPC `update_physical_asset_valuation`，issue #467 T2）：
    /// 追加一条估值历史行（只追加不改写），当前估值变为最新一条。
    UpdatePhysicalAssetValuation,
    /// 处置实物资产（IPC `dispose_physical_asset`，issue #468 T3）：
    /// 状态标记转已处置 + 处置信息落库，退出默认列表与在持合计。
    DisposePhysicalAsset,
    /// 软删除实物资产（IPC `delete_physical_asset`，issue #468 T3）：
    /// `is_deleted=1`，数据与估值历史保留，退出列表与合计。
    DeletePhysicalAsset,

    // ── 账户域 ──
    /// 余额调整（IPC `adjust_account_balance`，ADR-0026）：预期证据
    /// [`WriteEvidence::BlackHoleCreated`]——仅按需新建黑洞账户时参考表变更。
    AdjustAccountBalance,

    // ── 价格域：条件信号 `ledger:prices-changed`（ADR-0031），证据
    //    [`WriteEvidence::PriceWritten`]，映射内共享一份「实际写入」判定 ──
    /// 增量同步持仓价格（IPC `sync_holding_prices`）：证据 = 实际写入 n>0。
    SyncHoldingPrices,
    /// 全量同步标的行情（IPC `sync_instruments`）：证据 = 本次运行有落库（含用户中断）。
    SyncInstruments,
    /// 按代码即拉场外基金（IPC `add_fund_by_code`，ADR-0038）：证据 = 落现价缓存。
    AddFundByCode,
    /// 手动报价（IPC `record_manual_price`，ADR-0036）：证据 = 实际写入任一落点。
    RecordManualPrice,
    /// 标的创建 / 幂等复用（IPC `create_instrument` 手动创建；HTTP
    /// `POST /api/v1/instruments` 含基金增强分支，ADR-0037/0039）：标的字典写入本身
    /// 不发参考信号；仅基金增强分支落现价时携 [`WriteEvidence::PriceWritten`] 发价格信号。
    CreateInstrument,
    /// 删除标的（IPC `delete_instrument`）：刻意零信号——无流水引用的标的无
    /// 持仓 / 走势消费方，前端标的列表本地重拉（issue #292 验收项）。
    DeleteInstrument,
    /// 半成品写价通道（IPC `create_market_price`）：刻意零信号（ADR-0044 决策 6）。
    /// 写 `market_prices` 现价缓存、被 `ledger:prices-changed` 定义覆盖，但属
    /// 「手动报价落地（#291）前」的半成品通道、前端零调用点——补广播只会制造
    /// 「发了但没人听」的假一致性。淘汰 / 合并路径：`record_manual_price` 已承载该信号。
    CreateMarketPrice,
    /// 当期汇率写入（IPC `create_exchange_rate`）：刻意零信号——写 `fx_rates`
    /// 当期表，不在 `ledger:prices-changed` 定义（MarketPrice / PriceHistory /
    /// FxRateHistory，ADR-0031）覆盖范围内。
    CreateExchangeRate,
    /// 余额缓存手动审计（IPC `audit_balance_cache`，issue #491 / ADR-0067）：
    /// 刻意零信号——修复的是派生缓存行，不置脏（ADR-0032 豁免形态）、
    /// 前端按返回的差异报告就地刷新，无需失效广播。
    AuditBalanceCache,
    /// 备注拼音一键修复（IPC `repair_note_pinyin`，issue #513）：刻意零信号——
    /// 回填的是搜索派生列（V018 `note_pinyin`），不置脏（ADR-0032 豁免形态）、
    /// 前端按返回的修复报告就地展示，无需失效广播。
    RepairNotePinyin,

    // ── 备份域：`ledger:backups-changed`（issue #129）──
    /// 手动备份（IPC `create_backup`）：刻意零信号——前端备份组合在命令成功后
    /// 自行刷新列表（受管路径随后触发的滚动清理经 [`WriteOp::PruneBackups`] 发信号），
    /// 后端再广播属重复通知；收编不改现状（spec：信号语义与触发条件零变化）。
    CreateBackup,
    /// 受管备份修剪（IPC `prune_backups`）：清理成功改变备份列表。
    PruneBackups,
    /// 从备份恢复（IPC `restore_backup`）：刻意零信号——恢复成功后前端随即调
    /// `restart_app` 整体重启，全部状态重新加载，失效信号无消费窗口。
    RestoreBackup,
    /// **特例条目，不做命令键**（ADR-0044 决策 5）：自动备份深路径执行点
    /// （连接层写入口提交点的写时顺带检查等，无命令身份）拿不到 `AppHandle`，
    /// 经 `events::EVENT_APP` 镜像句柄发射（`events::emit_backups_changed_current`）。
    /// 登记于此只为「备份信号生产者清单单点可查」，壳层不得以本变体调用
    /// [`signals_for`] 发射。
    AutoBackupDeepPath,

    // ── 交易域：基线零信号；唯一例外是「即建商户」证据（ADR-0028 / ADR-0044 决策 4，
    //    修复 HTTP 导入即建商户后的参考数据陈旧漏发，#331 接线）──
    /// 创建单笔交易（IPC `create_transaction`）：预期证据
    /// [`WriteEvidence::MerchantCreated`]（入参带 `merchant_name` 且未命中即建）。
    CreateTransaction,
    /// 批量创建交易（IPC `create_transactions`；HTTP `POST /api/v1/transactions/batch`）：
    /// 证据 = 批内聚合「任一行即建商户」。
    BatchCreateTransactions,
    /// 全字段替换交易（IPC `update_transaction`；HTTP `PUT /api/v1/transactions/{id}`）：
    /// 证据同 [`WriteOp::CreateTransaction`]。
    UpdateTransaction,
    /// 删除交易（软删除；IPC `delete_transaction`；HTTP `DELETE /api/v1/transactions/{id}`）：
    /// 零信号——交易类写入不触发参考失效。
    DeleteTransaction,
    /// 执行定时期次（IPC `execute_scheduled_occurrence`）：写入交易行（商户为计划
    /// 既有引用、不即建），零信号。
    ExecuteScheduledOccurrence,
    /// 回填定时期次（IPC `expand_scheduled_occurrences`）：写入期次计划行，零信号。
    ExpandScheduledOccurrences,

    // ── 预算域：刻意零信号（预算写不属参考 / 价格 / 备份任何一信号语义）──
    /// 创建预算（IPC `create_budget`）。
    CreateBudget,
    /// 修改预算额度（IPC `update_budget`）。
    UpdateBudget,
    /// 删除预算（IPC `delete_budget`）。
    DeleteBudget,

    // ── 定时计划域：刻意零信号（计划写入不触发参考失效；期次执行见交易域）──
    /// 创建定时计划（IPC `create_scheduled_transaction`）。
    CreateScheduledTransaction,
    /// 启停定时计划（IPC `update_scheduled_transaction_status`）。
    UpdateScheduledTransactionStatus,
    /// 编辑订阅计划续费字段（IPC `update_scheduled_subscription`）。
    UpdateScheduledSubscription,

    // ── 设置域：刻意零信号（设备偏好 / 引导配置，无 `ledger:*` 失效语义；
    //    设置页自读回显）──
    /// 自动备份开关（IPC `set_auto_backup_enabled`，写 `app_settings` KV，ADR-0017）。
    SetAutoBackupEnabled,
    /// 自动备份目录推送（IPC `set_auto_backup_dir`，前端 localStorage 权威的进程内
    /// 镜像，ADR-0016；首次兜底备份若触发，经 [`WriteOp::AutoBackupDeepPath`]
    /// 发信号，与本命令键无关）。
    SetAutoBackupDir,
    /// 定时计划自动执行开关推送（IPC `set_auto_execution_enabled`，进程级标志镜像）。
    SetAutoExecutionEnabled,
    /// 提交数据目录更改意图（IPC `submit_data_location_change`，写 ADR-0018 引导
    /// 指针文件）：重启后生效，失效信号无消费窗口。
    SubmitDataLocationChange,
    /// 恢复默认数据位置（IPC `restore_default_data_location`，同上写引导指针文件）。
    RestoreDefaultDataLocation,
}

impl WriteOp {
    /// 全部写操作身份（闭集清单）：信号守门测试（`signals_cross_check`，ADR-0044
    /// 决策 3 / ADR-0073 决策 5）按此遍历做「映射未声明」反向核对——除特例条目
    /// [`WriteOp::AutoBackupDeepPath`]（登记生产者清单、刻意不做命令键）外，每个身份
    /// 须被至少一壳声明（`write_entry` 调用点或例外白名单），否则测试期即红。
    ///
    /// **与 enum 本体同步维护**：新增变体漏登本清单时，反向核对对该变体失明——
    /// 清单紧邻 enum，同步义务就地可查（同 `TransactionKind::ALL` 先例）。
    /// 长度标注与初始化个数不符即编译错；但 enum 新增变体而本清单漏登不会报错，
    /// 改 enum 必须同步改这里。
    pub const ALL: [WriteOp; 54] = [
        // 参考数据四表
        WriteOp::CreateAccount,
        WriteOp::UpdateAccount,
        WriteOp::DeleteAccount,
        WriteOp::CreateCategory,
        WriteOp::UpdateCategory,
        WriteOp::ReorderCategories,
        WriteOp::DeleteCategory,
        WriteOp::CreateMerchant,
        WriteOp::UpdateMerchant,
        WriteOp::DeleteMerchant,
        // 物品域
        WriteOp::CreateItem,
        WriteOp::UpdateItem,
        WriteOp::DisposeItem,
        WriteOp::DeleteItem,
        // 保单域
        WriteOp::CreatePolicy,
        WriteOp::UpdatePolicy,
        WriteOp::DeletePolicy,
        // 实物资产域
        WriteOp::CreatePhysicalAsset,
        WriteOp::UpdatePhysicalAsset,
        WriteOp::UpdatePhysicalAssetValuation,
        WriteOp::DisposePhysicalAsset,
        WriteOp::DeletePhysicalAsset,
        // 账户域
        WriteOp::AdjustAccountBalance,
        WriteOp::AuditBalanceCache,
        // 价格域
        WriteOp::SyncHoldingPrices,
        WriteOp::SyncInstruments,
        WriteOp::AddFundByCode,
        WriteOp::RecordManualPrice,
        WriteOp::CreateInstrument,
        WriteOp::DeleteInstrument,
        WriteOp::CreateMarketPrice,
        WriteOp::CreateExchangeRate,
        // 备份域
        WriteOp::CreateBackup,
        WriteOp::PruneBackups,
        WriteOp::RestoreBackup,
        WriteOp::AutoBackupDeepPath,
        // 交易域（含搜索派生数据维护）
        WriteOp::CreateTransaction,
        WriteOp::RepairNotePinyin,
        WriteOp::BatchCreateTransactions,
        WriteOp::UpdateTransaction,
        WriteOp::DeleteTransaction,
        WriteOp::ExecuteScheduledOccurrence,
        WriteOp::ExpandScheduledOccurrences,
        // 预算域
        WriteOp::CreateBudget,
        WriteOp::UpdateBudget,
        WriteOp::DeleteBudget,
        // 定时计划域
        WriteOp::CreateScheduledTransaction,
        WriteOp::UpdateScheduledTransactionStatus,
        WriteOp::UpdateScheduledSubscription,
        // 设置域
        WriteOp::SetAutoBackupEnabled,
        WriteOp::SetAutoBackupDir,
        WriteOp::SetAutoExecutionEnabled,
        WriteOp::SubmitDataLocationChange,
        WriteOp::RestoreDefaultDataLocation,
    ];
}

/// 结果证据（ADR-0044 决策 1 / 决策 4）：写操作本次执行的**自然返回值**归一化，
/// 承载条件信号的「条件」一半。默认 [`WriteEvidence::None`]（无证据，静态行决定信号）；
/// 三类条件信号各占一个布尔变体，「真 / 假」由调用方按域内口径归一化（如
/// sync `written > 0`、基金增强 `price_written`、行为层「即建商户」），
/// 映射表内只保留一份「实际写入」判定（[`WriteEvidence::price_written`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteEvidence {
    /// 无证据（默认）：信号完全由写操作身份的静态映射行决定。
    None,
    /// 价格实际写入：增量 / 全量同步「落库 n>0（含用户中断保留的落库）」、
    /// 按代码即拉「落现价缓存」、手动报价「实际写入任一落点」的统一形状。
    PriceWritten(bool),
    /// 余额调整按需新建黑洞账户（参考表变更；纯转账零变化）。
    BlackHoleCreated(bool),
    /// 交易写「即建商户」（入参带 `merchant_name` 且未命中，写第四张参考表；
    /// 仅命中复用为零信号，ADR-0028）。
    MerchantCreated(bool),
}

impl WriteEvidence {
    /// 「实际写入」判定（映射内唯一一份，ADR-0044 决策 4）：价格证据为真。
    /// 其余证据形状（含 [`WriteEvidence::None`]）一律为否——证据错配保守降级为
    /// 零信号，不发错信号。
    fn price_written(&self) -> bool {
        matches!(self, WriteEvidence::PriceWritten(true))
    }

    /// 黑洞账户即建证据为真。
    fn black_hole_created(&self) -> bool {
        matches!(self, WriteEvidence::BlackHoleCreated(true))
    }

    /// 商户即建证据为真（映射判定与批量聚合共享这一份形状判定）。
    pub(crate) fn merchant_created(&self) -> bool {
        matches!(self, WriteEvidence::MerchantCreated(true))
    }
}

/// 失效信号（ADR-0044 决策 5）：三个 `ledger:*` 粗粒度信号的类型化形状，
/// 事件名常量与发射机制归 `events.rs`（本模块不做字符串）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// 参考数据失效（`ledger:changed`，ADR-0012）；物品域复用同名事件
    /// （独立域订阅者，ADR-0014）。
    LedgerChanged,
    /// 价格数据失效（`ledger:prices-changed`，ADR-0031）。
    PricesChanged,
    /// 备份产物失效（`ledger:backups-changed`，issue #129）。
    BackupsChanged,
}

/// 零信号集：刻意「不发」的显式登记行共享同一空切片。
const NO_SIGNALS: &[Signal] = &[];

/// 参考失效信号集（`ledger:changed`）。
const LEDGER_CHANGED_SET: &[Signal] = &[Signal::LedgerChanged];
/// 价格失效信号集（`ledger:prices-changed`）。
const PRICES_CHANGED_SET: &[Signal] = &[Signal::PricesChanged];
/// 备份失效信号集（`ledger:backups-changed`）。
const BACKUPS_CHANGED_SET: &[Signal] = &[Signal::BackupsChanged];

/// 条件信号行助手：条件成立返回给定信号集，否则零信号。
fn when(cond: bool, signals: &'static [Signal]) -> &'static [Signal] {
    if cond { signals } else { NO_SIGNALS }
}

/// 写操作 → 失效信号集的**唯一判定**（ADR-0044 决策 1）：给定写操作身份与结果
/// 证据，返回本次写成功后应发射的信号集。纯函数——无副作用、不依赖 `AppHandle`、
/// 不触库，可直接断言；穷尽 `match` 使「enum 新增变体漏改映射」在编译期即红。
///
/// 调用方约定：写事务**提交成功后**调用（信号是写后通知），并经 [`emit_for`] /
/// [`emit_all`] 发射；发射失败静默忽略，不影响写结果。零信号操作的调用点写
/// [`WriteEvidence::None`]，「不发」由此在映射行显式可查。
pub fn signals_for(op: WriteOp, evidence: WriteEvidence) -> &'static [Signal] {
    match op {
        // ── 参考数据四表（ADR-0012）：任一写入成功即发参考失效信号 ──
        WriteOp::CreateAccount
        | WriteOp::UpdateAccount
        | WriteOp::DeleteAccount
        | WriteOp::CreateCategory
        | WriteOp::UpdateCategory
        | WriteOp::ReorderCategories
        | WriteOp::DeleteCategory
        | WriteOp::CreateMerchant
        | WriteOp::UpdateMerchant
        | WriteOp::DeleteMerchant => LEDGER_CHANGED_SET,

        // ── 物品域（ADR-0014）：独立领域复用 ledger:changed 同名事件，
        //    物品 store 与参考 store 各自订阅、各自重拉 ──
        WriteOp::CreateItem | WriteOp::UpdateItem | WriteOp::DisposeItem | WriteOp::DeleteItem => {
            LEDGER_CHANGED_SET
        }

        // ── 保单域（ADR-0051）：独立领域复用 ledger:changed 同名事件，
        //    保单 store 自行订阅、自行重拉 ──
        WriteOp::CreatePolicy | WriteOp::UpdatePolicy | WriteOp::DeletePolicy => LEDGER_CHANGED_SET,

        // ── 实物资产域（ADR-0064）：独立领域复用 ledger:changed 同名事件，
        //    实物资产 store 自行订阅、自行重拉（编辑 / 更新估值 / 处置 / 软删同）──
        WriteOp::CreatePhysicalAsset
        | WriteOp::UpdatePhysicalAsset
        | WriteOp::UpdatePhysicalAssetValuation
        | WriteOp::DisposePhysicalAsset
        | WriteOp::DeletePhysicalAsset => LEDGER_CHANGED_SET,

        // ── 账户域：余额调整仅「按需新建黑洞账户」时参考表变更（ADR-0026）──
        WriteOp::AdjustAccountBalance => when(evidence.black_hole_created(), LEDGER_CHANGED_SET),
        // 余额缓存审计修复：派生数据自愈，不置脏不发信号（ADR-0067）。
        WriteOp::AuditBalanceCache => NO_SIGNALS,
        // 备注拼音一键修复：搜索派生列回填，不置脏不发信号（issue #513，同上豁免形态）。
        WriteOp::RepairNotePinyin => NO_SIGNALS,

        // ── 价格域：五操作共享同一行——映射内唯一一份「实际写入 → 发价格
        //    信号」判定（ADR-0044 决策 4）；零变化不广播（ADR-0031）──
        WriteOp::SyncHoldingPrices
        | WriteOp::SyncInstruments
        | WriteOp::AddFundByCode
        | WriteOp::RecordManualPrice
        | WriteOp::CreateInstrument => when(evidence.price_written(), PRICES_CHANGED_SET),

        // ── 刻意零信号：决策行，动机见各变体文档 ──
        // 半成品写价通道（ADR-0044 决策 6，淘汰路径 record_manual_price）。
        WriteOp::CreateMarketPrice => NO_SIGNALS,
        // 当期汇率表不在 ledger:prices-changed 定义覆盖内（ADR-0031）。
        WriteOp::CreateExchangeRate => NO_SIGNALS,
        // 无流水引用的标的无消费方，前端本地重拉（issue #292）。
        WriteOp::DeleteInstrument => NO_SIGNALS,

        // ── 备份域（issue #129）──
        // 前端备份组合命令成功后自刷新；后端再广播属重复通知，收编不改现状。
        WriteOp::CreateBackup => NO_SIGNALS,
        // 受管修剪改变备份列表。
        WriteOp::PruneBackups => BACKUPS_CHANGED_SET,
        // 恢复成功后整体重启，失效信号无消费窗口。
        WriteOp::RestoreBackup => NO_SIGNALS,
        // 特例条目（ADR-0044 决策 5）：登记使生产者清单单点可查；实际发射走
        // events::emit_backups_changed_current（EVENT_APP 镜像句柄），不经壳层。
        WriteOp::AutoBackupDeepPath => BACKUPS_CHANGED_SET,

        // ── 交易域：基线零信号；唯一例外「即建商户」（写第四张参考表）。
        //    证据由行为层入口外传、两壳据此发射（#331 接线）──
        WriteOp::CreateTransaction
        | WriteOp::BatchCreateTransactions
        | WriteOp::UpdateTransaction => when(evidence.merchant_created(), LEDGER_CHANGED_SET),
        // 删除不建商户；期次执行 / 回填只写交易行与期次计划（商户为既有引用）。
        WriteOp::DeleteTransaction
        | WriteOp::ExecuteScheduledOccurrence
        | WriteOp::ExpandScheduledOccurrences => NO_SIGNALS,

        // ── 预算域：刻意零信号（不属参考 / 价格 / 备份任何信号语义）──
        WriteOp::CreateBudget | WriteOp::UpdateBudget | WriteOp::DeleteBudget => NO_SIGNALS,

        // ── 定时计划域：刻意零信号（期次执行产生的交易行走交易域行）──
        WriteOp::CreateScheduledTransaction
        | WriteOp::UpdateScheduledTransactionStatus
        | WriteOp::UpdateScheduledSubscription => NO_SIGNALS,

        // ── 设置域：刻意零信号（设备偏好 / 引导配置，无 ledger:* 失效语义；
        //    设置页自读回显）──
        WriteOp::SetAutoBackupEnabled
        | WriteOp::SetAutoBackupDir
        | WriteOp::SetAutoExecutionEnabled
        | WriteOp::SubmitDataLocationChange
        | WriteOp::RestoreDefaultDataLocation => NO_SIGNALS,
    }
}

/// 发射助手（ADR-0044 决策 1，机制侧收口）：遍历信号集逐个把事件名投递给发射器。
/// 发射器即机制接缝（`events::SignalEmitter`，spec #366）：生产路径传 `&AppHandle`
/// （主线程非阻塞投递，ADR-0054，trait 自动转型），测试注入闸门式假发射器
///（`test_utils::GatedEmitter`）断言「发射不阻塞写路径」（`emit_blocking_tests`，
/// spec #366）。本函数只做一次非阻塞
/// 投递即返回，不等发射完成；投递 / 发射失败静默忽略，不影响写事务结果。
/// 壳层约定形态：写路径经统一写入口 `write_entry`（ADR-0073）内化发射；
/// 本函数供不经入口的例外路径（备份修剪、全量同步自发射）与写入口本体消费，
/// 或先取 [`signals_for`] 再 [`emit_all`]（需要先记日志 / 断言信号集时用后者）。
pub fn emit_all(emitter: &dyn events::SignalEmitter, signals: &[Signal]) {
    for signal in signals {
        match signal {
            Signal::LedgerChanged => emitter.post(events::LEDGER_CHANGED),
            Signal::PricesChanged => emitter.post(events::PRICES_CHANGED),
            Signal::BackupsChanged => emitter.post(events::BACKUPS_CHANGED),
        }
    }
}

/// 组合助手：取 [`signals_for`] 判定并立即发射（写入口与例外路径的单行形态）。
pub fn emit_for(emitter: &dyn events::SignalEmitter, op: WriteOp, evidence: WriteEvidence) {
    emit_all(emitter, signals_for(op, evidence));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 断言信号集恰为期望值（含零信号显式断言：期望空切片）。
    fn assert_signals(actual: &[Signal], expected: &[Signal]) {
        assert_eq!(actual, expected);
    }

    use WriteEvidence as E;
    use WriteOp as Op;

    // ── 参考数据四表（ADR-0012）：一律 ledger:changed ──

    #[test]
    fn create_account_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::CreateAccount, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn update_account_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::UpdateAccount, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn delete_account_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::DeleteAccount, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn create_category_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::CreateCategory, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn update_category_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::UpdateCategory, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn reorder_categories_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::ReorderCategories, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn delete_category_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::DeleteCategory, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn create_merchant_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::CreateMerchant, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn update_merchant_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::UpdateMerchant, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn delete_merchant_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::DeleteMerchant, E::None),
            &[Signal::LedgerChanged],
        );
    }

    // ── 物品域（ADR-0014 复用 ledger:changed）──

    #[test]
    fn create_item_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::CreateItem, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn update_item_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::UpdateItem, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn dispose_item_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::DisposeItem, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn delete_item_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::DeleteItem, E::None),
            &[Signal::LedgerChanged],
        );
    }

    // ── 保单域（ADR-0051）：独立领域一律 ledger:changed ──

    #[test]
    fn create_policy_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::CreatePolicy, E::None),
            &[Signal::LedgerChanged],
        );
    }

    // ── 实物资产域（ADR-0064）：独立领域一律 ledger:changed ──

    #[test]
    fn create_physical_asset_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::CreatePhysicalAsset, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn update_physical_asset_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::UpdatePhysicalAsset, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn update_physical_asset_valuation_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::UpdatePhysicalAssetValuation, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn dispose_physical_asset_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::DisposePhysicalAsset, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn delete_physical_asset_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::DeletePhysicalAsset, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn update_policy_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::UpdatePolicy, E::None),
            &[Signal::LedgerChanged],
        );
    }

    #[test]
    fn delete_policy_emits_ledger_changed() {
        assert_signals(
            signals_for(Op::DeletePolicy, E::None),
            &[Signal::LedgerChanged],
        );
    }

    // ── 余额调整：仅黑洞即建发参考信号 ──

    #[test]
    fn adjust_balance_emits_ledger_changed_only_when_black_hole_created() {
        // 按需新建黑洞账户 = 参考表变更 → 发。
        assert_signals(
            signals_for(Op::AdjustAccountBalance, E::BlackHoleCreated(true)),
            &[Signal::LedgerChanged],
        );
        // 纯转账（黑洞已存在）零信号。
        assert_signals(
            signals_for(Op::AdjustAccountBalance, E::BlackHoleCreated(false)),
            &[],
        );
    }

    // ── 价格域：共享一份「实际写入」判定 ──

    #[test]
    fn sync_holding_prices_emits_prices_changed_only_when_written() {
        assert_signals(
            signals_for(Op::SyncHoldingPrices, E::PriceWritten(true)),
            &[Signal::PricesChanged],
        );
        // 零变化不广播（无持仓 / 全部跳过 / 基金全部「已是最新」）。
        assert_signals(
            signals_for(Op::SyncHoldingPrices, E::PriceWritten(false)),
            &[],
        );
    }

    #[test]
    fn sync_instruments_emits_prices_changed_only_when_written() {
        // 全量同步终态含用户中断（中断保留已落库价格，证据归一化为「有落库」）。
        assert_signals(
            signals_for(Op::SyncInstruments, E::PriceWritten(true)),
            &[Signal::PricesChanged],
        );
        assert_signals(
            signals_for(Op::SyncInstruments, E::PriceWritten(false)),
            &[],
        );
    }

    #[test]
    fn add_fund_by_code_emits_prices_changed_only_when_price_written() {
        // 落现价即广播；未取到净值仅建标的、不广播（ADR-0038）。
        assert_signals(
            signals_for(Op::AddFundByCode, E::PriceWritten(true)),
            &[Signal::PricesChanged],
        );
        assert_signals(signals_for(Op::AddFundByCode, E::PriceWritten(false)), &[]);
    }

    #[test]
    fn record_manual_price_emits_prices_changed_only_when_any_point_written() {
        // 实际写入任一落点（现价缓存 / 价格历史）即广播（ADR-0036）。
        assert_signals(
            signals_for(Op::RecordManualPrice, E::PriceWritten(true)),
            &[Signal::PricesChanged],
        );
        // 回填早于最新价格点且现价未动：零变化不广播。
        assert_signals(
            signals_for(Op::RecordManualPrice, E::PriceWritten(false)),
            &[],
        );
    }

    #[test]
    fn create_instrument_emits_prices_changed_only_when_nav_persisted() {
        // HTTP 基金增强分支落现价 → 发（ADR-0039）。
        assert_signals(
            signals_for(Op::CreateInstrument, E::PriceWritten(true)),
            &[Signal::PricesChanged],
        );
        // 通用创建 / 降级创建 / IPC 手动创建：标的字典写入本身零信号。
        assert_signals(
            signals_for(Op::CreateInstrument, E::PriceWritten(false)),
            &[],
        );
        assert_signals(signals_for(Op::CreateInstrument, E::None), &[]);
    }

    // ── 刻意零信号：显式登记（附动机见变体文档）──

    #[test]
    fn create_market_price_is_deliberately_silent() {
        // 半成品写价通道（ADR-0044 决策 6）：即便误携价格证据也不发。
        assert_signals(signals_for(Op::CreateMarketPrice, E::None), &[]);
        assert_signals(
            signals_for(Op::CreateMarketPrice, E::PriceWritten(true)),
            &[],
        );
    }

    #[test]
    fn create_exchange_rate_is_deliberately_silent() {
        // 当期汇率表不在 ledger:prices-changed 定义覆盖内（ADR-0031）。
        assert_signals(signals_for(Op::CreateExchangeRate, E::None), &[]);
    }

    #[test]
    fn delete_instrument_is_deliberately_silent() {
        // 无流水引用的标的无消费方，前端本地重拉（issue #292）。
        assert_signals(signals_for(Op::DeleteInstrument, E::None), &[]);
    }

    // ── 备份域 ──

    #[test]
    fn create_backup_is_deliberately_silent() {
        // 前端备份组合命令成功后自刷新；收编不改现状。
        assert_signals(signals_for(Op::CreateBackup, E::None), &[]);
    }

    #[test]
    fn prune_backups_emits_backups_changed() {
        assert_signals(
            signals_for(Op::PruneBackups, E::None),
            &[Signal::BackupsChanged],
        );
    }

    #[test]
    fn restore_backup_is_deliberately_silent() {
        // 恢复成功后整体重启，失效信号无消费窗口。
        assert_signals(signals_for(Op::RestoreBackup, E::None), &[]);
    }

    #[test]
    fn auto_backup_deep_path_is_registered_as_special_entry() {
        // 特例条目（ADR-0044 决策 5）：登记使生产者清单单点可查；
        // 发射走 events::emit_backups_changed_current，不经壳层 signals_for。
        assert_signals(
            signals_for(Op::AutoBackupDeepPath, E::None),
            &[Signal::BackupsChanged],
        );
    }

    // ── 交易域：基线零信号 + 商户即建例外 ──

    #[test]
    fn create_transaction_emits_ledger_changed_only_when_merchant_created() {
        // 即建商户（写第四张参考表）→ 发（修复 HTTP 导入漏发，#331 接线）。
        assert_signals(
            signals_for(Op::CreateTransaction, E::MerchantCreated(true)),
            &[Signal::LedgerChanged],
        );
        // 仅命中复用（名字命中或带 merchant_id）：零信号（不播无谓重拉）。
        assert_signals(
            signals_for(Op::CreateTransaction, E::MerchantCreated(false)),
            &[],
        );
        assert_signals(signals_for(Op::CreateTransaction, E::None), &[]);
    }

    #[test]
    fn batch_create_transactions_emits_ledger_changed_only_when_any_merchant_created() {
        // 批内聚合「任一行即建」→ 发。
        assert_signals(
            signals_for(Op::BatchCreateTransactions, E::MerchantCreated(true)),
            &[Signal::LedgerChanged],
        );
        assert_signals(
            signals_for(Op::BatchCreateTransactions, E::MerchantCreated(false)),
            &[],
        );
    }

    #[test]
    fn update_transaction_emits_ledger_changed_only_when_merchant_created() {
        assert_signals(
            signals_for(Op::UpdateTransaction, E::MerchantCreated(true)),
            &[Signal::LedgerChanged],
        );
        assert_signals(
            signals_for(Op::UpdateTransaction, E::MerchantCreated(false)),
            &[],
        );
    }

    #[test]
    fn delete_transaction_is_silent() {
        assert_signals(signals_for(Op::DeleteTransaction, E::None), &[]);
    }

    #[test]
    fn execute_scheduled_occurrence_is_silent() {
        assert_signals(signals_for(Op::ExecuteScheduledOccurrence, E::None), &[]);
    }

    #[test]
    fn expand_scheduled_occurrences_is_silent() {
        assert_signals(signals_for(Op::ExpandScheduledOccurrences, E::None), &[]);
    }

    // ── 预算域：刻意零信号 ──

    #[test]
    fn create_budget_is_silent() {
        assert_signals(signals_for(Op::CreateBudget, E::None), &[]);
    }

    #[test]
    fn update_budget_is_silent() {
        assert_signals(signals_for(Op::UpdateBudget, E::None), &[]);
    }

    #[test]
    fn delete_budget_is_silent() {
        assert_signals(signals_for(Op::DeleteBudget, E::None), &[]);
    }

    // ── 定时计划域：刻意零信号 ──

    #[test]
    fn create_scheduled_transaction_is_silent() {
        assert_signals(signals_for(Op::CreateScheduledTransaction, E::None), &[]);
    }

    #[test]
    fn update_scheduled_transaction_status_is_silent() {
        assert_signals(
            signals_for(Op::UpdateScheduledTransactionStatus, E::None),
            &[],
        );
    }

    #[test]
    fn update_scheduled_subscription_is_silent() {
        assert_signals(signals_for(Op::UpdateScheduledSubscription, E::None), &[]);
    }

    // ── 设置域：刻意零信号 ──

    #[test]
    fn set_auto_backup_enabled_is_silent() {
        assert_signals(signals_for(Op::SetAutoBackupEnabled, E::None), &[]);
    }

    #[test]
    fn set_auto_backup_dir_is_silent() {
        // 目录镜像推送本身零信号；首次兜底备份经 AutoBackupDeepPath 发射。
        assert_signals(signals_for(Op::SetAutoBackupDir, E::None), &[]);
    }

    #[test]
    fn set_auto_execution_enabled_is_silent() {
        assert_signals(signals_for(Op::SetAutoExecutionEnabled, E::None), &[]);
    }

    #[test]
    fn submit_data_location_change_is_silent() {
        // 引导指针文件写入：重启后生效，失效信号无消费窗口。
        assert_signals(signals_for(Op::SubmitDataLocationChange, E::None), &[]);
    }

    #[test]
    fn restore_default_data_location_is_silent() {
        assert_signals(signals_for(Op::RestoreDefaultDataLocation, E::None), &[]);
    }

    // ── 证据形状 ──

    #[test]
    fn mismatched_or_missing_evidence_degrades_to_zero_signal() {
        // 证据错配保守降级（经公共接缝断言，不发错信号）：
        // 条件行拿到非本域证据或无证据，一律零信号。
        let price_ops = [
            Op::SyncHoldingPrices,
            Op::SyncInstruments,
            Op::AddFundByCode,
            Op::RecordManualPrice,
            Op::CreateInstrument,
        ];
        for op in price_ops {
            assert_signals(signals_for(op, E::None), &[]);
            assert_signals(signals_for(op, E::MerchantCreated(true)), &[]);
            assert_signals(signals_for(op, E::BlackHoleCreated(true)), &[]);
        }
        assert_signals(
            signals_for(Op::AdjustAccountBalance, E::PriceWritten(true)),
            &[],
        );
        assert_signals(
            signals_for(Op::CreateTransaction, E::PriceWritten(true)),
            &[],
        );
    }
}

/// 「发射不阻塞写路径」回归测试（spec #366）：机制层注入闸门式假发射器
///（`test_utils::GatedEmitter`，与 HTTP 壳整链验证 spec #367 共用同一桩），
/// 钉死 ADR-0054 的外部行为——发射器阻塞期间写路径仍及时返回，放行后信号
/// 最终到达。与上方映射测试（知识层，谁发什么）并存，各守一个维度。
#[cfg(test)]
mod emit_blocking_tests;
