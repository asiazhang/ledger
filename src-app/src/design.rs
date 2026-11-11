//! 设计常量（issue #2100，ADR-0147 决策 3/4）：字阶、间距、圆角的唯一来源。
//!
//! **双轨蓝图**（ADR-0147 决策 3）：本模块先行止血——视图收口票
//! （#2101–#2107）把裸 `text_size(px(..))`、散写的间距与圆角逐批换到本模块；
//! 壳内 ui 层（一文件一件的封装族，ADR-0118 的 NSH 后继）成熟后，本模块是
//! 其尺寸 API 的取值底座——视图只组合封装件与设计常量，不裸写视觉值。
//!
//! **数值纪律**（ADR-0147 决策 4）：全部采用 NSH 官方设计指南规范值，不发明
//! 第二套数；规约成文（用途 → 取值的对表、同心规则、例外挂账处）见
//! `docs/DESIGN.md`——文档是规约真源，本模块是代码取值唯一来源，两者同源
//! 同值（漂移即文档修正或代码修正，按可验证行为裁决）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

/// 字阶（pt）：官方规范值 12/14/16/18/20 五档。正文基准 = [`font_size::BODY`]；
/// 阅读文字不小于 [`font_size::CAPTION`]；每屏 3–4 个字号、两种字重；可比较
/// 数字（时刻/计数/金额列）用等宽数字 `tabular-nums`。
pub mod font_size {
    /// 辅助小字下限（时间戳、计数、overline）：官方「阅读文字 ≥ 12pt」的档。
    pub const CAPTION: f32 = 12.0;
    /// 次要正文（表格行、表单标签、菜单、按钮）。
    pub const SECONDARY: f32 = 14.0;
    /// 正文基准（官方 text-base = 16pt）。
    pub const BODY: f32 = 16.0;
    /// 小节标题。
    pub const LEADING: f32 = 18.0;
    /// 页面/卡片标题。
    pub const TITLE: f32 = 20.0;
}

/// 间距（pt）：官方语义阶 2/4/8/12/16/24/32 七档。关系递增优先于绝对值：
/// 间距表达「两个东西的关系」，不是像素分配；padding 归组件、gap 归父级，
/// 不重复承担；横向优先 `ps-*`/`me-*` 逻辑边。
pub mod spacing {
    /// 光学校正（图标与文字的微调）。
    pub const HAIRLINE: f32 = 2.0;
    /// 控件部件之间（图标 ↔ 文字）。
    pub const PART: f32 = 4.0;
    /// 近亲控件（同字段内）。
    pub const SIBLING: f32 = 8.0;
    /// 同组（同一卡片内的字段间）。
    pub const GROUP: f32 = 12.0;
    /// 同 section 不同组。
    pub const SECTION: f32 = 16.0;
    /// 不同 section。
    pub const REGION: f32 = 24.0;
    /// 主要区域边界（页面级留白）。
    pub const PAGE: f32 = 32.0;
}

/// 圆角（pt）：阶 8/12/16 + inset 同心四档。普通矩形圆角明显小于短边之半；
/// 全圆（胶囊/圆形）用框架 `rounded-full`，不用本阶；嵌套处内层圆角按同心
/// 规则取（[`radius::concentric`]）或外层 `overflow` 裁剪。
pub mod radius {
    /// 控件/弹层基档（主题令牌 `radius` 同值）。
    pub const CONTROL: f32 = 8.0;
    /// 卡片档（主题令牌 `radius-card` 同值）。
    pub const CARD: f32 = 12.0;
    /// 大面板档（主题令牌 `radius-panel` 同值）。
    pub const PANEL: f32 = 16.0;
    /// inset 同心常用档（主题令牌 `radius-inset` 同值）。
    pub const INSET: f32 = 8.0;

    /// 同心规则（官方原文「内 ≈ max(0, 外 − 缝)」）：内层圆角 = 外层圆角减
    /// 层间缝，取最接近且合适的一档——外 12 卡内嵌 4pt 缝 → 内 8。
    pub fn concentric(outer: f32, gap: f32) -> f32 {
        (outer - gap).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方规范值闭集（ADR-0147 决策 4：不发明第二套数）——任一常量漂移
    /// 出规范值即红。
    #[test]
    fn constants_are_the_official_scale() {
        assert_eq!(
            [
                font_size::CAPTION,
                font_size::SECONDARY,
                font_size::BODY,
                font_size::LEADING,
                font_size::TITLE
            ],
            [12.0, 14.0, 16.0, 18.0, 20.0]
        );
        assert_eq!(
            [
                spacing::HAIRLINE,
                spacing::PART,
                spacing::SIBLING,
                spacing::GROUP,
                spacing::SECTION,
                spacing::REGION,
                spacing::PAGE
            ],
            [2.0, 4.0, 8.0, 12.0, 16.0, 24.0, 32.0]
        );
        assert_eq!(
            [radius::CONTROL, radius::CARD, radius::PANEL, radius::INSET],
            [8.0, 12.0, 16.0, 8.0]
        );
        assert_eq!(
            crate::theme::bundled_theme_radius_tiers(),
            (radius::CONTROL, radius::CARD, radius::PANEL, radius::INSET),
            "主题令牌圆角阶与常量模块同值（design 为唯一来源）"
        );
    }

    /// 同心规则：内 = max(0, 外 − 缝)；缝不大于外时不出现负圆角。
    #[test]
    fn concentric_never_goes_negative() {
        assert_eq!(radius::concentric(12.0, 4.0), 8.0);
        assert_eq!(radius::concentric(8.0, 8.0), 0.0);
        assert_eq!(radius::concentric(8.0, 12.0), 0.0);
    }
}
