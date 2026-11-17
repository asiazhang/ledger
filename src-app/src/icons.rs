//! 视图字形资产单点（#2135 布局对齐线）：ionicons Outline 风格 SVG 字形，
//! 与 Web 参照前端侧栏同源（web `App.vue` `viewIcons` 逐视图一行，@vicons/
//! ionicons5，MIT）——本壳经 `nsh_core::svg()` 原生端重画（feature `svg`，
//! `currentColor` 随元素文字色），消费面取本模块尺寸档（视觉规约图标
//! 14–16pt 两档 + 空态字形例外档，见 [`docs/DESIGN.md`] §6/§7）：侧栏导航
//! 项 [`NAV_ITEM_PT`]、行内小字形 [`INLINE_PT`]、空态字形 [`EMPTY_STATE_PT`]
//! （web NEmpty 插画的同位弱化对应，例外档已挂账）。
//!
//! 资产 = `assets/icons/*.svg`（include_str! 内联，首帧即出、零加载时序）；
//! 提取自 @vicons/ionicons5 0.13.0 产物（逐元素重建，stroke="currentColor"
//! 原样保留，观感随主题亮暗两态）。许可证：ionicons MIT（同仓 web 参照
//! 前端既消费面）；源图取 Outline 闭集，新增字形先补资产再扩映射。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_core::element::{AdaptiveColor, ImageSrc, Svg};
use nsh_core::prelude::*;

use crate::nav_model::ViewName;

/// 侧栏导航项字形尺寸（pt；视觉规约图标档上限，web NIcon 18px 的本壳档）。
pub(crate) const NAV_ITEM_PT: f32 = 16.0;

/// 行内小字形尺寸（pt；组「更多」链接、提示条内嵌图标——视觉规约图标档
/// 下限，贴 text_xs/SECONDARY 文字）。
pub(crate) const INLINE_PT: f32 = 14.0;

/// 空态字形尺寸（pt；视觉规约图标档外的例外档——web NEmpty 出厂插画同位
/// 的弱化中档，挂账 docs/DESIGN.md §7）。
pub(crate) const EMPTY_STATE_PT: f32 = 24.0;

// 字形资产（ionicons Outline；文件名 = ionicons 原名小写连字）。
const HOME: &str = include_str!("../assets/icons/home-outline.svg");
const SWAP_HORIZONTAL: &str = include_str!("../assets/icons/swap-horizontal-outline.svg");
const SEARCH: &str = include_str!("../assets/icons/search-outline.svg");
const WALLET: &str = include_str!("../assets/icons/wallet-outline.svg");
const BAR_CHART: &str = include_str!("../assets/icons/bar-chart-outline.svg");
const TRENDING_UP: &str = include_str!("../assets/icons/trending-up-outline.svg");
const REPEAT: &str = include_str!("../assets/icons/repeat-outline.svg");
const CALCULATOR: &str = include_str!("../assets/icons/calculator-outline.svg");
const FLAG: &str = include_str!("../assets/icons/flag-outline.svg");
const CUBE: &str = include_str!("../assets/icons/cube-outline.svg");
const SHIELD_CHECKMARK: &str = include_str!("../assets/icons/shield-checkmark-outline.svg");
const STOREFRONT: &str = include_str!("../assets/icons/storefront-outline.svg");
const UMBRELLA: &str = include_str!("../assets/icons/umbrella-outline.svg");
const SPARKLES: &str = include_str!("../assets/icons/sparkles-outline.svg");
const SETTINGS: &str = include_str!("../assets/icons/settings-outline.svg");
const LAYERS: &str = include_str!("../assets/icons/layers-outline.svg");
const FILE_TRAY: &str = include_str!("../assets/icons/file-tray-outline.svg");
const INFORMATION_CIRCLE: &str = include_str!("../assets/icons/information-circle-outline.svg");

/// 侧栏导航项字形（web `viewIcons` 同映射闭集；「更多」容器与跨账本汇总
/// 无侧栏字形——web 侧两者均不在 `viewIcons` 清单内）。
pub(crate) fn view_icon(name: ViewName) -> Option<&'static str> {
    let glyph = match name {
        ViewName::Dashboard => HOME,
        ViewName::Transactions => SWAP_HORIZONTAL,
        ViewName::Search => SEARCH,
        ViewName::Accounts => WALLET,
        ViewName::Reports => BAR_CHART,
        ViewName::Investments => TRENDING_UP,
        ViewName::Scheduled => REPEAT,
        ViewName::Budget => CALCULATOR,
        ViewName::SavingsGoals => FLAG,
        ViewName::Items => CUBE,
        ViewName::Policies => SHIELD_CHECKMARK,
        ViewName::Merchants => STOREFRONT,
        ViewName::PhysicalAssets => CUBE,
        ViewName::Insurers => UMBRELLA,
        ViewName::Ai => SPARKLES,
        ViewName::Settings => SETTINGS,
        // web 无侧栏字形（跨账本汇总不入侧栏；三容器是收纳聚合页）。
        ViewName::CrossBookSummary
        | ViewName::BookkeepingMore
        | ViewName::AssetsMore
        | ViewName::InsightsMore => return None,
    };
    Some(glyph)
}

/// 组「更多」链接字形（web `LayersOutline` 同源）。
pub(crate) const GROUP_MORE: &str = LAYERS;

/// 信息提示条字形（web NAlert type=info 出厂图标的同位对应物）。
pub(crate) const INFO_CIRCLE: &str = INFORMATION_CIRCLE;

/// 侧栏导航项字形渲染（currentColor 随元素文字色，消费方显式给
/// `.text_color(...)`——不依赖父链继承色，亮暗两态随主题）。
pub(crate) fn nav_icon(markup: &'static str) -> Svg {
    sized(markup, NAV_ITEM_PT)
}

/// 行内小字形渲染（组「更多」链接、提示条内嵌图标）。
pub(crate) fn inline_icon(markup: &'static str) -> Svg {
    sized(markup, INLINE_PT)
}

/// 空态字形（列表/搜索空态共用：web NEmpty 出厂插画的同位对应物——本壳
/// 不复刻插画，取弱化托盘形，观感自立；亮暗两态随 muted 令牌）。
pub(crate) fn empty_glyph(muted: AdaptiveColor) -> Svg {
    sized(FILE_TRAY, EMPTY_STATE_PT).text_color(muted)
}

/// 字形渲染（尺寸即元素盒；flex_shrink_0 防窄行挤压变形）。
fn sized(markup: &'static str, size: f32) -> Svg {
    svg(ImageSrc::svg_markup(markup))
        .w(px(size))
        .h(px(size))
        .flex_shrink_0()
}
