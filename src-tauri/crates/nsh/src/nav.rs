//! 进程内导航 API 与导航状态（issue #2032 壳导航骨架；ADR-0145 拆票② / spec
//! #2031）。
//!
//! 旧壳导航四件的重生与归位：`vue-router` 的进程内等价物（当前视图状态 +
//! 守卫裁决落位）、`sidebar-order` store（组内序/收纳清单 + 点选即写双存储
//! 同步）、`feature-toggles` store（关闭集合只读面——设置面票接写入口）、
//! `useViewShortcuts`（键位带派发）。守卫与词表的**纯决策半边**住
//! [`crate::nav_model`]，本模块是反应式外壳 + 持久化 + 派发接线。
//!
//! **单点接缝**（本票核心新件）：[`open`] 是进程内导航的唯一入口——守卫裁决
//! → 状态落位 → 持久化；后续视图票只消费不自建（下钻/跳转经
//! [`nav_model::resolve_source_jump`] 造 [`NavRequest`] 后调 [`open`]，消费
//! 侧在视图挂载时 [`take_intent`] 读一次）。等价旧壳 URL query 语义的映射：
//! 容器页签 / 定时形态页签 / focus 定位参数各归 [`NavRequest`] 独立字段（进程
//! 内无双写互踩），focus 的「读一次、不持久化、消费时机归视图」由
//! [`take_intent`] 单点实现。
//!
//! 界面状态持久化 = NSH 自建等价物：`nsh_core::storage::Storage` 平台文件侧
//! KV（金额隐私开关同款先例），键与值形状镜像旧壳 `view_state:*` 族（同键同
//! 值形态），但**不与旧壳共享**——旧壳读 WebView localStorage，本壳读平台存
//! 储，天然隔离；不进 SQLite、不动 schema（发布边界零涉）。
//!
//! 线程契约（[`crate::signals`] 同款）：信号读写都在引擎 UI 线程——本模块的
//! 导航入口由 UI 事件处理器（引擎线程）调用；headless 组件测试经引擎线程的
//! 真实生产面驱动（键位带 / 点击），focus 意图投递由 lib 内探针组件测试钉住
//! （引擎线程内调 [`open`]，消费面挂真实占位组件）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::sync::Mutex;

use nsh_core::prelude::*;
use serde_json::Value;

use crate::nav_model::{
    self, FactoryLists, GroupLists, NavFacts, NavRequest, ScheduledFormTab, SortAction, ViewName,
};
use crate::overlay;

/// 界面状态存储键（旧壳 `view_state:*` 同键同值形态：route = JSON 字符串、
/// 侧栏序/收纳清单 = 对象「组 id → 视图名数组」、关闭集合 = 数组；值形状见
/// [`crate::nav_model`] 的序列化与解析家族）。
const ROUTE_KEY: &str = "view_state:route";
const ORDERS_KEY: &str = "view_state:sidebar_order";
const CONTAINMENT_KEY: &str = "view_state:sidebar_containment";
const CLOSED_KEY: &str = "view_state:closed_features";

/// 下钻接缝的意图载荷（[`NavRequest`] 落位后待投递的部分）：实体定位参数 +
/// 定时形态页签。消费语义 = 读一次（[`take_intent`]），不持久化（重启即失，
/// 等价旧壳 focus 不写回、刷新重投在进程内的对应物 = 重新导航重新投递）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NavIntent {
    /// 实体定位参数（引用判据的载体；消费视图决定定位形态——开弹窗 / 滚动
    /// 高亮 / 分页锚点，旧壳三种形态的决策面归视图）。
    pub focus: Option<String>,
    /// 定时形态页签意图（目标为定时——直达或引用——时有义）。
    pub form_tab: Option<ScheduledFormTab>,
}

/// 导航反应式状态句柄（Copy；信号挂首次 [`store`] 调用所在组件实例——生产
/// 路径契约：`App` 组件体最先调用，状态挂根实例，随进程存活）。
#[derive(Clone, Copy)]
pub struct NavStore {
    /// 当前落位视图（含组「更多」容器）。
    pub(crate) current: Signal<ViewName>,
    /// 容器页签（会话态：页签不跨启动持久化，旧壳 query.tab 同口径；None =
    /// 容器默认首签）。
    pub(crate) container_tab: Signal<Option<ViewName>>,
    /// 待投递下钻意图（读一次，[`take_intent`] 消费）。
    pub(crate) intent: Signal<Option<NavIntent>>,
    /// 各组组内序（侧栏成员唯一来源；含移回的种子成员）。
    pub(crate) orders: Signal<GroupLists>,
    /// 各组收纳清单（「更多」页签唯一来源；清单序 = 页签序）。
    pub(crate) containment: Signal<GroupLists>,
    /// 已关闭功能集合（闭集内、清单序；只读面——写入口随设置面票）。
    pub(crate) closed: Signal<Vec<ViewName>>,
}

/// 进程级导航状态槽（headless 多场景串行复位，[`reset_for_tests`]；生产进程
/// 一次初始化）。锁毒化时取回内值——槽位本身不含不变量（app.rs 槽位同形）。
static NAV: Mutex<Option<NavStore>> = Mutex::new(None);

fn nav_slot() -> std::sync::MutexGuard<'static, Option<NavStore>> {
    NAV.lock().unwrap_or_else(|p| p.into_inner())
}

/// 导航状态（惰性初始化：首次消费执行一次启动读路径——读存储、解析防御、
/// 启动恢复裁决落位，与旧壳 pinia store 首次实例化同形）。须在引擎线程调用；
/// 生产路径在 `App` 组件体最先调用（信号挂根实例）。
pub fn store() -> NavStore {
    let mut slot = nav_slot();
    if let Some(s) = *slot {
        return s;
    }
    let s = init_store();
    *slot = Some(s);
    s
}

/// 复位导航状态槽（headless 测试接缝，[`crate::app::reset_book_assembly`] 消费）。
#[doc(hidden)]
pub fn reset_for_tests() {
    *nav_slot() = None;
}

/// 启动读路径：先解析收纳清单（移回豁免凭原始组内序），组内序解析凭它排除
/// 收纳成员；关闭集合解析防御；上次视图经守卫裁决落位（缺失/未知名/已关闭
/// 回出厂落点，被收纳视图落容器——旧壳 restoreLastView + 路由守卫两道接线
/// 的合并）。
fn init_store() -> NavStore {
    let raw_orders = read_value(ORDERS_KEY);
    let containment = nav_model::parse_containment_lists(
        &read_value(CONTAINMENT_KEY).unwrap_or(Value::Null),
        raw_orders.as_ref(),
    );
    let orders =
        nav_model::parse_group_orders(&raw_orders.clone().unwrap_or(Value::Null), &containment);
    let closed = nav_model::parse_closed_features(&read_value(CLOSED_KEY).unwrap_or(Value::Null));
    let saved = read_value(ROUTE_KEY).and_then(|v| v.as_str().map(str::to_owned));
    let facts = NavFacts::new(closed.clone(), containment.clone());
    let restored = nav_model::resolve_restored(saved.as_deref(), &facts);
    NavStore {
        current: signal(restored),
        container_tab: signal(None),
        intent: signal(None),
        orders: signal(orders),
        containment: signal(containment),
        closed: signal(closed),
    }
}

/// 存储读（JSON 载荷；缺失/损坏回 None 由解析防御兜底——旧壳 loadLocal 静默
/// 容错同口径）。
fn read_value(key: &str) -> Option<Value> {
    let raw = nsh_core::storage::Storage::get_item(key)?;
    serde_json::from_str(&raw).ok()
}

/// 存储写（同步 KV；配额 5MB、导航载荷远小于配额。写失败不静默：告警日志 +
/// 状态仍在本会话生效，下次启动回默认——等待成功优于即刻失败，不因持久化
/// 失败打断导航）。
fn write_value(key: &str, value: &Value) {
    if let Err(e) = nsh_core::storage::Storage::set_item(key, value.to_string()) {
        tracing::warn!(key, error = %e, "导航界面状态写入存储失败");
    }
}

impl NavStore {
    /// 守卫问询快照（纯裁决的入参；引擎线程内现读）。
    pub(crate) fn facts_snapshot(&self) -> NavFacts {
        NavFacts::new(
            self.closed.get_untracked(),
            self.containment.get_untracked(),
        )
    }

    /// 关闭态查询（响应式读的便捷面：菜单/页签剔除用）。
    pub fn is_closed(&self, view: ViewName) -> bool {
        self.closed.get_untracked().contains(&view) && view.closable()
    }

    /// 收纳在册查询（旧壳 isViewContained 同语义；跳转落点/菜单问询面）。
    pub fn is_view_contained(&self, view: ViewName) -> bool {
        self.containment.get_untracked().contains_any(view)
    }
}

/// **进程内导航 API 单点入口**：守卫裁决 → 状态落位 → 意图投递 → 上次视图
/// 持久化。语义（逐条对齐旧壳）：
/// - 关闭态裸访问回概览、带定位参数的引用放行（守卫裁决一）；
/// - 被收纳且在分流配置内的视图改道组「更多」对应页签（守卫裁决二）；
/// - 容器页签指向已关闭功能改道自有路由二次裁决、非法页签归默认首签；
/// - 落位意图随落位投递（改道概览的裸访问不投递）；同视图重复打开幂等
///   （信号写等值不通知，无重挂）。
pub fn open(request: NavRequest) {
    let s = store();
    let facts = s.facts_snapshot();
    let landing = nav_model::resolve_landing(request.clone(), &facts);
    let direct = landing.view == request.target;
    // 意图随落位投递：直达落位携带请求意图；改道（关闭兜底回概览）视为入口
    // 导航，意图一并丢弃（旧壳：重定向后的 URL 不再带 focus）。
    let intent = if direct {
        NavIntent {
            focus: request.focus,
            form_tab: request.form_tab,
        }
    } else {
        NavIntent::default()
    };
    let changed = s.current.get_untracked() != landing.view;
    s.current.set(landing.view);
    s.container_tab.set(landing.container_tab);
    s.intent
        .set((intent != NavIntent::default()).then_some(intent));
    if changed {
        // 上次视图持久化（旧壳 afterEach saveRouteName 同语义：保存最终落位
        // 名；同名不写）。JSON 字符串形状与旧壳 view_state:route 同形。
        write_value(ROUTE_KEY, &serde_json::json!(landing.view.as_str()));
    }
}

/// 消费待投递的下钻意图（**读一次**：在场回调载体、空转也封闸——此后重复 /
/// 迟到消费一律丢弃；页签切换重挂不复弹；重启即失）。每视图挂载时消费一次，
/// 只写「拿到意图后做什么」，时序归消费视图（旧壳 useFocusParam 单点同形）。
pub fn take_intent() -> Option<NavIntent> {
    let s = store();
    let pending = s.intent.get_untracked();
    s.intent.set(None);
    pending
}

/// 右键排序写路径（点选即写：组内序重排 + 立即持久化）。边界 no-op 不写存储
/// （保住「恢复默认 = 删 key」语义，旧壳 applySidebarSort 同纪律）。
pub fn apply_sort(name: ViewName, action: SortAction) {
    let s = store();
    let Some(gid) = nav_model::group_of_view(name) else {
        return;
    };
    let prev = s.orders.get_untracked();
    let next_members = nav_model::move_arrangeable(prev.get(gid), name, action);
    if prev.get(gid) == next_members.as_slice() {
        return;
    }
    let mut next = prev;
    next.set(gid, next_members);
    s.orders.set(next.clone());
    write_value(ORDERS_KEY, &nav_model::group_lists_to_value(&next));
}

/// 右键「移入更多」写路径：主项退出组内序（键位随动重排）+ 追加本组收纳清单
/// 尾；点选即写、双存储同步（旧壳 applyMoveIntoMore 同纪律）。非在册主项
/// no-op 不写。
pub fn apply_move_into_more(name: ViewName) {
    let s = store();
    let Some(gid) = nav_model::group_of_view(name) else {
        return;
    };
    let orders = s.orders.get_untracked();
    if !orders.get(gid).contains(&name) {
        return;
    }
    let containment = s.containment.get_untracked();
    let mut next_orders = orders.clone();
    next_orders.set(
        gid,
        orders
            .get(gid)
            .iter()
            .copied()
            .filter(|v| *v != name)
            .collect(),
    );
    let next_containment = nav_model::move_into_containment(&containment, gid, name);
    s.orders.set(next_orders.clone());
    s.containment.set(next_containment.clone());
    write_value(ORDERS_KEY, &nav_model::group_lists_to_value(&next_orders));
    write_value(
        CONTAINMENT_KEY,
        &nav_model::group_lists_to_value(&next_containment),
    );
}

/// 容器页签右键「移回侧栏」写路径：成员退出本组收纳清单 + 落本组主项末位
/// （键位随动重排）；点选即写、双存储同步；组满拒写（≤3 运行时硬上限兜底，
/// 菜单置灰为第一道防线）；非清单成员 no-op 不写（旧壳 applyMoveBackToSidebar
/// 同纪律）。
pub fn apply_move_back_to_sidebar(name: ViewName) {
    let s = store();
    let Some(gid) = nav_model::group_of_view(name) else {
        return;
    };
    let containment = s.containment.get_untracked();
    if !containment.get(gid).contains(&name) {
        return;
    }
    let orders = s.orders.get_untracked();
    if nav_model::is_group_full(orders.get(gid)) {
        return;
    }
    let next_containment = nav_model::move_back_to_sidebar(&containment, gid, name);
    let mut next_orders = orders.clone();
    let mut members = orders.get(gid).to_vec();
    members.push(name);
    next_orders.set(gid, members);
    s.containment.set(next_containment.clone());
    s.orders.set(next_orders.clone());
    write_value(
        CONTAINMENT_KEY,
        &nav_model::group_lists_to_value(&next_containment),
    );
    write_value(ORDERS_KEY, &nav_model::group_lists_to_value(&next_orders));
}

/// 恢复默认排序：组内序与收纳清单一并清除回出厂（删两 key = 出厂语义，旧壳
/// resetSidebarOrder 同语义，出厂默认序将来调整时自动跟随）。
pub fn reset_sidebar_order() {
    let s = store();
    nsh_core::storage::Storage::remove_item(ORDERS_KEY);
    nsh_core::storage::Storage::remove_item(CONTAINMENT_KEY);
    s.orders.set(GroupLists::factory(FactoryLists::Orders));
    s.containment
        .set(GroupLists::factory(FactoryLists::Containment));
}

/// 键位派发（键位带处理器体，「闸门 → 决策 → 派发」三步，先例翻页/周期步进
/// 同形）：弹层闸（Overlay Suppression，删除即红钉在 headless 抑制测试）→
/// 键位带推导（组内序 + 关闭集合现读）→ 同视图幂等 → [`open`]。`mod+` 组合
/// 键非文本编辑键，输入焦点期间照常派发（旧壳 ViewShortcut「输入框聚焦时组
/// 合键仍生效」同语义；`When::Auto` 绑定对非编辑键恒放行）。
fn dispatch_key(key: char) {
    // Overlay Suppression 闸门：弹层开着不派发。
    if overlay::has_open_overlay() {
        return;
    }
    let s = store();
    let shortcuts =
        nav_model::derive_view_shortcuts(&s.orders.get_untracked(), &s.closed.get_untracked());
    let Some(shortcut) = shortcuts.iter().find(|sc| sc.key == key) else {
        return;
    };
    if shortcut.view == s.current.get_untracked() {
        return;
    }
    open(NavRequest::bare(shortcut.view));
}

/// 注册键位带（⌘` 概览 / ⌘1–⌘9 三组固定带 / ⌘0 AI / ⌘, 设置，ADR-0065 固定
/// 组带；`mod` 在 Apple 平台为 Command、其余为 Control）。在 `App` 组件体调用
/// 一次：绑定随根组件实例存亡（`on_key` 的 owner 挂当前组件）。处理器即
/// [`dispatch_key`]。
pub fn bind_shortcuts() {
    on_key("mod+`", |_| dispatch_key('`'));
    for key in '1'..='9' {
        on_key(&format!("mod+{key}"), move |_| dispatch_key(key));
    }
    on_key("mod+0", |_| dispatch_key('0'));
    on_key("mod+,", |_| dispatch_key(','));
}

/// 键位提示（侧栏项右端小字；`apple` 取运行平台——bin 当前仅 Linux，⌃ 形态
/// 在役，macOS 随平台票开门后取 `os()` 判定）。
pub(crate) fn shortcut_hint_text(key: char, apple: bool) -> String {
    nav_model::shortcut_hint(key, apple)
}

#[cfg(test)]
mod tests;
