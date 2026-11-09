//! 域接缝接线（issue #2017 / ADR-0112 决策 5：下层定义注册点、上层注册实现、
//! 壳层启动时接线）。
//!
//! 与旧壳启动接线（根包 `lib.rs` setup）同形的最小集：立桩读路径（交易列表
//! total 消费 `ledger-transaction` 读路径）触达的全部读时投影接缝——来源列反查
//! （计划 / 物品 / 保单 / 标的）、出资账户视图、转换两腿（六向组合）与计划来源
//! 解析；另接交易写入协议的余额刷新接缝（写入协议的组成部分，未注册即码化错误，
//! 立桩后续写路径与测试经域层公开写入口造数共同消费）。
//!
//! 刻意不接的接线与成因（宿主机制随宿主落地，骨架不启动宿主）：
//! - 备份置脏（`ledger-backup::install_after_commit_hook`）与追补触发——宿主是
//!   自动备份调度线程，骨架不拉起；
//! - 期次落账置脏对装——宿主是定时计划自动执行，骨架不执行计划。
//!
//! 写后即时同步钩子（`ledger_sync_engine::trigger::install_after_write_hook`）
//! 已随 #2080 接线（宿主是多端同步触发编排，宿主面随该票在 [`crate::sync_triggers`]
//! 落地）：本地 op 产出单点的响应闭包（去抖合流跑一轮）幂等装入，调度未拉起
//! 时投递零动作——写路径对同步域完全无感（与旧壳 setup 同一接线时序）。
//!
//! 后续写路径（如立桩票 spec #2015 的行删除扩展）接入时按需补装对应接缝。
//!
//! 幂等：各注册点进程级一次、重复注册保留首次实现（与旧壳 / 测试工厂同形），
//! 注册先于任何建库与读写。

/// 装入域栈接缝：六向组合 + 计划来源解析 + 余额刷新 + 写后即时同步响应闭包。
///
/// 幂等、先装者优先；调用先于任何建库/写库（[`crate::boot::boot_ledger`] 内
/// 单点调用，测试与生产同形）。
pub fn install_domain_seams() {
    // 交易域六向接缝（issue #1092 / #1180 组合安装的壳侧等价面：投资 6 钩子
    // ——计划装配/副作用/回退/释放 + 标的反查 + 转换两腿、商户名钩子组、本位币
    // 基准、来源列物品/保单反查、出资账户视图）。旧壳经根包
    // `transaction_wiring::install_all` 聚合；本壳独立接线，聚合的只是壳层接线
    // 动作（双壳并存期域接缝两侧同构，ADR-0145 代价条已接受）。
    ledger_investment::install_transaction_hooks();
    ledger_merchants::install_merchant_hooks();
    ledger_currencies::install_base_currency_hook();
    ledger_item::install_source_hook();
    ledger_policy::install_source_hook();
    ledger_accounts::install_funding_account_hook();
    // 计划来源解析（issue #1090）：来源列「计划反查」段的实现注册（读路径）。
    ledger_scheduled::install_plan_source_hook();
    // 余额刷新（issue #1090 挂载点②）：交易写入协议的余额缓存重算（写路径，
    // 未注册即码化错误——交易写入协议的组成部分，随域栈引导一并接线）。
    ledger_accounts::balance::install_balance_refresh_hook();
    // 写后即时同步的响应闭包（issue #2080 / ADR-0091 决策 9）：本地 op 产出
    // 单点（协议 crate record_local）的钩子槽装入域去抖合流入口——连续记账
    // 在 [WRITE_DEBOUNCE] 静默后合流成一轮。幂等（先装者优先）；调度未拉起
    // 时投递零动作（写路径对同步域无感），与旧壳 setup 同一接线时序。
    ledger_sync_engine::trigger::install_after_write_hook();
}
