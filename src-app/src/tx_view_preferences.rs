//! 交易页视图偏好（issue #2062 / ADR-0136 偏好与筛选分家）：设备级持久行集
//! 裁决的唯一读写方（旧壳 `transaction-view-preferences` store 的 NSH 重生）。
//!
//! 语义：跨启动本地持久（`nsh_core::storage::Storage` 平台文件侧 KV，键名同
//! 旧壳 view_state 家族、值 JSON 布尔字面量，**不与旧壳共享存储**——旧壳
//! WebView localStorage、本壳平台文件侧；不入账本 SQLite、不进备份、不参与
//! 多端同步）；默认关；脏值（缺失/非 `"true"` 字面量）整体回默认关（旧壳
//! `parseHideInvestmentRelated` 同口径）。与交易筛选会话态分家（ADR-0136
//! 决策 1）：偏好是持久裁决不是 TransactionFilter 维度，不入会话筛选
//! （[`crate::tx_filter`]）、不参与清除筛选与 ESC 复位；行集 = 偏好收窄 ∩
//! 既有筛选，下钻不让位（读路径透传面在 `boot::TxListQuery` → 域
//! `TransactionListFilter::hide_investment_related`）。
//!
//! 反应式外壳：进程级信号单例（开关受控态；存储是领域真源，信号是本会话
//! 镜像——写路径单点先落盘再落信号）。

use std::sync::Mutex;

use nsh_core::prelude::*;

/// 存储键（旧壳 view_state 家族同名键：`packages/utils/view-state.ts` 的
/// `view_state:hide_investment_related`；值为 JSON 布尔字面量 `"true"`——
/// 开启持久化、关闭删记录，默认态 = 无记录）。
const HIDE_INVESTMENT_RELATED_KEY: &str = "view_state:hide_investment_related";

/// 偏好设备态读（引擎线程内同步读；解析镜像旧壳 `parseHideInvestmentRelated`：
/// 仅字面量 `"true"` 为开——缺失/脏值整体回默认关，不产生非法状态）。
fn hide_investment_related() -> bool {
    nsh_core::storage::Storage::get_item(HIDE_INVESTMENT_RELATED_KEY)
        .is_some_and(|raw| raw.trim() == "true")
}

/// 进程级信号槽（headless 多场景串行复位 [`reset_for_tests`]；生产进程一次
/// 初始化）。锁毒化时取回内值——槽位本身不含不变量（[`crate::tx_filter`] 同形）。
static HIDE_INVESTMENT_RELATED: Mutex<Option<Signal<bool>>> = Mutex::new(None);

fn slot() -> std::sync::MutexGuard<'static, Option<Signal<bool>>> {
    HIDE_INVESTMENT_RELATED
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

/// 偏好信号（惰性初始化：首次消费从设备存储读初值；信号挂首次调用所在组件
/// 实例——生产路径契约：`App` 根组件体最先调用，状态挂根实例，随进程存活、
/// 视图卸载不丢；headless 测试经 `reset_for_tests` 复位后由下一场景重挂）。
#[doc(hidden)]
pub fn store() -> Signal<bool> {
    let mut slot = slot();
    if let Some(s) = *slot {
        return s;
    }
    let s = signal(hide_investment_related());
    *slot = Some(s);
    s
}

/// 偏好写路径（点选即写；旧壳 `setHideInvestmentRelated` 重生）：开启持久化
/// （`"true"` 字面量）、关闭删记录（默认态 = 无记录）；同值守卫——目标态与
/// 当前态相同时不动作、不产生存储写入。写失败不静默：告警日志 + 本会话信号
/// 仍生效（下次启动回默认——等待成功优于即刻失败，nav 写路径同纪律）。
pub fn set(value: bool) {
    let s = store();
    if value == s.get_untracked() {
        return;
    }
    if value {
        if let Err(e) = nsh_core::storage::Storage::set_item(HIDE_INVESTMENT_RELATED_KEY, "true") {
            tracing::warn!(key = HIDE_INVESTMENT_RELATED_KEY, error = %e, "视图偏好写入存储失败");
        }
    } else {
        nsh_core::storage::Storage::remove_item(HIDE_INVESTMENT_RELATED_KEY);
    }
    s.set(value);
}

/// 复位偏好槽（headless 测试接缝，[`crate::app::reset_book_assembly`] 消费）：
/// 下一场景从（新）设备存储重读初值，本会话镜像不跨场景残留。
#[doc(hidden)]
pub fn reset_for_tests() {
    *slot() = None;
}
