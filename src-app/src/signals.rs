//! 进程内失效信号（issue #2022；词汇表「失效信号（Invalidation Signal）」语义的
//! NSH 壳内信号通道重建）。
//!
//! 语义沿词汇表裁决：**粗粒度、无 payload**——只表达「交易数据变了」，不携带
//! 数据；零写入不广播（失效信号的本义是数据确实发生了变化）；消费方订阅后自行
//! 重拉。与旧壳的投递差异只在机制：旧壳经 Tauri 事件跨进程边界，NSH 是壳内
//! 进程内直调（spec #2015「写路径证明」：域公开写入口直调 → 进程内失效信号
//! 驱动列表重读），不需要 IPC 事件。
//!
//! 线程契约：订阅与广播都发生在引擎的 UI 线程（订阅者持有信号句柄，信号与
//! 视图层同线程；写路径的重活经 `blocking` 在后台线程执行，完成后回到 UI 线程
//! 才广播）——订阅者表因此住 thread_local，不做跨线程投递。唯一例外是备份族
//! 的深路径发射（#2078）：发射点在任意写线程（调度线程 / 连接层写线程），经
//! 进程级 pending 计数（[`BACKUPS_PENDING`]）解耦后由 UI 线程泵换广播——
//! 机制差异留痕在此，语义（粗粒度、无 payload）不变。
//!
//! 接线形态（ADR-0112 决策 5 的壳内等价形）：本模块只定义注册点（订阅 /
//! 广播单点），重读实现由视图（[`crate::transactions::TransactionsView`]）注册，写路
//! 径完成点（删除成功）广播——下层不直调上层，不为消除合法依赖引入端口反转。
//!
//! 失效模式反转（ADR-0035 同哲学）：漏广播的症状是「库中已删而列表仍旧」——
//! 用户可见（headless 删除测试钉住：删除本模块的广播接线即红，见
//! tests/app_headless.rs 的「删除即红」负向条目），不再是静默永久失效。
//!
//! 单点内核（#2073）：交易 / 预算 / 投资 / 备份四族的订阅器 / 广播器 / 句柄
//! 曾逐族复制（每族一个 thread_local 表 + 句柄 struct + 订阅 fn + 广播 fn），
//! 现收拢为一份共享内核——一张订阅者表、族身份是表内标记（[`Family`]）、句柄
//! 单型（[`SignalSubscription`]，id 进程级唯一故撤销无需族信息）。跨族不串扰
//! 语义由广播按族过滤承担，公开名面（各 `subscribe_xxx` / `notify_xxx` / 句
//! 柄类型别名）不变。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::{
    cell::RefCell,
    rc::Rc,
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
};

/// 信号族（订阅者表内的族标记）：每族一份公开名面（订阅 fn / 广播 fn / 句柄
/// 别名），族间不串扰由广播过滤保证——测试钉住（跨族广播不触发他族订阅者）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    /// 交易数据失效（#2022，本模块立桩族）。
    Transactions,
    /// 预算数据失效（#2033，写路径成功后广播，预算视图订阅重读）。
    Budgets,
    /// 投资数据失效（#2039；广播接线随投资写路径迁移票落地——NSH 现无投资写
    /// 入口，注册点先立，消费面先行）。
    Investments,
    /// 备份产物失效（#2078；自动备份完成 / 受管备份清理等改变备份列表的动作
    /// 成功后广播，消费面 = 设置卡备份列表与自动备份状态重读）。
    Backups,
}

/// 订阅者表条目：订阅 id（进程级唯一）+ 族标记 + 处理器。
type Subscriber = (u64, Family, Rc<dyn Fn()>);

// 全族统一的订阅者表（UI 线程持有；订阅序无关：广播时逐个调用，顺序不构成
// 语义）。RefCell 防再入借用：广播先克隆出本族处理器再逐个调用，广播路径上
// 的再入订阅/撤销不与借用互斥。
thread_local! {
    static SUBSCRIBERS: RefCell<Vec<Subscriber>> = const { RefCell::new(Vec::new()) };
}

/// 备份产物变更的进程级 pending 计数（#2078 跨线程桥的落点）：深路径发射
/// （[`ledger_infra::events::emit_backups_changed_current`]，任意写线程）经
/// 深路径发射器桥接置数；引擎线程泵（[`crate::backup_host`] 装载）消费计数
/// 后在本线程广播 [`notify_backups_invalidated`]。计数无场景态（进程级原子），
/// 桥接与消费两侧任意场景顺序下语义不变。发射可丢失旧壳 payload 同款：粗粒
/// 度失效信号只表达「备份列表变过」，消费方重拉即得最新全量。
static BACKUPS_PENDING: AtomicUsize = AtomicUsize::new(0);

/// 订阅句柄发放计数器（进程级单调，每订阅一个；跨族共享——id 全表唯一，
/// 句柄撤销按 id 清行即可，无需族信息）。
static NEXT_SUBSCRIBER: AtomicU64 = AtomicU64::new(0);

/// 订阅句柄（四族共用单型）：实例作用域持有，随实例消亡自动撤销——Drop 兜底，
/// 与 [`crate::overlay::OverlayToken`] 同形。四族的公开句柄名（
/// [`TransactionsChangedSubscription`] 等）是本型的别名。
#[derive(Debug)]
pub struct SignalSubscription {
    id: u64,
}

impl Drop for SignalSubscription {
    fn drop(&mut self) {
        SUBSCRIBERS.with_borrow_mut(|slot| slot.retain(|(id, _, _)| *id != self.id));
    }
}

/// 订阅单点内核：入族表（订阅 id + 族 + 处理器），返回句柄（消亡即撤销）。
fn subscribe_family(family: Family, handler: impl Fn() + 'static) -> SignalSubscription {
    let id = NEXT_SUBSCRIBER.fetch_add(1, Ordering::Relaxed);
    SUBSCRIBERS.with_borrow_mut(|slot| slot.push((id, family, Rc::new(handler))));
    SignalSubscription { id }
}

/// 广播单点内核：克隆出本族处理器再逐个调用——广播路径上的再入订阅/撤销不与
/// 借用冲突，他族订阅者不触达（族过滤即跨族不串扰）。
fn notify_family(family: Family) {
    let handlers = SUBSCRIBERS.with(|slot| {
        slot.borrow()
            .iter()
            .filter(|(_, f, _)| *f == family)
            .map(|(_, _, handler)| handler.clone())
            .collect::<Vec<_>>()
    });
    for handler in handlers {
        handler();
    }
}

/// 交易数据失效的订阅句柄（[`SignalSubscription`] 别名，公开名面沿立桩名）。
pub type TransactionsChangedSubscription = SignalSubscription;

/// 预算数据失效的订阅句柄（[`SignalSubscription`] 别名）。
pub type BudgetsChangedSubscription = SignalSubscription;

/// 投资数据失效的订阅句柄（[`SignalSubscription`] 别名）。
pub type InvestmentsChangedSubscription = SignalSubscription;

/// 备份产物失效的订阅句柄（[`SignalSubscription`] 别名）。
pub type BackupsChangedSubscription = SignalSubscription;

/// 订阅交易数据失效信号（视图装载时注册一次；返回句柄消亡即撤销订阅）。
pub fn subscribe_transactions_changed(
    handler: impl Fn() + 'static,
) -> TransactionsChangedSubscription {
    subscribe_family(Family::Transactions, handler)
}

/// 订阅预算数据失效信号（预算视图装载时注册一次；返回句柄消亡即撤销订阅）。
pub fn subscribe_budgets_changed(handler: impl Fn() + 'static) -> BudgetsChangedSubscription {
    subscribe_family(Family::Budgets, handler)
}

/// 订阅投资数据失效信号（概览视图投资/自由度卡装载时注册一次；返回句柄消亡
/// 即撤销订阅）。
pub fn subscribe_investments_changed(
    handler: impl Fn() + 'static,
) -> InvestmentsChangedSubscription {
    subscribe_family(Family::Investments, handler)
}

/// 订阅备份产物失效信号（设置卡备份列表/自动备份状态装载时注册一次；返回
/// 句柄消亡即撤销订阅）。
pub fn subscribe_backups_changed(handler: impl Fn() + 'static) -> BackupsChangedSubscription {
    subscribe_family(Family::Backups, handler)
}

/// 广播交易数据失效（写入成功后调用；粗粒度、无 payload——消费方自行重拉）。
pub fn notify_transactions_invalidated() {
    notify_family(Family::Transactions);
}

/// 广播预算数据失效（预算写路径成功后调用；粗粒度、无 payload——消费方自行
/// 重拉；各族信号互不串扰）。
pub fn notify_budgets_invalidated() {
    notify_family(Family::Budgets);
}

/// 广播投资数据失效（投资写路径成功后调用；粗粒度、无 payload——消费方自行
/// 重拉；各族信号互不串扰。广播接线随投资写路径迁移票落地）。
pub fn notify_investments_invalidated() {
    notify_family(Family::Investments);
}

/// 广播备份产物失效（设置卡备份列表与自动备份状态重读；粗粒度、无 payload，
/// 各族信号互不串扰）。只在引擎线程调用（深路径发射的跨线程投递经
/// [`record_backups_pending`] + 泵消费，不直呼本函数）。
pub fn notify_backups_invalidated() {
    notify_family(Family::Backups);
}

/// 深路径发射落点（任意写线程；infra 深路径发射器桥接实现消费）：置一次
/// 备份产物变更 pending。非阻塞、无锁争用面（原子加计）——发射失败静默语义
/// 沿 ADR-0044（本落点不可失败）。
pub fn record_backups_pending() {
    BACKUPS_PENDING.fetch_add(1, Ordering::Relaxed);
}

/// 消费备份产物变更 pending（UI 线程泵消费点）：有待消费置数则清零并返回
/// true，泵据此广播 [`notify_backups_invalidated`]。
pub fn take_backups_pending() -> bool {
    BACKUPS_PENDING.swap(0, Ordering::Relaxed) > 0
}

/// 清空全部订阅（headless 多场景串行测试的复位接缝：上一场景的引擎已撤销，
/// 其视图订阅者不得响应下一场景的广播；生产 bin 不消费）。与
/// [`crate::app::reset_book_assembly`] 同一批复位点。全表一并复位；备份族
/// 的 pending 计数同点清零（跨场景残留会令下一场景首帧就广播一次）。
#[doc(hidden)]
pub fn reset_for_tests() {
    SUBSCRIBERS.with_borrow_mut(|slot| slot.clear());
    BACKUPS_PENDING.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// thread_local 槽位在测试间共享（同线程串行）：每个用例先复位再订阅。
    fn reset() {
        reset_for_tests();
    }

    #[test]
    fn notify_reaches_every_subscriber() {
        reset();
        let hits = Rc::new(AtomicUsize::new(0));
        let a = subscribe_transactions_changed({
            let hits = hits.clone();
            move || {
                hits.fetch_add(1, Ordering::Relaxed);
            }
        });
        let b = subscribe_transactions_changed({
            let hits = hits.clone();
            move || {
                hits.fetch_add(10, Ordering::Relaxed);
            }
        });
        notify_transactions_invalidated();
        assert_eq!(hits.load(Ordering::Relaxed), 11, "广播到达每个订阅者");
        drop(a);
        drop(b);
        reset();
    }

    #[test]
    fn unsubscribed_handlers_are_not_called() {
        reset();
        let hits = Rc::new(AtomicUsize::new(0));
        let subscription = subscribe_transactions_changed({
            let hits = hits.clone();
            move || {
                hits.fetch_add(1, Ordering::Relaxed);
            }
        });
        drop(subscription);
        notify_transactions_invalidated();
        assert_eq!(hits.load(Ordering::Relaxed), 0, "撤销后的订阅者不再被调用");
        reset();
    }

    #[test]
    fn reset_clears_every_subscription() {
        reset();
        let hits = Rc::new(AtomicUsize::new(0));
        let subscription = subscribe_transactions_changed({
            let hits = hits.clone();
            move || {
                hits.fetch_add(1, Ordering::Relaxed);
            }
        });
        reset();
        drop(subscription);
        notify_transactions_invalidated();
        assert_eq!(hits.load(Ordering::Relaxed), 0, "复位后广播无人接收");
        reset();
    }

    #[test]
    fn budgets_notify_reaches_subscriber_and_reset_clears() {
        reset();
        let hits = Rc::new(AtomicUsize::new(0));
        let subscription = subscribe_budgets_changed({
            let hits = hits.clone();
            move || {
                hits.fetch_add(1, Ordering::Relaxed);
            }
        });
        notify_budgets_invalidated();
        assert_eq!(hits.load(Ordering::Relaxed), 1, "预算广播到达订阅者");
        drop(subscription);
        notify_budgets_invalidated();
        assert_eq!(hits.load(Ordering::Relaxed), 1, "撤销后的订阅者不再被调用");
        let held = subscribe_budgets_changed(|| {});
        reset();
        drop(held);
        notify_budgets_invalidated();
        // 复位后广播无再入路径可断（表已清空）；本用例以不 panic 收尾。
    }

    #[test]
    fn transaction_broadcast_does_not_reach_budget_subscribers() {
        reset();
        let budget_hits = Rc::new(AtomicUsize::new(0));
        let _budgets = subscribe_budgets_changed({
            let hits = budget_hits.clone();
            move || {
                hits.fetch_add(1, Ordering::Relaxed);
            }
        });
        notify_transactions_invalidated();
        assert_eq!(
            budget_hits.load(Ordering::Relaxed),
            0,
            "两族信号互不串扰：交易广播不触发预算订阅者"
        );
        notify_budgets_invalidated();
        assert_eq!(
            budget_hits.load(Ordering::Relaxed),
            1,
            "预算广播触发预算订阅者"
        );
        reset();
    }

    #[test]
    fn investments_notify_reaches_subscriber_and_reset_clears() {
        reset();
        let hits = Rc::new(AtomicUsize::new(0));
        let subscription = subscribe_investments_changed({
            let hits = hits.clone();
            move || {
                hits.fetch_add(1, Ordering::Relaxed);
            }
        });
        notify_investments_invalidated();
        assert_eq!(hits.load(Ordering::Relaxed), 1, "投资广播到达订阅者");
        drop(subscription);
        notify_investments_invalidated();
        assert_eq!(hits.load(Ordering::Relaxed), 1, "撤销后的订阅者不再被调用");
        let held = subscribe_investments_changed(|| {});
        reset();
        drop(held);
        notify_investments_invalidated();
        // 复位后广播无再入路径可断（表已清空）；本用例以不 panic 收尾。
    }

    #[test]
    fn investment_broadcast_is_isolated_from_transaction_and_budget_families() {
        reset();
        let investment_hits = Rc::new(AtomicUsize::new(0));
        let _investments = subscribe_investments_changed({
            let hits = investment_hits.clone();
            move || {
                hits.fetch_add(1, Ordering::Relaxed);
            }
        });
        // 交易/预算广播不触发投资订阅者（各族互不串扰）。
        notify_transactions_invalidated();
        notify_budgets_invalidated();
        assert_eq!(
            investment_hits.load(Ordering::Relaxed),
            0,
            "交易/预算广播不触发投资订阅者"
        );
        notify_investments_invalidated();
        assert_eq!(investment_hits.load(Ordering::Relaxed), 1);
        reset();
    }

    #[test]
    fn subscribing_during_notify_does_not_conflict() {
        reset();
        // 广播路径先克隆出本族处理器再调用：订阅者在广播内再入订阅/撤销不与
        // 借用冲突；本轮广播只触达先已在表的订阅者（新订阅下一轮生效）。
        let hits = Rc::new(AtomicUsize::new(0));
        let late = Rc::new(AtomicUsize::new(0));
        let first = subscribe_transactions_changed({
            let hits = hits.clone();
            move || {
                hits.fetch_add(1, Ordering::Relaxed);
            }
        });
        let joining = subscribe_transactions_changed({
            let hits = hits.clone();
            let late = late.clone();
            move || {
                hits.fetch_add(1, Ordering::Relaxed);
                let joined = subscribe_transactions_changed({
                    let late = late.clone();
                    move || {
                        late.fetch_add(1, Ordering::Relaxed);
                    }
                });
                drop(joined);
            }
        });
        notify_transactions_invalidated();
        assert_eq!(hits.load(Ordering::Relaxed), 2, "本轮广播触达先在订阅者");
        assert_eq!(
            late.load(Ordering::Relaxed),
            0,
            "广播中再入的订阅本轮不触发"
        );
        notify_transactions_invalidated();
        assert_eq!(hits.load(Ordering::Relaxed), 4);
        assert_eq!(
            late.load(Ordering::Relaxed),
            0,
            "句柄已撤销的再入订阅不触发"
        );
        drop(first);
        drop(joining);
        reset();
    }

    /// 备份族（#2078）：广播到达订阅者、跨族不串扰、pending 计数置/取对称
    /// （深路径落点 → 泵消费 → 广播的手工重演：落点任意线程可调，本测在测试
    /// 线程直呼验证计数语义；泵接线归 backup_host 装载，headless 旅程钉住）。
    #[test]
    fn backups_notify_reaches_subscriber_and_pending_round_trips() {
        reset();
        let hits = Rc::new(AtomicUsize::new(0));
        let subscription = subscribe_backups_changed({
            let hits = hits.clone();
            move || {
                hits.fetch_add(1, Ordering::Relaxed);
            }
        });
        record_backups_pending();
        record_backups_pending();
        assert!(take_backups_pending(), "有待消费 pending 应取到 true");
        assert!(!take_backups_pending(), "消费即清零，第二次取应为 false");
        notify_backups_invalidated();
        assert_eq!(hits.load(Ordering::Relaxed), 1, "广播到达订阅者");
        // 跨族互不串扰：交易广播不触发备份订阅者。
        notify_transactions_invalidated();
        assert_eq!(hits.load(Ordering::Relaxed), 1);
        drop(subscription);
        reset();
        notify_backups_invalidated();
        assert_eq!(hits.load(Ordering::Relaxed), 1, "复位后广播无人接收");
        reset();
    }
}
