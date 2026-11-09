//! 域接缝接线（issue #2017 / ADR-0112 决策 5：下层定义注册点、上层注册实现、
//! 壳层启动时接线）。
//!
//! 与旧壳启动接线（根包 `lib.rs` setup）同形：立桩读路径（交易列表
//! total 消费 `ledger-transaction` 读路径）触达的全部读时投影接缝——来源列反查
//! （计划 / 物品 / 保单 / 标的）、出资账户视图、转换两腿（六向组合）与计划来源
//! 解析；交易写入协议的余额刷新接缝（写入协议的组成部分，未注册即码化错误）；
//! #2078 起兼备份域三接缝与持锁预算登记（置脏触发①、期次落账置脏对装、追补
//! 触发对装④——宿主（自动备份调度线程）已随宿主票在 NSH 落地，接线不再
//! 随宿主缺位而豁免，注册先于任何建库与写库，与旧壳同序）；#2087 起置脏
//! 触发①由 boot 写函数族经壳层统一写入口直锁形态
//! （[`crate::shell_support::write_entry_locked`]）真实到达（旧壳写命令经门面
//! 写作业到达同一提交点，本壳写路径在调用线程直锁执行，提交点单点同源）。
//!
//! 写后即时同步钩子（`ledger_sync_engine::trigger::install_after_write_hook`）
//! 已随 #2080 接线（宿主是多端同步触发编排，宿主面随该票在 [`crate::sync_triggers`]
//! 落地）：本地 op 产出单点的响应闭包（去抖合流跑一轮）幂等装入，调度未拉起
//! 时投递零动作——写路径对同步域完全无感（与旧壳 setup 同一接线时序）。
//!
//! 后续写路径（如立桩票 spec #2015 的行删除扩展）接入时按需补装对应接缝。
//!
//! 幂等：各注册点进程级一次、重复注册保留首次实现（与旧壳 / 测试工厂同形），
//! 注册先于任何建库与读写。持锁预算登记例外：登记表是 Vec 追加、重复调用累
//! 加条目（探针取最宽预算，重复同值无行为差），与旧壳登记只在启动发生一次
//! 的同款口径。

/// 装入域栈接缝：六向组合 + 计划来源解析 + 余额刷新 + 写后即时同步响应闭包
/// + 备份域三接缝。
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
    // 写路径副作用接缝接线（spec #1086 / issue #1088，挂载点①；#2078 起随宿主
    // 落地接线，#2087 起由写函数族经写入口真实触发）：连接层写入口提交点的
    // 后置动作（置脏 + 写时顺带到期检查，ADR-0032）由备份域提供实现、壳层
    // 启动时注册——基础设施 crate 不再反向依赖业务域。
    ledger_backup::install_after_commit_hook();
    // 期次落账置脏对装（issue #1090）：实现由备份域提供、定时计划域注册点承接
    // ——定时计划域对备份域零直接依赖。追补落账实际执行时置脏（宿主随 #2078
    // 落地，追补判定随调度轮运行；开关镜像默认关时空转）。
    ledger_scheduled::auto_run::register_after_occurrence_hook(
        ledger_backup::occurrence_dirty_hook,
    );
    // 追补触发对装（issue #1091 / 挂载点④，ADR-0112 决策 5）：自动备份调度
    // 线程单一 tick 的追补判定实现由定时计划域提供（开关镜像 + 本地今天在
    // 实现内注入）、壳层启动时注册进备份域的注册点——备份域对定时计划域零依赖。
    // 镜像默认关（NSH 现无自动执行开关推送面）：未推送即空转，与旧壳同口径。
    ledger_backup::register_catch_up_hook(ledger_scheduled::auto_run::catch_up_hook);
    // 备份作业类的持锁预算登记（issue #1765，ADR-0112 决策 5 反转）：探针注册点
    // 在基础设施、预算值由备份域提供（[`ledger_backup::LOCK_HOLD_BUDGET`]）。
    // 登记面 = 门面作业命令名前缀；NSH 门面作业消费面 = 本票调度的 `backup.`
    // 前缀三作业（调度轮 / 退出兜底 / 首次兜底）——旧壳其余命令名预算（手动
    // 备份族命令）在 NSH 无门面作业消费方（直调取锁不经探针），不预登记死配置。
    ledger_infra::db::register_lock_hold_budget("backup.", ledger_backup::LOCK_HOLD_BUDGET);
    // 写后即时同步的响应闭包（issue #2080 / ADR-0091 决策 9）：本地 op 产出
    // 单点（协议 crate record_local）的钩子槽装入域去抖合流入口——连续记账
    // 在 [WRITE_DEBOUNCE] 静默后合流成一轮。幂等（先装者优先）；调度未拉起
    // 时投递零动作（写路径对同步域无感），与旧壳 setup 同一接线时序。
    ledger_sync_engine::trigger::install_after_write_hook();
}
