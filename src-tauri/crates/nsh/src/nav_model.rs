//! 导航纯模型（issue #2032 壳导航骨架）：视图/分组闭集、zh-CN 词表、侧栏出厂
//! 定义（三组主项 + 每组收纳种子 + 主项硬上限）、键位带推导（旧壳
//! `useViewShortcuts.ts` `deriveViewShortcuts` 的重生）、持久化载荷解析防御
//! （旧壳 `parseGroupOrders` / `parseContainmentLists` / `parseClosedFeatures`
//! 的重生）、守卫落位裁决（旧壳 `featureRouteGuard` + `groupMoreTabGuard` +
//! `resolveRestoredViewName` 的进程内重生）与跳转目标计算（旧壳
//! `source-jump.ts` `resolveSourceJumpTarget` 的进程内重生）。
//!
//! 纯函数纪律（旧壳同款）：不见引擎、不见 store、不见存储——状态经入参注入，
//! 可离线直测；响应式外壳与持久化住 [`crate::nav`]。
//!
//! 词表平价基准：旧壳 `src/stores/sidebar-order.ts`（组与种子）、
//! `src/settings/feature-toggles.ts`（可关九项）、
//! `packages/i18n/src/locales/zh-CN/common.json`（nav / sidebarGroup /
//! sidebarSort / sidebarContainment 文案）。语义裁决见本文件各条目注释与
//! ADR-0145 / spec #2031。

/// 视图名词表（旧壳路由 name 同词，持久化序列化与旧壳同形同键）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewName {
    /// 概览（旧壳固定首位；不可关）。
    Dashboard,
    /// 交易（已迁移，立桩首屏；不可关）。
    Transactions,
    /// 搜索（不可关）。
    Search,
    /// 账户（不可关）。
    Accounts,
    /// 报表。
    Reports,
    /// 投资。
    Investments,
    /// 定时（出厂收纳于记账组）。
    Scheduled,
    /// 预算。
    Budget,
    /// 攒钱。
    SavingsGoals,
    /// 物品。
    Items,
    /// 保单（出厂收纳于资产组）。
    Policies,
    /// 商户（出厂收纳于记账组）。
    Merchants,
    /// 实物资产（出厂收纳于资产组）。
    PhysicalAssets,
    /// 保险公司（出厂收纳于资产组）。
    Insurers,
    /// 投资汇总（旧壳不入侧栏分组，随投资开关；本壳经导航接缝可达）。
    CrossBookSummary,
    /// 记账组「更多」容器。
    BookkeepingMore,
    /// 资产组「更多」容器。
    AssetsMore,
    /// 洞察组「更多」容器。
    InsightsMore,
    /// AI（旧壳固定倒数第二；不可关）。
    Ai,
    /// 设置（旧壳固定末位；不可关）。
    Settings,
}

/// 视图 → 持久化名（旧壳路由 name 同词；`view_state:route` 与
/// `view_state:sidebar_order` / `sidebar_containment` / `closed_features` 的
/// JSON 载荷值用同一词表）。
impl ViewName {
    pub fn as_str(self) -> &'static str {
        match self {
            ViewName::Dashboard => "dashboard",
            ViewName::Transactions => "transactions",
            ViewName::Search => "search",
            ViewName::Accounts => "accounts",
            ViewName::Reports => "reports",
            ViewName::Investments => "investments",
            ViewName::Scheduled => "scheduled",
            ViewName::Budget => "budget",
            ViewName::SavingsGoals => "savingsGoals",
            ViewName::Items => "items",
            ViewName::Policies => "policies",
            ViewName::Merchants => "merchants",
            ViewName::PhysicalAssets => "physicalAssets",
            ViewName::Insurers => "insurers",
            ViewName::CrossBookSummary => "cross-book-summary",
            ViewName::BookkeepingMore => "bookkeeping-more",
            ViewName::AssetsMore => "assets-more",
            ViewName::InsightsMore => "insights-more",
            ViewName::Ai => "ai",
            ViewName::Settings => "settings",
        }
    }

    /// 持久化名 → 视图（未知名 None；调用方回退出厂落点）。
    pub fn parse(name: &str) -> Option<Self> {
        ALL.iter().copied().find(|v| v.as_str() == name)
    }

    /// zh-CN 屏名（旧壳 `common.nav.<name>` 同词表）。
    pub fn label(self) -> &'static str {
        match self {
            ViewName::Dashboard => "概览",
            ViewName::Transactions => "交易",
            ViewName::Search => "搜索",
            ViewName::Accounts => "账户",
            ViewName::Reports => "报表",
            ViewName::Investments => "投资",
            ViewName::Scheduled => "定时",
            ViewName::Budget => "预算",
            ViewName::SavingsGoals => "攒钱",
            ViewName::Items => "物品",
            ViewName::Policies => "保单",
            ViewName::Merchants => "商户",
            ViewName::PhysicalAssets => "实物资产",
            ViewName::Insurers => "保险公司",
            ViewName::CrossBookSummary => "投资汇总",
            ViewName::BookkeepingMore => "记账 · 更多",
            ViewName::AssetsMore => "资产 · 更多",
            ViewName::InsightsMore => "洞察 · 更多",
            ViewName::Ai => "AI",
            ViewName::Settings => "设置",
        }
    }

    /// 是否组「更多」容器。
    pub fn is_group_more(self) -> bool {
        matches!(
            self,
            ViewName::BookkeepingMore | ViewName::AssetsMore | ViewName::InsightsMore
        )
    }

    /// 已关闭时入口剔除与守卫改道是否适用（旧壳 `isClosableFeature`：可关九项
    /// 闭集；不可关六项与未知恒 false）。id 与侧栏视图名同词（旧壳同款，不另
    /// 造映射层）。
    pub fn closable(self) -> bool {
        matches!(
            self,
            ViewName::Budget
                | ViewName::Reports
                | ViewName::Scheduled
                | ViewName::Merchants
                | ViewName::Investments
                | ViewName::Items
                | ViewName::Policies
                | ViewName::PhysicalAssets
                | ViewName::Insurers
        )
    }
}

/// 全量闭集（`ViewName::parse` 的穷尽扫描序）。
const ALL: [ViewName; 20] = [
    ViewName::Dashboard,
    ViewName::Transactions,
    ViewName::Search,
    ViewName::Accounts,
    ViewName::Reports,
    ViewName::Investments,
    ViewName::Scheduled,
    ViewName::Budget,
    ViewName::SavingsGoals,
    ViewName::Items,
    ViewName::Policies,
    ViewName::Merchants,
    ViewName::PhysicalAssets,
    ViewName::Insurers,
    ViewName::CrossBookSummary,
    ViewName::BookkeepingMore,
    ViewName::AssetsMore,
    ViewName::InsightsMore,
    ViewName::Ai,
    ViewName::Settings,
];

/// 侧栏分组（旧壳 `SidebarGroupId` 同词：组 id 即旧壳 i18n key
/// `common.sidebarGroup.<id>`）。组与组序固定（旧壳同款）：记账 → 资产 → 洞察；
/// 组内序可排（右键菜单）且持久化对象收窄为组内序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupId {
    /// 记账。
    Bookkeeping,
    /// 资产。
    Assets,
    /// 洞察。
    Insights,
}

/// 全部分组（固定组序）。
pub const GROUPS: [GroupId; 3] = [GroupId::Bookkeeping, GroupId::Assets, GroupId::Insights];

impl GroupId {
    /// 持久化名（旧壳组 id 同词，JSON 对象键）。
    pub fn as_str(self) -> &'static str {
        match self {
            GroupId::Bookkeeping => "bookkeeping",
            GroupId::Assets => "assets",
            GroupId::Insights => "insights",
        }
    }

    /// zh-CN 组名（旧壳 `common.sidebarGroup.<id>` 同词表）。
    pub fn label(self) -> &'static str {
        match self {
            GroupId::Bookkeeping => "记账",
            GroupId::Assets => "资产",
            GroupId::Insights => "洞察",
        }
    }

    /// 组「更多」容器视图。
    pub fn more_view(self) -> ViewName {
        match self {
            GroupId::Bookkeeping => ViewName::BookkeepingMore,
            GroupId::Assets => ViewName::AssetsMore,
            GroupId::Insights => ViewName::InsightsMore,
        }
    }

    /// 固定键位带（ADR-0065）：记账 1–3、资产 4–6、洞察 7–9。带内位置由组内序
    /// 决定；主项不足 3 时带内空键保留，不跨组压缩。
    pub fn key_band(self) -> [char; 3] {
        match self {
            GroupId::Bookkeeping => ['1', '2', '3'],
            GroupId::Assets => ['4', '5', '6'],
            GroupId::Insights => ['7', '8', '9'],
        }
    }

    /// 持久化名 → 组（未知名 None）。
    pub fn parse(name: &str) -> Option<Self> {
        GROUPS.iter().copied().find(|g| g.as_str() == name)
    }
}

/// 每组主项硬上限（旧壳 `GROUP_MAIN_LIMIT`，运行时不变量）：3 组 × 3 = ⌘1–⌘9
/// 九键（ADR-0065），键位带封闭性的本体；组满时「移回侧栏」置灰（上限可见、
/// 可学习，不自动换出）。
pub const GROUP_MAIN_LIMIT: usize = 3;

/// 出厂主项定义（旧壳 `SIDEBAR_GROUPS` 同款数组）：组序固定、成员闭集；低频成员
/// 由各组「更多」收纳（[`containment_seeds`]）。
pub fn sidebar_groups() -> &'static [(GroupId, &'static [ViewName])] {
    &[
        (
            GroupId::Bookkeeping,
            &[ViewName::Transactions, ViewName::Accounts, ViewName::Budget],
        ),
        (
            GroupId::Assets,
            &[
                ViewName::Investments,
                ViewName::Items,
                ViewName::SavingsGoals,
            ],
        ),
        (GroupId::Insights, &[ViewName::Reports, ViewName::Search]),
    ]
}

/// 每组收纳清单出厂种子（旧壳 `GROUP_CONTAINMENT_SEEDS` 同款，开发者清单）：
/// 记账 = [定时, 商户]、资产 = [保单, 实物资产, 保司]、洞察 = 空。清单序 =
/// 「更多」页签序。
pub fn containment_seeds(gid: GroupId) -> &'static [ViewName] {
    match gid {
        GroupId::Bookkeeping => &[ViewName::Scheduled, ViewName::Merchants],
        GroupId::Assets => &[
            ViewName::Policies,
            ViewName::PhysicalAssets,
            ViewName::Insurers,
        ],
        GroupId::Insights => &[],
    }
}

/// 视图 → 所属组（主项与出厂种子各有其组，词法归属；概览/AI/设置与容器、
/// 未知名不在任何组）。旧壳 `groupOfView` 同语义（含种子的词法组归属）。
pub fn group_of_view(name: ViewName) -> Option<GroupId> {
    for (gid, views) in sidebar_groups() {
        if views.contains(&name) {
            return Some(*gid);
        }
    }
    GROUPS
        .into_iter()
        .find(|gid| containment_seeds(*gid).contains(&name))
}

/// 「组 id → 成员数组」的三组载荷（组内序与收纳清单同形状，旧壳
/// `SidebarGroupOrders` / `SidebarContainmentLists` 的结构等价物）：字段名即
/// 持久化 JSON 对象键（与旧壳载荷形状逐键一致）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupLists {
    /// 记账组成员（组内序或收纳清单序）。
    pub bookkeeping: Vec<ViewName>,
    /// 资产组成员。
    pub assets: Vec<ViewName>,
    /// 洞察组成员。
    pub insights: Vec<ViewName>,
}

impl GroupLists {
    /// 空清单（三组皆空；测试谓词构造器用）。
    pub fn empty() -> Self {
        Self {
            bookkeeping: Vec::new(),
            assets: Vec::new(),
            insights: Vec::new(),
        }
    }

    /// 出厂态（主项：各组出厂主项序；收纳清单：各组出厂种子序）。
    pub fn factory(kind: FactoryLists) -> Self {
        let fill = |names: &'static [ViewName]| names.to_vec();
        match kind {
            FactoryLists::Orders => Self {
                bookkeeping: fill(sidebar_groups()[0].1),
                assets: fill(sidebar_groups()[1].1),
                insights: fill(sidebar_groups()[2].1),
            },
            FactoryLists::Containment => Self {
                bookkeeping: containment_seeds(GroupId::Bookkeeping).to_vec(),
                assets: containment_seeds(GroupId::Assets).to_vec(),
                insights: containment_seeds(GroupId::Insights).to_vec(),
            },
        }
    }

    /// 读某组成员（不可变）。
    pub fn get(&self, gid: GroupId) -> &[ViewName] {
        match gid {
            GroupId::Bookkeeping => &self.bookkeeping,
            GroupId::Assets => &self.assets,
            GroupId::Insights => &self.insights,
        }
    }

    /// 写某组成员（点选即写的载体；调用方负责持久化）。
    pub fn set(&mut self, gid: GroupId, members: Vec<ViewName>) {
        match gid {
            GroupId::Bookkeeping => self.bookkeeping = members,
            GroupId::Assets => self.assets = members,
            GroupId::Insights => self.insights = members,
        }
    }

    /// 名是否在任一组的当前清单中（运行时在册判定：组内序版 = 侧栏在册；
    /// 收纳清单版 = 收纳在册。旧壳 isSidebarMember / isViewContained 同语义）。
    pub fn contains_any(&self, name: ViewName) -> bool {
        GROUPS.iter().any(|g| self.get(*g).contains(&name))
    }
}

/// [`GroupLists::factory`] 的两种出厂态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactoryLists {
    /// 各组出厂主项序。
    Orders,
    /// 各组出厂收纳种子序。
    Containment,
}

/// 出厂组内序（旧壳 `defaultGroupOrders` 同语义）。
pub fn default_group_orders() -> GroupLists {
    GroupLists::factory(FactoryLists::Orders)
}

/// 视图快捷键（键位带推导产物）：视图与其带内主键；`key` 恒非空——超位成员
/// 不入表（带内下标越界 = 理论上限内不发生，旧壳得 null 的分支在本壳为不入表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewShortcut {
    /// 视图。
    pub view: ViewName,
    /// 主键（带内字符；派发时与 `mod+<key>` 组合）。
    pub key: char,
}

/// 键位带推导纯函数（旧壳 `deriveViewShortcuts` 同语义）：概览恒 `` ` ``、
/// 三组按固定组带（组内序定带内位置）、AI 恒 `0`、设置恒 `,`。只扫主项；
/// 已关闭功能退出键位表但保留其带内下标（空键待命，不跨组压缩，旧壳
/// issue #1242 / ADR-0116 同语义）。收纳成员与容器不入表（无键位、不出提示、
/// 不可键盘触发）。
pub fn derive_view_shortcuts(orders: &GroupLists, closed: &[ViewName]) -> Vec<ViewShortcut> {
    let mut shortcuts = vec![ViewShortcut {
        view: ViewName::Dashboard,
        key: '`',
    }];
    for (gid, _) in sidebar_groups() {
        let band = gid.key_band();
        for (i, name) in orders.get(*gid).iter().enumerate() {
            if closed.contains(name) {
                continue;
            }
            if let Some(&key) = band.get(i) {
                shortcuts.push(ViewShortcut { view: *name, key });
            }
        }
    }
    shortcuts.push(ViewShortcut {
        view: ViewName::Ai,
        key: '0',
    });
    shortcuts.push(ViewShortcut {
        view: ViewName::Settings,
        key: ',',
    });
    shortcuts
}

/// 键位提示文案（旧壳 `shortcutHint` 同形）：Apple 平台 ⌘N、其余 ⌃N（⌃ 与 ⌘
/// 同族单字形、跨平台等宽对齐）。
pub fn shortcut_hint(key: char, apple: bool) -> String {
    if apple {
        format!("⌘{key}")
    } else {
        format!("⌃{key}")
    }
}

/// 可关九项的归一序（旧壳 `CLOSABLE_FEATURES` 清单序同序）：关闭集合输出与
/// 键位带判定外的集合语义按此序稳定可复现。
pub const CLOSABLE_FEATURES: [ViewName; 9] = [
    ViewName::Budget,
    ViewName::Reports,
    ViewName::Scheduled,
    ViewName::Merchants,
    ViewName::Investments,
    ViewName::Items,
    ViewName::Policies,
    ViewName::PhysicalAssets,
    ViewName::Insurers,
];

/// 组内序解析（旧壳 `parseGroupOrders` 同语义纯函数）：已存「组 id → 视图名
/// 数组」JSON → 各组解析后的组内序。整体形状防御：仅接受对象，旧平铺数组、
/// null、标量等整体回默认序。组内防御：合法池 = 出厂主项 ∪ 出厂种子 减去
/// 收纳在册（清单是收纳成员资格的唯一事实源，不复活）；非法名过滤、去重保
/// 首现、缺失项按池序补尾；组值非数组则该组整体回池序默认（他组不受牵连）。
pub fn parse_group_orders(raw: &serde_json::Value, contained: &GroupLists) -> GroupLists {
    let mut result = default_group_orders();
    let Some(rec) = raw.as_object() else {
        return result;
    };
    for (gid, views) in sidebar_groups() {
        let pool: Vec<ViewName> = views
            .iter()
            .copied()
            .chain(containment_seeds(*gid).iter().copied())
            .filter(|v| !contained.get(*gid).contains(v))
            .collect();
        let mut kept: Vec<ViewName> = Vec::new();
        if let Some(arr) = rec.get(gid.as_str()).and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(name) = item.as_str().and_then(ViewName::parse)
                    && pool.contains(&name)
                    && !kept.contains(&name)
                {
                    kept.push(name);
                }
            }
        }
        for name in &pool {
            if !kept.contains(name) {
                kept.push(*name);
            }
        }
        result.set(*gid, kept);
    }
    result
}

/// 收纳清单解析（旧壳 `parseContainmentLists` 同语义纯函数）：已存「组 id →
/// 收纳视图名数组」JSON → 各组解析后的收纳清单。合法池 = 出厂种子 + 本组主项
/// （不跨组）；非法名过滤、去重保首现；缺失种子按出厂序补尾。`raw_orders` 是
/// 未解析的组内序原始值：「移回豁免」判定面——清单在且未列某种子、而种子已在
/// 原始组内序数组中 = 用户已移回主项，不按缺失补尾复活；清单存储缺失（出厂）
/// 不豁免。
pub fn parse_containment_lists(
    raw: &serde_json::Value,
    raw_orders: Option<&serde_json::Value>,
) -> GroupLists {
    let mut result = GroupLists::factory(FactoryLists::Containment);
    let Some(rec) = raw.as_object() else {
        return result;
    };
    let order_rec = raw_orders.and_then(|o| o.as_object());
    for gid in GROUPS {
        let legal: Vec<ViewName> = containment_seeds(gid)
            .iter()
            .copied()
            .chain(
                sidebar_groups()
                    .iter()
                    .find(|(g, _)| g == &gid)
                    .map(|(_, views)| *views)
                    .unwrap_or(&[])
                    .iter()
                    .copied(),
            )
            .collect();
        let mut kept: Vec<ViewName> = Vec::new();
        let raw_arr = rec.get(gid.as_str()).and_then(|v| v.as_array());
        if let Some(arr) = raw_arr {
            for item in arr {
                if let Some(name) = item.as_str().and_then(ViewName::parse)
                    && legal.contains(&name)
                    && !kept.contains(&name)
                {
                    kept.push(name);
                }
            }
        }
        for seed in containment_seeds(gid) {
            if kept.contains(seed) {
                continue;
            }
            // 移回豁免：清单在且未列、原始组内序在——已是主项。
            let moved_back = raw_arr.is_some()
                && order_rec
                    .and_then(|o| o.get(gid.as_str()))
                    .and_then(|v| v.as_array())
                    .is_some_and(|a| a.iter().any(|i| i.as_str() == Some(seed.as_str())));
            if !moved_back {
                kept.push(*seed);
            }
        }
        result.set(gid, kept);
    }
    result
}

/// 关闭集合序列化（旧壳 feature-toggles store `closedFeatures` 数组同形状：
/// 可关九项的持久化名字数组；关闭集合写路径用，载荷形状与旧壳
/// `view_state:closed_features` 同形同值）。不可关项不入载荷。
pub fn closed_features_to_value(closed: &[ViewName]) -> serde_json::Value {
    serde_json::Value::Array(
        normalize_closed_features(closed)
            .into_iter()
            .map(|name| serde_json::Value::String(name.as_str().to_owned()))
            .collect(),
    )
}

/// 关闭集合归一（写路径的纯决策半边）：剔不可关项、去重，输出归一为
/// [`CLOSABLE_FEATURES`] 可关清单序（与 read 路径的
/// [`parse_closed_features`] 同一归一序，写出的集合稳定可复现）。
pub fn normalize_closed_features(closed: &[ViewName]) -> Vec<ViewName> {
    let present: Vec<ViewName> = closed
        .iter()
        .copied()
        .filter(|view| view.closable())
        .collect();
    CLOSABLE_FEATURES
        .into_iter()
        .filter(|view| present.contains(view))
        .collect()
}

/// 关闭集合解析（旧壳 `parseClosedFeatures` 同语义纯函数）：非数组整体回退
/// 空集合（= 全开）；只留可关九项、去重；输出归一为可关清单序。
pub fn parse_closed_features(raw: &serde_json::Value) -> Vec<ViewName> {
    let Some(arr) = raw.as_array() else {
        return Vec::new();
    };
    let present: Vec<ViewName> = arr
        .iter()
        .filter_map(|item| item.as_str().and_then(ViewName::parse))
        .filter(|v| v.closable())
        .collect();
    CLOSABLE_FEATURES
        .into_iter()
        .filter(|v| present.contains(v))
        .collect()
}

/// 组清单载荷序列化（旧壳 `saveSidebarOrders` / `saveContainmentLists` 同形状：
/// 对象「组 id → 视图名数组」；键与值同旧壳词表，持久化层不另立形状）。
pub fn group_lists_to_value(lists: &GroupLists) -> serde_json::Value {
    let entry = |gid: GroupId| {
        serde_json::Value::Array(
            lists
                .get(gid)
                .iter()
                .map(|v| serde_json::Value::String(v.as_str().to_owned()))
                .collect(),
        )
    };
    serde_json::json!({
        GroupId::Bookkeeping.as_str(): entry(GroupId::Bookkeeping),
        GroupId::Assets.as_str(): entry(GroupId::Assets),
        GroupId::Insights.as_str(): entry(GroupId::Insights),
    })
}

// ---------------------------------------------------------------------------
// 守卫落位裁决与导航请求（旧壳路由守卫族的进程内重生：URL query 双写互踩在
// 进程内退化为独立字段——容器页签归容器、定时形态页签独立、定位参数单点携带）。
// ---------------------------------------------------------------------------

/// 定时视图形态页签（旧壳 `ScheduledFormTab` 同词表；页签内容与图标归定时
/// 迁移票，本壳只承载意图字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduledFormTab {
    /// 订阅。
    Subscriptions,
    /// 分期。
    Installments,
    /// 定额转账。
    Transfers,
}

impl ScheduledFormTab {
    /// 持久化名（旧壳 query 值同词）。
    pub fn as_str(self) -> &'static str {
        match self {
            ScheduledFormTab::Subscriptions => "subscriptions",
            ScheduledFormTab::Installments => "installments",
            ScheduledFormTab::Transfers => "transfers",
        }
    }

    /// 形态页签名 → 意图（未知名 None）。
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "subscriptions" => Some(ScheduledFormTab::Subscriptions),
            "installments" => Some(ScheduledFormTab::Installments),
            "transfers" => Some(ScheduledFormTab::Transfers),
            _ => None,
        }
    }
}

/// 导航请求（进程内导航 API 的载荷单点）：目标视图 + 容器页签意图 + 定时形态
/// 页签意图 + 实体定位参数 + 交易筛选下钻载荷。旧壳 URL query 的通道在此各归
/// 独立字段——「容器 query.tab 归容器、形态页签叠加通道、focus 一名一义、
/// 筛选参数归 URL 参数表」的旧约定在进程内不再有双写互踩面。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavRequest {
    /// 落点目标（视图或组「更多」容器）。
    pub target: ViewName,
    /// 容器页签意图（目标为容器时有义）。
    pub container_tab: Option<ViewName>,
    /// 定时形态页签意图（目标为定时——直达或引用时有义）。
    pub form_tab: Option<ScheduledFormTab>,
    /// 实体定位参数（引用判据：非空串在场；空串与缺席同义，旧壳
    /// `readFocusId` 空串语义同源）。
    pub focus: Option<String>,
    /// 交易筛选下钻载荷（#2037）：目标为交易列表时以载荷为准全量替换筛选
    /// 维度（旧壳 URL 参数表的进程内等价；载荷缺席 = 不动保留态，旧壳「无参
    /// 裸导航回保留态」同口径）。
    pub tx_filters: Option<crate::tx_filter::DrillFilters>,
    /// 投资页下钻载荷（#2047 持仓下钻）：目标为投资视图时携带页签指向与明细
    /// 筛选维（旧壳 `?tab=detail&account=&instrument=` URL 参数表的进程内等
    /// 价；载荷缺席 = 不动保留态）。focus 字段承载的标的定位（标的页签
    /// anchor 分页 + 高亮）走 focus 通道，本字段只承载「落明细页签 + 筛选维」
    /// ——两通道各归独立字段，无双写互踩。
    pub investments_drill: Option<InvestmentsDrill>,
}

/// 投资页下钻载荷（#2047 持仓下钻，ADR-0135 决策 6 的接缝通道）：目标页签
/// （值域 = 投资页签 key，旧壳 `tab=detail` 既有词）+ 明细筛选维（账户 /
/// 标的，二者均可缺席——缺席维度不动保留态，旧壳「参数在场永远赢、缺席
/// 维度保留保留态」同口径）。纯数据、零业务判定：页签指向的合法性与消费
/// （定位形式随页签闭集收窄）归投资视图侧。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvestmentsDrill {
    /// 目标页签（旧壳 URL `tab` 值同词：`detail` = 明细页签；其余值由消费
    /// 侧视为不在场）。
    pub tab: &'static str,
    /// 明细筛选·账户维（缺席 = 不动保留态）。
    pub account_id: Option<String>,
    /// 明细筛选·标的维（缺席 = 不动保留态）。
    pub instrument_id: Option<String>,
}

impl NavRequest {
    /// 裸访问（侧栏点击 / 键位带 / 启动恢复 / 容器页签点击）。
    pub fn bare(target: ViewName) -> Self {
        Self {
            target,
            container_tab: None,
            form_tab: None,
            focus: None,
            tx_filters: None,
            investments_drill: None,
        }
    }

    /// 引用访问（带实体定位参数；空串归缺席）。
    pub fn focus(target: ViewName, entity_id: &str) -> Self {
        Self::bare(target).with_focus(entity_id)
    }

    /// 容器落位（可选页签意图）。
    pub fn container(target: ViewName, tab: Option<ViewName>) -> Self {
        Self::bare(target).with_container_tab(tab)
    }

    /// 容器落位 + 页签 + 定位参数（下钻接缝的容器分流形态）。
    pub fn container_tab_focus(target: ViewName, tab: ViewName, entity_id: &str) -> Self {
        Self::container(target, Some(tab)).with_focus(entity_id)
    }

    /// 附实体定位参数（空串归缺席；链式构造）。
    pub fn with_focus(mut self, entity_id: &str) -> Self {
        self.focus = (!entity_id.is_empty()).then(|| entity_id.to_owned());
        self
    }

    /// 附容器页签意图（链式构造）。
    pub fn with_container_tab(mut self, tab: Option<ViewName>) -> Self {
        self.container_tab = tab;
        self
    }

    /// 附交易筛选下钻载荷（链式构造，#2037 下钻接缝的筛选通道）。
    pub fn with_tx_filters(mut self, filters: crate::tx_filter::DrillFilters) -> Self {
        self.tx_filters = Some(filters);
        self
    }

    /// 附投资页下钻载荷（链式构造，#2047 持仓下钻的接缝通道）。
    pub fn with_investments_drill(mut self, drill: InvestmentsDrill) -> Self {
        self.investments_drill = Some(drill);
        self
    }
}

/// 守卫问询面（守卫裁决的两轴状态：关闭集合与收纳清单）。旧壳守卫内惰性解
/// 析 store 实例的本壳等价物——调用方从导航状态快照构造，纯函数只读快照。
#[derive(Clone, Debug)]
pub struct NavFacts {
    /// 已关闭功能集合（闭集内，清单序）。
    closed: Vec<ViewName>,
    /// 每组收纳清单（收纳在册 = 名在任一组清单中）。
    containment: GroupLists,
}

impl NavFacts {
    /// 从状态快照构造。
    pub fn new(closed: Vec<ViewName>, containment: GroupLists) -> Self {
        Self {
            closed,
            containment,
        }
    }

    /// 出厂快照（全开 + 出厂种子收纳）。
    pub fn factory() -> Self {
        Self::new(Vec::new(), GroupLists::factory(FactoryLists::Containment))
    }

    /// 关闭态查询：不可关项与未知恒 false（旧壳 `isFeatureClosed` 同语义）。
    pub fn is_closed(&self, view: ViewName) -> bool {
        view.closable() && self.closed.contains(&view)
    }

    /// 收纳在册查询：名在任一组当前收纳清单中（旧壳 `isViewContained` 同语义）。
    pub fn is_contained(&self, view: ViewName) -> bool {
        self.containment.contains_any(view)
    }

    /// 某组收纳清单（容器页签合法性的问询面）。
    pub fn containment_list(&self, gid: GroupId) -> &[ViewName] {
        self.containment.get(gid)
    }
}

/// 路由守卫的开关挂靠（旧壳路由表 beforeEnter featureRouteGuard 参数面的
/// 镜像，#2055）：某视图并入的功能自有路由——守卫按挂靠功能的关闭态裁决
///（旧壳 cross-book-summary 路由调 featureRouteGuard("investments")：投资
/// 汇总随投资开关随行关闭，ADR-0114 决策 6）。无挂靠视图返回 None（守卫只
/// 看自身可关态与收纳分流）。
fn guard_feature(view: ViewName) -> Option<ViewName> {
    match view {
        ViewName::CrossBookSummary => Some(ViewName::Investments),
        _ => None,
    }
}

/// 守卫的生效关闭态（旧壳 featureRouteGuard 的参数挂靠：挂靠功能关闭 =
/// 宿主路由关闭；复用原判语义：不可关项恒 false［closable 闭集内联在
/// is_closed］，挂靠面只看可关九项自身）。
fn effectively_closed(view: ViewName, facts: &NavFacts) -> bool {
    facts.is_closed(view) || guard_feature(view).is_some_and(|feature| facts.is_closed(feature))
}

/// 守卫裁决落位（最终落点）：目标视图 + 容器页签意图（目标为容器时有义，
/// 非法或不可见页签归 None = 容器默认首签）。

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Landing {
    /// 最终落位视图。
    pub view: ViewName,
    /// 容器页签意图（None = 默认首签）。
    pub container_tab: Option<ViewName>,
}

/// 收纳分流配置（旧壳路由表 containment 参数的镜像：仅三个出厂收纳视图的
/// 自有路由带分流——定时→记账、保单/保司→资产；其余被收视图裸开仍落自有
/// 路由，旧壳路由表同状态）。
fn containment_redirect(view: ViewName) -> Option<GroupId> {
    match view {
        ViewName::Scheduled => Some(GroupId::Bookkeeping),
        ViewName::Policies | ViewName::Insurers => Some(GroupId::Assets),
        _ => None,
    }
}

/// 守卫落位裁决（旧壳 `featureRouteGuard` + `groupMoreTabGuard` 的进程内合并，
/// 语义逐条对齐）：
/// 1. 关闭态裸访问回概览（概览不可关恒安全落点——骨架期出厂落点见
///    [`resolve_restored`] 注）；带实体定位参数的引用放行自有路由。
/// 2. 开放态且被收纳且在分流配置内 → 改道组「更多」对应页签（focus 与形态
///    页签透传不丢）。
/// 3. 容器页签指向已关闭功能 → 改道该功能自有路由二次裁决（裸回概览、
///    focus 放行；容器页签丢弃，形态页签独立字段自然随行）。
/// 4. 容器页签合法性收窄：仅在清单且可见时保留，否则容器默认首签。
///
/// 改道链最深两跳（容器→功能→概览），循环有界兜底。
pub fn resolve_landing(mut req: NavRequest, facts: &NavFacts) -> Landing {
    for _ in 0..3 {
        if req.target.is_group_more() {
            let Some(gid) = GROUPS.iter().copied().find(|g| g.more_view() == req.target) else {
                break;
            };
            // 容器页签守卫：页签指向已关闭功能 → 改道自有路由二次裁决。
            if let Some(tab) = req.container_tab
                && facts.is_closed(tab)
            {
                req = NavRequest {
                    target: tab,
                    container_tab: None,
                    form_tab: req.form_tab.take(),
                    focus: req.focus.take(),
                    tx_filters: req.tx_filters.take(),
                    investments_drill: req.investments_drill.take(),
                };
                continue;
            }
            // 页签合法性收窄（在清单且可见；否则默认首签）。
            let tab = req
                .container_tab
                .filter(|t| facts.containment_list(gid).contains(t) && !facts.is_closed(*t));
            return Landing {
                view: req.target,
                container_tab: tab,
            };
        }
        // 功能守卫裁决一：关闭态——裸回概览、引用放行（关闭态含挂靠面：
        // 投资关闭时投资汇总随行关闭，#2055）。
        if effectively_closed(req.target, facts) {
            return if req.focus.is_some() {
                Landing {
                    view: req.target,
                    container_tab: None,
                }
            } else {
                Landing {
                    view: ViewName::Dashboard,
                    container_tab: None,
                }
            };
        }
        // 功能守卫裁决二：收纳分流（仅分流配置内视图）。
        if let Some(gid) = containment_redirect(req.target)
            && facts.is_contained(req.target)
        {
            req = NavRequest {
                target: gid.more_view(),
                container_tab: Some(req.target),
                form_tab: req.form_tab.take(),
                focus: req.focus.take(),
                tx_filters: req.tx_filters.take(),
                investments_drill: req.investments_drill.take(),
            };
            continue;
        }
        return Landing {
            view: req.target,
            container_tab: None,
        };
    }
    Landing {
        view: ViewName::Dashboard,
        container_tab: None,
    }
}

/// 出厂落点（概览——旧壳默认路由第一屏）：无记录/未知名/已关闭的启动恢复与
/// 出厂落点同归此处。骨架期曾落交易列表（唯一已迁移视图的安全兑底，#2032）；
/// 概览迁入后由 #2039 改回概览——它同时是守卫回退的安全落点（概览不可关），
/// 与旧壳「关闭恢复/未知名回概览」语义自然重合。
pub const DEFAULT_VIEW: ViewName = ViewName::Dashboard;

/// 启动恢复裁决（旧壳 `resolveRestoredViewName` + 路由守卫两道接线的合并）：
/// 已存名先过守卫（被收纳视图落容器，与裸访问同路径）；未知名回出厂落点；
/// 关闭视图的恢复落守卫改道后的概览（出厂落点与守卫安全落点同为概览，本
/// 分支即旧壳同语义：关闭 → 概览）。
pub fn resolve_restored(saved: Option<&str>, facts: &NavFacts) -> ViewName {
    let Some(view) = saved.and_then(ViewName::parse) else {
        return DEFAULT_VIEW;
    };
    let landing = resolve_landing(NavRequest::bare(view), facts);
    if landing.view == ViewName::Dashboard && view != ViewName::Dashboard {
        DEFAULT_VIEW
    } else {
        landing.view
    }
}

// ---------------------------------------------------------------------------
// 跳转目标计算（旧壳 source-jump.ts `resolveSourceJumpTarget` 的进程内重生：
// 来源列点击 → 导航目标的唯一计算点，后续消费票只调本函数不各写一遍）。
// ---------------------------------------------------------------------------

/// 来源类型闭集（旧壳 `TransactionSourceKind` 同词：交易行来源 wire 契约的
/// 消费侧镜像；装配层产出的来源种类名与此对齐）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// 分期计划。
    InstallmentPlan,
    /// 订阅。
    Subscription,
    /// 定额转账。
    ScheduledTransfer,
    /// 保单。
    Policy,
    /// 物品。
    Item,
    /// 投资标的。
    Instrument,
}

/// 来源 → 目标视图与形态页签（闭集穷尽映射，无分支——新增来源种类编译器
/// 强制补行；旧壳 `KIND_TARGETS` 同表）。
fn source_kind_target(kind: SourceKind) -> (ViewName, Option<ScheduledFormTab>) {
    match kind {
        SourceKind::InstallmentPlan => (ViewName::Scheduled, Some(ScheduledFormTab::Installments)),
        SourceKind::Subscription => (ViewName::Scheduled, Some(ScheduledFormTab::Subscriptions)),
        SourceKind::ScheduledTransfer => (ViewName::Scheduled, Some(ScheduledFormTab::Transfers)),
        SourceKind::Policy => (ViewName::Policies, None),
        SourceKind::Item => (ViewName::Items, None),
        SourceKind::Instrument => (ViewName::Investments, None),
    }
}

/// 跳转目标计算：六类来源 × 主项/收纳/关闭三态 → 导航请求。收纳态落所在组
/// 「更多」对应页签（容器页签归容器）；关闭态跳过收纳分流改落自有路由（关闭
/// 的功能不进组「更多」页签，旧壳 ADR-0116 决策 3/4）；全部目标一律携带实体
/// 定位参数（focus 一名一义）。
pub fn resolve_source_jump(
    kind: SourceKind,
    entity_id: &str,
    is_contained: impl Fn(ViewName) -> bool,
    is_closed: impl Fn(ViewName) -> bool,
) -> NavRequest {
    let (view, form_tab) = source_kind_target(kind);
    if is_contained(view)
        && !is_closed(view)
        && let Some(gid) = group_of_view(view)
    {
        return NavRequest {
            target: gid.more_view(),
            container_tab: Some(view),
            form_tab,
            focus: (!entity_id.is_empty()).then(|| entity_id.to_owned()),
            tx_filters: None,
            investments_drill: None,
        };
    }
    NavRequest {
        target: view,
        container_tab: None,
        form_tab,
        focus: (!entity_id.is_empty()).then(|| entity_id.to_owned()),
        tx_filters: None,
        investments_drill: None,
    }
}

// ---------------------------------------------------------------------------
// 移动纯函数（旧壳 sidebar-order.ts 移动函数族的重生：只见数据、不见 store，
// 写路径把找组、在册守卫、双存储同步内化在导航状态侧）。
// ---------------------------------------------------------------------------

/// 排序动作：组内上移一位 / 下移一位 / 移到组内顶部 / 移到组内底部。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortAction {
    /// 上移一位。
    Up,
    /// 下移一位。
    Down,
    /// 移到组内顶部。
    Top,
    /// 移到组内底部。
    Bottom,
}

/// 移动纯函数：在给定组内序中把 name 移到目标位置，返回新数组（不改输入）。
/// 已在对应边界或不在序中时内容不变（旧壳 `moveArrangeable` 同语义）。
pub fn move_arrangeable(order: &[ViewName], name: ViewName, action: SortAction) -> Vec<ViewName> {
    let Some(index) = order.iter().position(|v| *v == name) else {
        return order.to_vec();
    };
    let last = order.len().saturating_sub(1);
    let target = match action {
        SortAction::Up => index.saturating_sub(1),
        SortAction::Down => index + 1,
        SortAction::Top => 0,
        SortAction::Bottom => last,
    };
    if target == index || target > last {
        return order.to_vec();
    }
    let mut next = order.to_vec();
    next.remove(index);
    next.insert(target, name);
    next
}

/// 移入纯函数：主项名追加为本组收纳清单尾，返回新清单（不改输入）；已在
/// 清单中时内容不变（旧壳 `moveIntoContainment` 同语义；清单序 = 页签序，
/// 移入即该组「更多」最后一个页签）。
pub fn move_into_containment(lists: &GroupLists, gid: GroupId, name: ViewName) -> GroupLists {
    if lists.get(gid).contains(&name) {
        return lists.clone();
    }
    let mut next = lists.clone();
    let mut list = next.get(gid).to_vec();
    list.push(name);
    next.set(gid, list);
    next
}

/// 移回纯函数：从本组收纳清单删除成员，返回新清单（不改输入）；成员不在
/// 清单时内容不变（旧壳 `moveBackToSidebar` 同语义）。
pub fn move_back_to_sidebar(lists: &GroupLists, gid: GroupId, name: ViewName) -> GroupLists {
    if !lists.get(gid).contains(&name) {
        return lists.clone();
    }
    let mut next = lists.clone();
    next.set(
        gid,
        lists
            .get(gid)
            .iter()
            .copied()
            .filter(|v| *v != name)
            .collect(),
    );
    next
}

/// 组满判定纯函数：本组主项数已达硬上限——「移回侧栏」菜单置灰与写路径拒写
/// 的同一判定面（旧壳 `isGroupFull` 同语义）。
pub fn is_group_full(order: &[ViewName]) -> bool {
    order.len() >= GROUP_MAIN_LIMIT
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 词表闭集：解析往返恒等（持久化名与旧壳路由 name 同词的钉子——改词即红，
    /// 提示同步旧壳 view_state 载荷形状）。
    #[test]
    fn view_names_round_trip_through_old_shell_route_names() {
        for view in ALL {
            assert_eq!(ViewName::parse(view.as_str()), Some(view));
        }
        assert_eq!(ViewName::parse("nope"), None);
        assert_eq!(ViewName::parse(""), None);
    }

    /// 出厂词表钉子：三组主项与旧壳 SIDEBAR_GROUPS 数组一致（含攒钱在资产组
    /// 第三位），每组主项硬上限 3（⌘1–⌘9 键位封闭的本体）。
    #[test]
    fn factory_sidebar_groups_match_old_shell() {
        let groups = sidebar_groups();
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].0, GroupId::Bookkeeping);
        assert_eq!(
            groups[0].1,
            &[ViewName::Transactions, ViewName::Accounts, ViewName::Budget]
        );
        assert_eq!(
            groups[1].1,
            &[
                ViewName::Investments,
                ViewName::Items,
                ViewName::SavingsGoals
            ]
        );
        assert_eq!(groups[2].1, &[ViewName::Reports, ViewName::Search]);
        assert_eq!(GROUP_MAIN_LIMIT, 3);
    }

    /// 出厂收纳种子钉子：记账 [定时, 商户]、资产 [保单, 实物资产, 保司]、
    /// 洞察空（清单序 = 「更多」页签序）。
    #[test]
    fn factory_containment_seeds_match_old_shell() {
        assert_eq!(
            containment_seeds(GroupId::Bookkeeping),
            &[ViewName::Scheduled, ViewName::Merchants]
        );
        assert_eq!(
            containment_seeds(GroupId::Assets),
            &[
                ViewName::Policies,
                ViewName::PhysicalAssets,
                ViewName::Insurers
            ]
        );
        assert_eq!(containment_seeds(GroupId::Insights), &[]);
    }

    /// 可关闭集钉子：可关九项为真、不可关六项与容器/固定项为假。
    #[test]
    fn closable_matches_old_shell_feature_ids() {
        for v in [
            ViewName::Budget,
            ViewName::Reports,
            ViewName::Scheduled,
            ViewName::Merchants,
            ViewName::Investments,
            ViewName::Items,
            ViewName::Policies,
            ViewName::PhysicalAssets,
            ViewName::Insurers,
        ] {
            assert!(v.closable(), "{} 应可关", v.as_str());
        }
        for v in [
            ViewName::Dashboard,
            ViewName::Transactions,
            ViewName::Accounts,
            ViewName::Search,
            ViewName::Ai,
            ViewName::Settings,
            ViewName::BookkeepingMore,
        ] {
            assert!(!v.closable(), "{} 不可关", v.as_str());
        }
    }

    /// 键位带推导（旧壳 ADR-0065 固定组带）：概览恒 ⌘`、记账 1–3、资产 4–6、
    /// 洞察 7–9、AI 恒 ⌘0、设置恒 ⌘,；出厂主项占 ⌘1–⌘8，⌘9 带内空置。
    #[test]
    fn derive_shortcuts_follows_fixed_group_bands() {
        let orders = default_group_orders();
        let shortcuts = derive_view_shortcuts(&orders, &[]);
        let key_of = |v: ViewName| {
            shortcuts
                .iter()
                .find(|s| s.view == v)
                .map(|s| s.key)
                .unwrap_or_else(|| panic!("{} 缺键位", v.as_str()))
        };
        assert_eq!(key_of(ViewName::Dashboard), '`');
        assert_eq!(key_of(ViewName::Transactions), '1');
        assert_eq!(key_of(ViewName::Accounts), '2');
        assert_eq!(key_of(ViewName::Budget), '3');
        assert_eq!(key_of(ViewName::Investments), '4');
        assert_eq!(key_of(ViewName::Items), '5');
        assert_eq!(key_of(ViewName::SavingsGoals), '6');
        assert_eq!(key_of(ViewName::Reports), '7');
        assert_eq!(key_of(ViewName::Search), '8');
        assert_eq!(key_of(ViewName::Ai), '0');
        assert_eq!(key_of(ViewName::Settings), ',');
        // ⌘9 带内空置：无视图持有 '9'。
        assert!(
            !shortcuts.iter().any(|s| s.key == '9'),
            "出厂 ⌘9 空置（洞察组主项不足 3，带内空键待命）"
        );
    }

    /// 组内重排键位带内随动：预算移到记账组首位 → ⌘1，交易退 ⌘2。
    #[test]
    fn reordered_members_shift_keys_within_band() {
        let mut orders = default_group_orders();
        orders.set(
            GroupId::Bookkeeping,
            vec![ViewName::Budget, ViewName::Transactions, ViewName::Accounts],
        );
        let shortcuts = derive_view_shortcuts(&orders, &[]);
        let key_of = |v: ViewName| {
            shortcuts
                .iter()
                .find(|s| s.view == v)
                .map(|s| s.key)
                .unwrap_or_else(|| panic!("{} 缺键位", v.as_str()))
        };
        assert_eq!(key_of(ViewName::Budget), '1');
        assert_eq!(key_of(ViewName::Transactions), '2');
        assert_eq!(key_of(ViewName::Accounts), '3');
    }

    /// 已关闭功能退出键位表但保留带内下标（空键待命，不跨组压缩，旧壳
    /// `deriveViewShortcuts` 同语义）：关闭预算 → ⌘3 无视图持有、账户仍 ⌘2；
    /// 关闭报表 → 搜索仍 ⌘8（洞带不前移）。
    #[test]
    fn closed_features_exit_keys_without_compressing_bands() {
        let orders = default_group_orders();
        let closed = [ViewName::Budget, ViewName::Reports];
        let shortcuts = derive_view_shortcuts(&orders, &closed);
        assert!(!shortcuts.iter().any(|s| s.view == ViewName::Budget));
        assert!(!shortcuts.iter().any(|s| s.view == ViewName::Reports));
        let key_of = |v: ViewName| {
            shortcuts
                .iter()
                .find(|s| s.view == v)
                .map(|s| s.key)
                .unwrap_or_else(|| panic!("{} 缺键位", v.as_str()))
        };
        assert_eq!(key_of(ViewName::Accounts), '2');
        assert!(!shortcuts.iter().any(|s| s.key == '3'), "⌘3 空键待命");
        assert_eq!(key_of(ViewName::Search), '8', "洞带不因报表关闭前移");
    }

    /// 键位提示文案（旧壳 shortcutHint 同形）：Apple 平台 ⌘、其余 ⌃。
    #[test]
    fn shortcut_hint_uses_platform_symbol() {
        assert_eq!(shortcut_hint('1', false), "⌃1");
        assert_eq!(shortcut_hint('1', true), "⌘1");
        assert_eq!(shortcut_hint('`', false), "⌃`");
    }

    // -----------------------------------------------------------------------
    // 持久化载荷解析防御（旧壳 parseGroupOrders / parseContainmentLists /
    // parseClosedFeatures 的重生；载荷形状 = 旧壳 view_state:* 同形 JSON）。
    // -----------------------------------------------------------------------

    /// 组内序解析：脏形状（非对象/数组/标量）整体回出厂序；非法名过滤、去重、
    /// 缺失项按默认序补尾；组值非数组则该组整体回默认（他组不受牵连）；收纳
    /// 成员不入组内序（合法性过滤与缺失补尾双排除）。
    #[test]
    fn parse_group_orders_defends_dirty_payloads() {
        let contained = GroupLists::factory(FactoryLists::Containment);
        // 非对象整体回默认。
        for raw in [
            serde_json::json!(null),
            serde_json::json!(["transactions"]),
            serde_json::json!("x"),
            serde_json::json!(42),
        ] {
            assert_eq!(
                parse_group_orders(&raw, &contained),
                default_group_orders(),
                "非对象输入整体回出厂序"
            );
        }
        // 非法名过滤（未知/固定项/他组成员/非字符串）+ 去重 + 缺失补尾。
        let raw = serde_json::json!({
            "bookkeeping": ["budget", "budget", "dashboard", "reports", 7, "transactions"],
            "assets": "dirty",
            "insights": []
        });
        let parsed = parse_group_orders(&raw, &contained);
        assert_eq!(
            parsed.get(GroupId::Bookkeeping),
            &[ViewName::Budget, ViewName::Transactions, ViewName::Accounts]
        );
        // assets 组值非数组：该组整体回默认，他组不受牵连。
        assert_eq!(
            parsed.get(GroupId::Assets),
            default_group_orders().get(GroupId::Assets)
        );
        // insights 空数组：合法项全缺失，按默认序补尾。
        assert_eq!(
            parsed.get(GroupId::Insights),
            default_group_orders().get(GroupId::Insights)
        );
    }

    /// 收纳成员不复活：清单在册成员被排除在组内序合法池与缺失补尾之外
    /// （旧壳 issue #474：清单是收纳成员资格的唯一事实源）。
    #[test]
    fn parse_group_orders_excludes_contained_members() {
        let contained = GroupLists::factory(FactoryLists::Containment);
        let raw = serde_json::json!({
            "bookkeeping": ["scheduled", "transactions", "merchants"]
        });
        let parsed = parse_group_orders(&raw, &contained);
        assert_eq!(
            parsed.get(GroupId::Bookkeeping),
            &[ViewName::Transactions, ViewName::Accounts, ViewName::Budget]
        );
    }

    /// 移回豁免联动：种子被移回侧栏后（清单不含、组内序含），组内序解析把它
    /// 当合法主项保留。
    #[test]
    fn parse_group_orders_accepts_moved_back_seeds() {
        let mut contained = GroupLists::factory(FactoryLists::Containment);
        contained.set(GroupId::Bookkeeping, vec![ViewName::Merchants]);
        let raw = serde_json::json!({
            "bookkeeping": ["transactions", "scheduled"]
        });
        let parsed = parse_group_orders(&raw, &contained);
        assert_eq!(
            parsed.get(GroupId::Bookkeeping),
            &[
                ViewName::Transactions,
                ViewName::Scheduled,
                ViewName::Accounts,
                ViewName::Budget
            ]
        );
    }

    /// 收纳清单解析：脏形状整体回出厂种子；合法池 = 种子 + 本组主项（不跨组）
    /// ；非法名过滤、去重保首现；缺失种子按出厂序补尾。
    #[test]
    fn parse_containment_lists_defends_dirty_payloads() {
        // 非对象整体回种子。
        for raw in [
            serde_json::json!(null),
            serde_json::json!([]),
            serde_json::json!("x"),
        ] {
            assert_eq!(
                parse_containment_lists(&raw, None),
                GroupLists::factory(FactoryLists::Containment)
            );
        }
        let raw = serde_json::json!({
            "bookkeeping": ["merchants", "policies", "merchants", "nope"],
            "insights": "dirty"
        });
        let parsed = parse_containment_lists(&raw, None);
        // 保单属资产组（不跨组）被过滤；商户去重保首现；定时缺失补尾。
        assert_eq!(
            parsed.get(GroupId::Bookkeeping),
            &[ViewName::Merchants, ViewName::Scheduled]
        );
        assert_eq!(
            parsed.get(GroupId::Assets),
            containment_seeds(GroupId::Assets)
        );
        assert_eq!(parsed.get(GroupId::Insights), &[]);
    }

    /// 移回豁免（旧壳 issue #475）：清单在且未列某种子、而原始组内序数组含它
    /// = 已移回主项，不按「缺失补尾」复活；清单存储缺失时不豁免。
    #[test]
    fn parse_containment_lists_honors_move_back_exemption() {
        let raw_orders = serde_json::json!({
            "bookkeeping": ["transactions", "accounts", "budget", "scheduled"]
        });
        let raw = serde_json::json!({
            "bookkeeping": ["merchants"]
        });
        let parsed = parse_containment_lists(&raw, Some(&raw_orders));
        assert_eq!(parsed.get(GroupId::Bookkeeping), &[ViewName::Merchants]);
        // 原始组内序缺失：不豁免，种子照常补尾。
        let parsed = parse_containment_lists(&raw, None);
        assert_eq!(
            parsed.get(GroupId::Bookkeeping),
            &[ViewName::Merchants, ViewName::Scheduled]
        );
    }

    /// 关闭集合解析：非数组整体回全开（空集合）；只留可关九项、去重；输出
    /// 归一为可关清单序（稳定可复现）。
    #[test]
    fn parse_closed_features_defends_dirty_payloads() {
        for raw in [
            serde_json::json!(null),
            serde_json::json!("budget"),
            serde_json::json!({"0": "budget"}),
        ] {
            assert_eq!(parse_closed_features(&raw), Vec::<ViewName>::new());
        }
        let raw = serde_json::json!([
            "insurers",
            "budget",
            "dashboard",
            "nope",
            "insurers",
            3,
            "transactions"
        ]);
        assert_eq!(
            parse_closed_features(&raw),
            vec![ViewName::Budget, ViewName::Insurers]
        );
    }

    /// 关闭集合序列化与解析往返恒等（设置卡写路径的载荷形状）：写出的数组只
    /// 含可关九项、顺序归一为可关清单序；解析回读一致（settings 卡写 → 重启
    /// 读同形）。
    #[test]
    fn closed_features_round_trip_through_json_payload() {
        // 乱序输入 → 归一为可关清单序。
        assert_eq!(
            normalize_closed_features(&[ViewName::Insurers, ViewName::Budget, ViewName::Reports,]),
            vec![ViewName::Budget, ViewName::Reports, ViewName::Insurers,]
        );
        // 序列化 → 解析往返恒等。
        let closed = [ViewName::Budget, ViewName::Items, ViewName::PhysicalAssets];
        let value = closed_features_to_value(&closed);
        assert_eq!(
            value,
            serde_json::json!(["budget", "items", "physicalAssets"])
        );
        assert_eq!(
            parse_closed_features(&value),
            normalize_closed_features(&closed)
        );
        // 不可关项不入载荷（写路径防御：闭集外输入不落盘）。
        let value = closed_features_to_value(&[ViewName::Settings, ViewName::Accounts]);
        assert_eq!(value, serde_json::json!([]));
    }

    /// 载荷序列化与解析往返恒等（写 → 读一致；JSON 键 = 旧壳组 id 同词）。
    #[test]
    fn group_lists_round_trip_through_json_payload() {
        let orders = default_group_orders();
        let value = group_lists_to_value(&orders);
        assert_eq!(
            value,
            serde_json::json!({
                "bookkeeping": ["transactions", "accounts", "budget"],
                "assets": ["investments", "items", "savingsGoals"],
                "insights": ["reports", "search"]
            })
        );
        let contained = GroupLists::factory(FactoryLists::Containment);
        assert_eq!(
            parse_group_orders(&group_lists_to_value(&orders), &contained),
            orders
        );
        assert_eq!(
            parse_containment_lists(&group_lists_to_value(&contained), None),
            contained
        );
    }
    // -----------------------------------------------------------------------
    // 守卫落位裁决（旧壳 featureRouteGuard + groupMoreTabGuard +
    // resolveRestoredViewName 的进程内重生）。
    // -----------------------------------------------------------------------

    /// 快照构造器：指定关闭集合（收纳 = 出厂种子）。
    fn facts_with_closed(closed: &[ViewName]) -> NavFacts {
        NavFacts::new(
            closed.to_vec(),
            GroupLists::factory(FactoryLists::Containment),
        )
    }

    /// 快照构造器：指定某视图收纳在册（追加进指定组清单，全开）。
    fn facts_contained_with(view: ViewName, gid: GroupId) -> NavFacts {
        let mut containment = GroupLists::factory(FactoryLists::Containment);
        if !containment.contains_any(view) {
            let mut list = containment.get(gid).to_vec();
            list.push(view);
            containment.set(gid, list);
        }
        NavFacts::new(Vec::new(), containment)
    }

    /// 两轴谓词构造器（跳转目标计算用）：contained / closed 各自独立指定
    /// （收纳 = 空清单起步，不含出厂种子——主项态不受种子影响）。
    fn facts_bool(view: ViewName, contained: bool, closed: bool) -> NavFacts {
        let mut containment = GroupLists::empty();
        if contained {
            containment.set(GroupId::Assets, vec![view]);
        }
        NavFacts::new(if closed { vec![view] } else { Vec::new() }, containment)
    }

    /// 打开态裸访问落位自有视图；关闭态裸访问回概览（旧壳 featureRouteGuard
    /// 裁决一）；关闭态带实体定位参数的引用放行（裁决二，判据「带 focus 的
    /// 访问是引用」；空串不算在场，旧壳 readFocusId 同语义）。
    #[test]
    fn guard_redirects_closed_bare_access_and_admits_focus_references() {
        let facts = facts_with_closed(&[ViewName::Budget, ViewName::Investments]);
        // 打开态裸访问。
        assert_eq!(
            resolve_landing(NavRequest::bare(ViewName::Accounts), &facts).view,
            ViewName::Accounts
        );
        // 关闭态裸访问 → 概览。
        assert_eq!(
            resolve_landing(NavRequest::bare(ViewName::Budget), &facts).view,
            ViewName::Dashboard
        );
        // 关闭态带定位参数 → 放行自有路由（focus 透传）。
        assert_eq!(
            resolve_landing(NavRequest::focus(ViewName::Investments, "inst-1"), &facts).view,
            ViewName::Investments
        );
        // 空串 focus = 不在场（旧壳 readFocusId：空串按缺席）。
        assert_eq!(
            resolve_landing(NavRequest::focus(ViewName::Budget, ""), &facts).view,
            ViewName::Dashboard
        );
    }

    /// 路由守卫的开关挂靠（旧壳路由表 beforeEnter 的 featureRouteGuard("investments")
    /// 参数面，#2055）：投资汇总随投资开关随行关闭——投资关闭时汇总裸开回概览、
    /// 启动恢复同守卫；投资关闭时撤销守卫映射判据（映射删除即红，测试在
    /// headless 与本表双承载）。汇总自身不可关（不入可关九项），关闭面只挂
    /// 映射功能。
    #[test]
    fn cross_book_summary_closure_follows_investments_feature() {
        let facts = facts_with_closed(&[ViewName::Investments]);
        // 关闭投资 → 汇总裸开回概览。
        assert_eq!(
            resolve_landing(NavRequest::bare(ViewName::CrossBookSummary), &facts).view,
            ViewName::Dashboard
        );
        // 关闭投资 → 汇总恢复也落概览（旧壳「关闭恢复回概览」重合）。
        assert_eq!(
            resolve_restored(Some("cross-book-summary"), &facts),
            ViewName::Dashboard
        );
        // 投资开放 → 汇总落自有路由。
        let facts = facts_with_closed(&[]);
        assert_eq!(
            resolve_landing(NavRequest::bare(ViewName::CrossBookSummary), &facts).view,
            ViewName::CrossBookSummary
        );
    }

    /// 收纳分流（旧壳 featureRouteGuard containment 配置：定时/保单/保司三视图）：
    /// 开放态且被收纳 → 改道组「更多」对应页签，focus 与形态页签透传；关闭态
    /// 跳过分流（引用直达自有路由）。
    #[test]
    fn guard_reroutes_contained_configured_views_to_group_more() {
        let facts = facts_with_closed(&[]);
        let landing = resolve_landing(NavRequest::bare(ViewName::Scheduled), &facts);
        assert_eq!(landing.view, ViewName::BookkeepingMore);
        assert_eq!(landing.container_tab, Some(ViewName::Scheduled));
        // 保单带 focus：容器页签归容器，focus 透传不丢（旧壳 /policies 守卫注释同语义）。
        let landing = resolve_landing(NavRequest::focus(ViewName::Policies, "p-1"), &facts);
        assert_eq!(landing.view, ViewName::AssetsMore);
        assert_eq!(landing.container_tab, Some(ViewName::Policies));
        // 未配置收纳分流的视图（物品）被收纳后裸开仍落自有路由（旧壳路由表
        // 同状态：仅三个出厂收纳视图带 containment 配置）。
        let contained = facts_contained_with(ViewName::Items, GroupId::Assets);
        let landing = resolve_landing(NavRequest::bare(ViewName::Items), &contained);
        assert_eq!(landing.view, ViewName::Items);
        assert_eq!(landing.container_tab, None);
        // 关闭态优先于收纳分流：定时关闭后带 focus 直达自有路由。
        let facts = facts_with_closed(&[ViewName::Scheduled]);
        let landing = resolve_landing(NavRequest::focus(ViewName::Scheduled, "s-1"), &facts);
        assert_eq!(landing.view, ViewName::Scheduled);
    }

    /// 容器落位：页签合法且可见 → 页签意图保留；页签指向已关闭功能 → 改道该
    /// 功能自有路由，由功能守卫二次裁决（裸 → 概览、focus → 放行，旧壳
    /// groupMoreTabGuard 同语义）；页签非法 → 默认首签（None 意图）。
    #[test]
    fn guard_resolves_container_tabs_and_reroutes_closed_tab_targets() {
        let facts = facts_with_closed(&[ViewName::Merchants]);
        // 合法可见页签。
        let landing = resolve_landing(
            NavRequest::container(ViewName::BookkeepingMore, Some(ViewName::Scheduled)),
            &facts,
        );
        assert_eq!(landing.view, ViewName::BookkeepingMore);
        assert_eq!(landing.container_tab, Some(ViewName::Scheduled));
        // 页签指向已关闭功能：改道自有路由（裸 → 概览）。
        let landing = resolve_landing(
            NavRequest::container(ViewName::BookkeepingMore, Some(ViewName::Merchants)),
            &facts,
        );
        assert_eq!(landing.view, ViewName::Dashboard);
        // 页签指向已关闭功能带 focus：改道自有路由且放行（引用不落空）。
        let landing = resolve_landing(
            NavRequest::container_tab_focus(ViewName::BookkeepingMore, ViewName::Merchants, "m-1"),
            &facts,
        );
        assert_eq!(landing.view, ViewName::Merchants);
        // 非法页签（不在清单）→ 默认首签。
        let landing = resolve_landing(
            NavRequest::container(ViewName::BookkeepingMore, Some(ViewName::Items)),
            &facts,
        );
        assert_eq!(landing.view, ViewName::BookkeepingMore);
        assert_eq!(landing.container_tab, None);
    }

    /// 启动恢复裁决（旧壳 resolveRestoredViewName + 路由守卫两条路径的合并）：
    /// 无记录/未知名 → 出厂落点（概览——旧壳默认路由第一屏，#2039 改回）；
    /// 已关闭 → 守卫回概览（出厂落点与守卫安全落点重合）；开放态走守卫
    /// （被收纳的定时 → 组「更多」页签）。
    #[test]
    fn restore_resolves_unknown_closed_and_contained_last_views() {
        let facts = facts_with_closed(&[ViewName::Budget]);
        assert_eq!(resolve_restored(None, &facts), ViewName::Dashboard);
        assert_eq!(resolve_restored(Some("nope"), &facts), ViewName::Dashboard);
        assert_eq!(
            resolve_restored(Some("budget"), &facts),
            ViewName::Dashboard
        );
        assert_eq!(
            resolve_restored(Some("accounts"), &facts),
            ViewName::Accounts
        );
        let facts = facts_with_closed(&[]);
        // 被收纳视图恢复走守卫：落容器（旧壳 restore replace 后守卫同路径）。
        assert_eq!(
            resolve_restored(Some("policies"), &facts),
            ViewName::AssetsMore
        );
        // 容器名恢复原样（旧壳 afterEach 保存的是重定向后的最终路由名）。
        assert_eq!(
            resolve_restored(Some("bookkeeping-more"), &facts),
            ViewName::BookkeepingMore
        );
    }

    // -----------------------------------------------------------------------
    // 跳转目标计算（旧壳 source-jump.ts resolveSourceJumpTarget 的进程内重生）。
    // -----------------------------------------------------------------------

    /// 六类来源 × 三态（主项/收纳/关闭）：计划三形态携形态页签；收纳态落组
    /// 「更多」对应页签（容器页签归容器）；关闭态跳过收纳分流落自有路由；
    /// 全部目标一律携带实体定位参数。
    #[test]
    fn source_jump_targets_cover_kinds_states_and_focus() {
        // 按用例构造一致状态快照（两轴谓词同源，与生产侧同形）。
        fn jump_with(kind: SourceKind, facts: &NavFacts) -> NavRequest {
            resolve_source_jump(
                kind,
                "s-1",
                |v| facts.is_contained(v),
                |v| facts.is_closed(v),
            )
        }

        // 计划三形态（主项态）：定时独立路由 + 形态页签 + focus。
        let main_facts = facts_bool(ViewName::Scheduled, false, false);
        for (kind, form) in [
            (SourceKind::InstallmentPlan, ScheduledFormTab::Installments),
            (SourceKind::Subscription, ScheduledFormTab::Subscriptions),
            (SourceKind::ScheduledTransfer, ScheduledFormTab::Transfers),
        ] {
            let target = jump_with(kind, &main_facts);
            assert_eq!(target.target, ViewName::Scheduled);
            assert_eq!(target.form_tab, Some(form));
            assert_eq!(target.focus.as_deref(), Some("s-1"));
        }
        // 收纳态：落组「更多」对应页签，形态页签随行（旧壳 scheduledTab 叠加
        // 通道在进程内退化为独立字段，无双写互踩）。
        let target = jump_with(
            SourceKind::Subscription,
            &facts_bool(ViewName::Scheduled, true, false),
        );
        assert_eq!(target.target, ViewName::BookkeepingMore);
        assert_eq!(target.container_tab, Some(ViewName::Scheduled));
        assert_eq!(target.form_tab, Some(ScheduledFormTab::Subscriptions));
        // 保单收纳态：资产「更多」保单页签。
        let target = jump_with(
            SourceKind::Policy,
            &facts_bool(ViewName::Policies, true, false),
        );
        assert_eq!(target.target, ViewName::AssetsMore);
        assert_eq!(target.container_tab, Some(ViewName::Policies));
        // 关闭态跳过收纳分流：改落自有路由（旧壳 ADR-0116 决策 4）。
        let target = jump_with(
            SourceKind::Instrument,
            &facts_bool(ViewName::Investments, true, true),
        );
        assert_eq!(target.target, ViewName::Investments);
        assert_eq!(target.container_tab, None);
        // 主项态直达。
        let target = jump_with(SourceKind::Item, &facts_bool(ViewName::Items, false, false));
        assert_eq!(target.target, ViewName::Items);
    }

    // -----------------------------------------------------------------------
    // 移动纯函数（旧壳 moveArrangeable / moveIntoContainment /
    // moveBackToSidebar / isGroupFull 的重生）。
    // -----------------------------------------------------------------------

    /// 排序动作四向与边界 no-op：首位上移/移顶、末位下移/移底内容不变；
    /// 不在序中的名原样返回。
    #[test]
    fn move_arrangeable_moves_within_bounds_and_no_ops_at_edges() {
        let order = [ViewName::Transactions, ViewName::Accounts, ViewName::Budget];
        assert_eq!(
            move_arrangeable(&order, ViewName::Transactions, SortAction::Up),
            order.to_vec()
        );
        assert_eq!(
            move_arrangeable(&order, ViewName::Transactions, SortAction::Top),
            order.to_vec()
        );
        assert_eq!(
            move_arrangeable(&order, ViewName::Budget, SortAction::Down),
            order.to_vec()
        );
        assert_eq!(
            move_arrangeable(&order, ViewName::Budget, SortAction::Bottom),
            order.to_vec()
        );
        assert_eq!(
            move_arrangeable(&order, ViewName::Reports, SortAction::Up),
            order.to_vec()
        );
        assert_eq!(
            move_arrangeable(&order, ViewName::Accounts, SortAction::Up),
            vec![ViewName::Accounts, ViewName::Transactions, ViewName::Budget]
        );
        assert_eq!(
            move_arrangeable(&order, ViewName::Accounts, SortAction::Bottom),
            vec![ViewName::Transactions, ViewName::Budget, ViewName::Accounts]
        );
    }

    /// 移入/移回清单与组满判定：移入追加尾部（已在清单 no-op 保位）；移回删除
    /// （不在清单 no-op）；主项数达硬上限即组满。
    #[test]
    fn containment_move_helpers_append_remove_and_detect_fullness() {
        let lists = GroupLists::factory(FactoryLists::Containment);
        let moved_in = move_into_containment(&lists, GroupId::Insights, ViewName::Search);
        assert_eq!(moved_in.get(GroupId::Insights), &[ViewName::Search]);
        // 已在清单：内容不变。
        assert_eq!(
            move_into_containment(&moved_in, GroupId::Insights, ViewName::Search),
            moved_in
        );
        assert_eq!(
            move_back_to_sidebar(&moved_in, GroupId::Insights, ViewName::Search),
            lists
        );
        assert_eq!(
            move_back_to_sidebar(&lists, GroupId::Insights, ViewName::Search),
            lists
        );
        assert!(is_group_full(&[
            ViewName::Transactions,
            ViewName::Accounts,
            ViewName::Budget
        ]));
        assert!(!is_group_full(&[ViewName::Transactions]));
    }
}
