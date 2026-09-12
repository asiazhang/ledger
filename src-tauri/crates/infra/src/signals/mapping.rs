//! 写操作身份 → 失效信号的唯一映射（ADR-0044 决策 1 / 决策 3）：信号类型
//! [`Signal`]、静态信号集常量、条件行助手与穷尽 `match` 的 [`signals_for`]。
//!
use super::evidence::WriteEvidence;
use super::write_op::WriteOp;

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
/// 调用方约定：写事务**提交成功后**调用（信号是写后通知），并经
/// [`crate::signals::emit_for`] / [`crate::signals::emit_all`] 发射；发射失败静默忽略，
/// 不影响写结果。零信号操作的调用点写
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
        | WriteOp::DeleteMerchant
        | WriteOp::CreateInsurer
        | WriteOp::UpdateInsurer
        | WriteOp::DeleteInsurer => LEDGER_CHANGED_SET,

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

        // ── 价格域：四操作共享同一行——映射内唯一一份「实际写入 → 发价格
        //    信号」判定（ADR-0044 决策 4）；零变化不广播（ADR-0031）──
        WriteOp::SyncInstrumentInfo
        | WriteOp::AddFundByCode
        | WriteOp::AddInstrumentByCode
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
        | WriteOp::RestoreDefaultDataLocation
        | WriteOp::SetLogLevel
        | WriteOp::SetBaseCurrency
        | WriteOp::SetSyncChannelConfig => NO_SIGNALS,

        // ── 多端同步域（ADR-0091）：本轮实际应用外来 op 才广播参考失效 ──
        WriteOp::SyncRound => when(evidence.ledger_applied(), LEDGER_CHANGED_SET),
    }
}
