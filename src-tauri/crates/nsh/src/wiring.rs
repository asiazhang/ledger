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
//! - 写后即时同步（`ledger-sync-engine` trigger）——宿主是多端同步编排，骨架
//!   不做同步；
//! - 期次落账置脏对装——宿主是定时计划自动执行，骨架不执行计划。
//!
//! 交易列表立桩票（spec #2015 后续票）接入写路径（行删除）时按需补装。
//!
//! 幂等：各注册点进程级一次、重复注册保留首次实现（与旧壳 / 测试工厂同形），
//! 注册先于任何建库与读写。

/// 装入域栈接缝：六向组合 + 计划来源解析 + 余额刷新。
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
}
