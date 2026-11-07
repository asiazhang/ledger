//! 交易列表筛选会话态（issue #2037；旧壳 `transactions-session` store 与
//! `useTransactionFilter` 深模块的 NSH 重生——TransactionFilter 完整语义装配层化）。
//!
//! 语义对齐旧壳（ADR-0094 会话内保留 / ADR-0030 过滤状态机 / ADR-0135 主列表
//! kind 收窄 / issue #377 分类三态 / issue #893 统一出口）：
//! - **维度闭集**（旧壳 `TransactionFilters` 六维 + 页码）：类型集合（手动多选，
//!   可选集 = 四通用 kind，ADR-0135）、涉及账户（转出 ∪ 转入 ∪ 出资三端命中，
//!   issue #937）、商户（含软删商户历史交易口径，issue #191）、分类（三态：
//!   精确 id /「仅无分类」哨兵 / 不过滤，issue #377）、期间快照（旧壳
//!   dateFrom/dateTo 的本壳对应物，#2021 起以 [`crate::period::PeriodRange`]
//!   承载）与页码。金额区间不在此闭集——旧壳金额维度属搜索面
//!   （`TransactionSearchFilter`），随搜索态住视图组件（见 [`crate::app`]）。
//! - **会话内保留**：本态住进程级单例信号（`App` 根实例所有），视图卸载重挂
//!   不丢（旧壳「离开交易页再回来 = 回到离开时的样子」）；**零写盘**（不进
//!   Storage、不进 SQLite），冷启动回默认。分类维度无手动控件（旧壳同口径，
//!   URL 下钻只读入口的进程内等价 = 下钻载荷）。
//! - **统一出口**：筛选维度写入口一律「翻页归零」（page = 1）；页码直写归翻页
//!   路径（ADR-0008/0045 分页契约）；同值守卫（同值不动作，旧壳 mutate 同规）。
//! - **下钻全量替换**（旧壳 URL query 语义的进程内等价，#2037 下钻接缝）：
//!   带筛选载荷的落位意图以载荷为准整表替换各下钻维度（载荷未提及的维度复位
//!   ——旧壳「新 query 全量替换旧 query」同构；日期非法视为不在场，与旧壳
//!   「非法回退不过滤」同规）；裸导航（无载荷）不动保留态。
//! - **搜索态分工**（搜索 × 期间裁决，#2037）：期间快照对搜索命中面**生效**
//!   （平价旧壳搜索页 QuickTimeRange：期间写入搜索过滤）；账户/商户/分类/类型
//!   维度仅作用于列表态（域搜索读路径不支持这些维度，立桩现状定型为规格）。
//!
//! 纯决策半边（模块级纯函数，离线单测）在前、反应式外壳（信号单例）在后。
//! 测试豁免（ADR-0060）经 crate 根 cfg(test) 整体放行六件套。

use std::sync::Mutex;

use ledger_transaction::TransactionKind;
use nsh_core::prelude::*;

use crate::period;

/// 主列表类型口径（ADR-0135 / issue #1783，旧壳 `MAIN_LIST_KINDS` 同名单同序）：
/// 主交易列表只呈现四通用 kind；类型下拉可选集 = 本名单；请求默认载荷 = 本名单
/// （显式排除投资 kind 的装配单点在 [`resolve_request_kinds`]）。
pub const MAIN_LIST_KINDS: [TransactionKind; 4] = [
    TransactionKind::Income,
    TransactionKind::Expense,
    TransactionKind::Transfer,
    TransactionKind::Refund,
];

/// 分类过滤维度三态中的两态（issue #377）：精确分类 id（不含子分类，含软删
/// 分类历史交易口径）或「仅无分类」哨兵；`None` = 不过滤（第三态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CategoryFilter {
    /// 精确分类 id。
    Exact(String),
    /// 仅无分类（旧壳 `UNCATEGORIZED_ONLY` 哨兵的枚举化）。
    UncategorizedOnly,
}

/// 交易筛选会话态（维度闭集 + 页码；默认 = 无筛选第 1 页）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxFilterState {
    /// 类型集合（`None` = 默认；`Some` = 手动多选子集，经 [`normalize_kind_selection`]
    /// 归一——空集与满选归 `None`，ADR-0135）。
    pub kinds: Option<Vec<TransactionKind>>,
    /// 涉及账户过滤（账户 id）。
    pub involving_account_id: Option<String>,
    /// 商户过滤（商户 id，含软删）。
    pub merchant_id: Option<String>,
    /// 分类维度三态。
    pub category: Option<CategoryFilter>,
    /// 期间快照（「全部」= `None`）。
    pub range: Option<period::PeriodRange>,
    /// 页码（1 起）。
    pub page: u32,
}

impl Default for TxFilterState {
    fn default() -> Self {
        Self {
            kinds: None,
            involving_account_id: None,
            merchant_id: None,
            category: None,
            range: None,
            // 页码从 1 起（ADR-0008 分页契约；默认态即第 1 页）。
            page: 1,
        }
    }
}

/// 下钻筛选载荷（#2037 下钻接缝的筛选通道；旧壳 URL 参数表六维的进程内等价）。
/// 发送方视图（报表分类下钻 / 商户下钻 / 账户下钻等）造本载荷经
/// [`crate::nav`] 落位投递；字段缺席 = 该维度复位（全量替换语义）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrillFilters {
    /// 涉及账户维度（旧壳 `?account=`）。
    pub account: Option<String>,
    /// 商户维度（旧壳 `?merchant=`）。
    pub merchant: Option<String>,
    /// 分类维度（旧壳 `?category=`，保留值「仅无分类」以
    /// [`CategoryFilter::UncategorizedOnly`] 承载）。
    pub category: Option<CategoryFilter>,
    /// 类型集合（旧壳 `?kinds=` 逗号分隔闭集字面量的类型化形态）。
    pub kinds: Option<Vec<TransactionKind>>,
    /// 日期起（旧壳 `?dateFrom=`；非法视为不在场）。
    pub date_from: Option<String>,
    /// 日期止（旧壳 `?dateTo=`；非法视为不在场）。
    pub date_to: Option<String>,
}

/// 列表请求的类型集合装配（旧壳 `resolveRequestKinds` 同口径）：显式集合在场
/// 按其携带；默认态显式携带四通用 kind——含默认态的显式排除是本单点（主列表
/// 排除投资 kind 的唯一机制，ADR-0135）。
pub fn resolve_request_kinds(kinds: Option<&[TransactionKind]>) -> Vec<TransactionKind> {
    match kinds {
        Some(kinds) if !kinds.is_empty() => kinds.to_vec(),
        _ => MAIN_LIST_KINDS.to_vec(),
    }
}

/// 手动多选归一（旧壳 `normalizeKindSelection` 同口径，ADR-0135）：空集合与
/// 满选四通用 kind 都归 `None`（空集 ≡ 满选 ≡ 不过滤 ≡ 默认态）；子集原样保留
/// （展示序由下拉选项闭集序承担，不在此排序）。
pub fn normalize_kind_selection(values: &[TransactionKind]) -> Option<Vec<TransactionKind>> {
    if values.is_empty() {
        return None;
    }
    if MAIN_LIST_KINDS.iter().all(|k| values.contains(k)) {
        return None;
    }
    Some(values.to_vec())
}

/// 是否有任一激活的筛选维度（旧壳 `filtersActive` 同口径：页码不算筛选——
/// 清除按钮禁用态与空态分型的判定输入）。
pub fn filters_active(state: &TxFilterState) -> bool {
    state.kinds.is_some()
        || state.involving_account_id.is_some()
        || state.merchant_id.is_some()
        || state.category.is_some()
        || state.range.is_some()
}

/// 保留态是否处于默认（无筛选 + 第 1 页；旧壳 `isAtDefault` 同口径，页大小本壳
/// 固定常量不设维）：ESC 复位幂等判定（「无保留状态无操作」）的单一谓词。
pub fn is_at_default(state: &TxFilterState) -> bool {
    !filters_active(state) && state.page == 1
}

/// 下钻载荷 → 全量替换后的筛选态（纯函数）：载荷未提及的维度复位（旧壳「新
/// query 全量替换旧 query」同构）；日期双端齐备且格式合法才落期间快照（单端/
/// 非法 = 无快照，「非法视为不在场」与旧壳日期参数同规）；页码归 1（下钻即
/// 重查第一页）。
pub fn drilled(drill: &DrillFilters) -> TxFilterState {
    let range = match (drill.date_from.as_deref(), drill.date_to.as_deref()) {
        (Some(from), Some(to))
            if period::parse_iso_date(from).is_some() && period::parse_iso_date(to).is_some() =>
        {
            Some(period::PeriodRange {
                from: from.to_owned(),
                to: to.to_owned(),
            })
        }
        _ => None,
    };
    TxFilterState {
        kinds: drill.kinds.clone().filter(|k| !k.is_empty()),
        involving_account_id: drill.account.clone(),
        merchant_id: drill.merchant.clone(),
        category: drill.category.clone(),
        range,
        page: 1,
    }
}

// ---------------------------------------------------------------------------
// 反应式外壳：进程级单例信号束（会话内保留的唯一读写方）。
// ---------------------------------------------------------------------------

/// 导航式进程槽（headless 多场景串行复位 [`reset_for_tests`]；生产进程一次
/// 初始化）。锁毒化时取回内值——槽位本身不含不变量（[`crate::nav`] 同形）。
static STORE: Mutex<Option<TxFilterStore>> = Mutex::new(None);

fn store_slot() -> std::sync::MutexGuard<'static, Option<TxFilterStore>> {
    STORE.lock().unwrap_or_else(|p| p.into_inner())
}

/// 筛选会话态句柄（Copy；信号挂首次 [`store`] 调用所在组件实例——生产路径
/// 契约：`App` 组件体最先调用，状态挂根实例，随进程存活、视图卸载不丢）。
/// 各维度独立信号：下拉封装件的受控信号形态（[`crate::pinyin_select`] /
/// [`crate::select`]）可直接消费维度信号，无需第二份镜像态。
#[derive(Clone, Copy)]
pub struct TxFilterStore {
    /// 类型集合（`None` = 默认；写入方先经 [`normalize_kind_selection`] 归一）。
    pub kinds: Signal<Option<Vec<TransactionKind>>>,
    /// 涉及账户维度。
    pub account: Signal<Option<String>>,
    /// 商户维度。
    pub merchant: Signal<Option<String>>,
    /// 分类三态（下钻载荷；无手动控件）。
    pub category: Signal<Option<CategoryFilter>>,
    /// 期间快照（「全部」= `None`；步进/预设/清除共用）。
    pub range: Signal<Option<period::PeriodRange>>,
    /// 页码（翻页路径直写；维度出口的翻页归零归各写入口）。
    pub page: Signal<u32>,
}

/// 筛选会话态（惰性初始化：首次消费建信号，默认态起）。
pub fn store() -> TxFilterStore {
    let mut slot = store_slot();
    if let Some(s) = *slot {
        return s;
    }
    let s = TxFilterStore {
        kinds: signal(None),
        account: signal(None),
        merchant: signal(None),
        category: signal(None),
        range: signal(None),
        page: signal(1u32),
    };
    *slot = Some(s);
    s
}

/// 复位筛选态槽（headless 测试接缝，[`crate::app::reset_book_assembly`] 消费）。
#[doc(hidden)]
pub fn reset_for_tests() {
    *store_slot() = None;
}

impl TxFilterStore {
    /// 当前筛选态快照（非响应式；装载闭包取当下值）。
    pub fn snapshot(&self) -> TxFilterState {
        TxFilterState {
            kinds: self.kinds.get_untracked(),
            involving_account_id: self.account.get_untracked(),
            merchant_id: self.merchant.get_untracked(),
            category: self.category.get_untracked(),
            range: self.range.get_untracked(),
            page: self.page.get_untracked(),
        }
    }

    /// 页码直写（翻页路径；页码从 1 起，下界防御同列表分页契约）。
    pub fn set_page(&self, page: u32) {
        self.page.set(page.max(1));
    }

    /// 下钻全量替换（落位意图消费路径）：以载荷为准整表替换各维度 + 翻页归零
    /// （[drilled] 纯决策单点；信号等值守卫免费继承——同载荷重复落位不触发
    /// 依赖方重算）。
    pub fn apply_drill(&self, drill: &DrillFilters) {
        let next = drilled(drill);
        self.kinds.set(next.kinds);
        self.account.set(next.involving_account_id);
        self.merchant.set(next.merchant_id);
        self.category.set(next.category);
        self.range.set(next.range);
        self.page.set(next.page);
    }

    /// 清除全部筛选（显式动词：清除按钮 / ESC 复位共用出口）——回默认态并翻页
    /// 归零（旧壳 resetFilters 同语义；信号等值不通知的幂等面由 Signal 承担）。
    pub fn reset(&self) {
        let next = TxFilterState::default();
        self.kinds.set(next.kinds);
        self.account.set(next.involving_account_id);
        self.merchant.set(next.merchant_id);
        self.category.set(next.category);
        self.range.set(next.range);
        self.page.set(next.page);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ledger_transaction::TransactionKind;

    fn kinds(values: &[TransactionKind]) -> Vec<TransactionKind> {
        values.to_vec()
    }

    // ---- 类型维度：默认载荷装配（ADR-0135 主列表收窄） ----

    #[test]
    fn resolve_request_kinds_defaults_to_main_list() {
        // 默认态显式携带四通用 kind——投资 kind 的排除是本单点。
        assert_eq!(
            resolve_request_kinds(None),
            kinds(&[
                TransactionKind::Income,
                TransactionKind::Expense,
                TransactionKind::Transfer,
                TransactionKind::Refund,
            ])
        );
        assert_eq!(
            resolve_request_kinds(Some(&[])),
            resolve_request_kinds(None)
        );
    }

    #[test]
    fn resolve_request_kinds_carries_explicit_subset() {
        let subset = kinds(&[TransactionKind::Expense, TransactionKind::Refund]);
        assert_eq!(resolve_request_kinds(Some(&subset)), subset);
    }

    // ---- 类型维度：手动多选归一（ADR-0135） ----

    #[test]
    fn normalize_kind_selection_empty_and_full_collapse_to_default() {
        assert_eq!(normalize_kind_selection(&[]), None);
        let full = kinds(&[
            TransactionKind::Expense,
            TransactionKind::Income,
            TransactionKind::Transfer,
            TransactionKind::Refund,
        ]);
        assert_eq!(normalize_kind_selection(&full), None, "满选 ≡ 不过滤");
    }

    #[test]
    fn normalize_kind_selection_keeps_subset() {
        let subset = kinds(&[TransactionKind::Expense]);
        assert_eq!(normalize_kind_selection(&subset), Some(subset));
    }

    // ---- 激活判定与默认判定 ----

    #[test]
    fn filters_active_reflects_each_dimension_but_not_page() {
        let mut state = TxFilterState::default();
        assert!(!filters_active(&state), "默认无筛选");
        state.page = 3;
        assert!(
            !filters_active(&state),
            "页码不是筛选维度（旧壳 filtersActive 同口径）"
        );
        state.page = 1;
        state.involving_account_id = Some("a-1".to_owned());
        assert!(filters_active(&state));
        state.involving_account_id = None;
        state.merchant_id = Some("m-1".to_owned());
        assert!(filters_active(&state));
        state.merchant_id = None;
        state.category = Some(CategoryFilter::UncategorizedOnly);
        assert!(filters_active(&state));
        state.category = None;
        state.range = Some(period::PeriodRange {
            from: "2026-01-01".to_owned(),
            to: "2026-01-31".to_owned(),
        });
        assert!(filters_active(&state));
        state.range = None;
        state.kinds = Some(kinds(&[TransactionKind::Expense]));
        assert!(filters_active(&state));
    }

    #[test]
    fn is_at_default_requires_no_filter_and_first_page() {
        let mut state = TxFilterState::default();
        assert!(is_at_default(&state));
        state.page = 2;
        assert!(!is_at_default(&state), "翻页即保留态（复位需动作）");
        state.page = 1;
        state.range = Some(period::PeriodRange {
            from: "2026-01-01".to_owned(),
            to: "2026-01-31".to_owned(),
        });
        assert!(!is_at_default(&state));
    }

    // ---- 下钻全量替换（#2037 下钻接缝载荷语义） ----

    #[test]
    fn drill_replaces_every_mentioned_dimension_and_resets_the_rest() {
        // 保留态：账户 + 类型先在场（仅作叙事；纯函数不读旧态——全量替换）。
        let _previous = TxFilterState {
            kinds: Some(kinds(&[TransactionKind::Income])),
            involving_account_id: Some("acc-old".to_owned()),
            merchant_id: Some("m-old".to_owned()),
            category: Some(CategoryFilter::Exact("c-old".to_owned())),
            range: Some(period::PeriodRange {
                from: "2025-01-01".to_owned(),
                to: "2025-12-31".to_owned(),
            }),
            page: 4,
        };
        // 载荷只提及商户 + 类型子集（旧壳「新 query 全量替换旧 query」同构：
        // 未提及的维度复位）。
        let state = drilled(&DrillFilters {
            merchant: Some("m-new".to_owned()),
            kinds: Some(kinds(&[TransactionKind::Expense, TransactionKind::Refund])),
            ..DrillFilters::default()
        });
        assert_eq!(state.involving_account_id, None, "未提及 → 复位");
        assert_eq!(state.category, None, "未提及 → 复位");
        assert_eq!(state.range, None, "未提及 → 复位");
        assert_eq!(state.merchant_id.as_deref(), Some("m-new"));
        assert_eq!(
            state.kinds,
            Some(kinds(&[TransactionKind::Expense, TransactionKind::Refund]))
        );
        assert_eq!(state.page, 1, "下钻即重查第一页");
    }

    #[test]
    fn drill_applies_period_only_with_both_valid_bounds() {
        let state = drilled(&DrillFilters {
            date_from: Some("2026-01-01".to_owned()),
            date_to: Some("2026-12-31".to_owned()),
            ..DrillFilters::default()
        });
        assert_eq!(
            state.range,
            Some(period::PeriodRange {
                from: "2026-01-01".to_owned(),
                to: "2026-12-31".to_owned(),
            }),
            "双端合法 → 期间快照落位"
        );

        let single = drilled(&DrillFilters {
            date_from: Some("2026-01-01".to_owned()),
            ..DrillFilters::default()
        });
        assert_eq!(single.range, None, "单端 → 不在场");

        let malformed = drilled(&DrillFilters {
            date_from: Some("2026-13-01".to_owned()),
            date_to: Some("2026-12-31".to_owned()),
            ..DrillFilters::default()
        });
        assert_eq!(malformed.range, None, "非法日期 → 不在场（旧壳同规）");
    }

    #[test]
    fn drill_category_sentinel_and_empty_kind_set_round_trip() {
        let state = drilled(&DrillFilters {
            category: Some(CategoryFilter::UncategorizedOnly),
            kinds: Some(Vec::new()),
            ..DrillFilters::default()
        });
        assert_eq!(state.category, Some(CategoryFilter::UncategorizedOnly));
        assert_eq!(state.kinds, None, "空集合载荷 ≡ 默认（与归一口径一致）");
    }
}
