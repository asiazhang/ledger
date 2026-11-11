//! 危险确认对话框（issue #2022；`App*` 封装族首批件之一，ADR-0078 分级语义的
//! 原生壳重建，旧壳 `AppDangerConfirmModal` 的 NSH 对应件）。
//!
//! 两级形态（ADR-0078 决策 2，判据是「错误执行的代价与可回退性」）：
//! - **error 级**（不可逆，或回退需依赖用户无感知的隐藏通道）：红色警示块 +
//!   加粗后果句（后果说明必选，调用方保证）+ 红色确认按钮；
//! - **warning 级**（破坏性但有兜底或影响可再操作）：琥珀色警示块与确认按钮，
//!   警示句承载后果与兜底说明。
//!
//! 标题、说明段与确认/取消文案全部由调用方传入（调用方持词表），本封装不持
//! 业务语义；两级形态都是本封装的构造面（分级是封装件的一部分），交易删除
//! （不可逆，旧壳确认文案自述「删除后不可恢复」）在本壳消费 error 级。
//!
//! 弹层纪律继承旧壳 AppModal 薄封装（ADR-0078 决策 1 / #252 弹层关闭语义）：
//! 遮罩点击不关（遮罩吞掉点击，不构成关闭意图）、ESC 可关（确认/取消/ESC 三
//! 路径，ESC 与取消同为 [`DangerOutcome::Dismissed`]）、开/关上报 Overlay
//! Suppression 注册表（[`crate::overlay`]，快捷键抑制零新机制，ADR-0035）。
//!
//! 呈现面（应用内模态，非系统对话框——ADR-0078 决策 1「原生 confirm 清零」
//! 在本壳同样成立）：遮罩层内居中卡片。分级色全部走主题令牌（issue #2100
//! 收编：error = `destructive`，warning = `warning`——令牌表补档后本模块
//! 零本地色常量，警示句前景色经 [`crate::theme::color`] 单点消费）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::button;
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::overlay;

/// 分级（ADR-0078 决策 2 的形态闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DangerLevel {
    /// 不可逆，或回退需依赖用户无感知的隐藏通道，或核心数据面临不可读风险。
    Error,
    /// 破坏性但有兜底或影响可再操作。
    Warning,
}

/// 确认对话框的结局（三路径：确认按钮 / 取消按钮 / ESC——后两者同为放弃）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DangerOutcome {
    /// 用户点了确认。
    Confirmed,
    /// 用户放弃（取消按钮或 ESC）。
    Dismissed,
}

/// 对话框的内容与文案（调用方持词表，本封装零 i18n）。
#[derive(Debug, Clone)]
pub struct DangerConfirmSpec {
    /// 分级（形态随级切换）。
    pub level: DangerLevel,
    /// 标题。
    pub title: String,
    /// 说明段（警示块上方的灰字，如状态变化的直接后果）。
    pub lead: Option<String>,
    /// 加粗警示句：error 级为后果说明（后果说明必选）；warning 级为后果与
    /// 兜底说明。
    pub strong_warning: Option<String>,
    /// 补充说明段（警示块下方灰字）。
    pub detail: Option<String>,
    /// 确认按钮文案。
    pub confirm_label: String,
    /// 取消按钮文案。
    pub cancel_label: String,
}

/// 分级 → 呈现参数（纯映射单点，两级构造面闭集；UI 只消费）。前景色走
/// 主题令牌（issue #2100 收编：色值住 theme.json 令牌表，读取经
/// [`crate::theme::color`] 单点，缺失落可读兜底）——本映射只持令牌键与
/// 令牌类，零本地色常量（分级语义的测试面由此成立）。
struct DangerPalette {
    /// 警示块的容器类（警示块底色与描边随级）。
    alert_classes: &'static str,
    /// 警示句与确认按钮前景色的主题令牌键（nsh-base 缺省主题双态都有）。
    accent_token: &'static str,
    /// 确认按钮文字的主题令牌键（实色填充上的前景，令牌表双态同白）。
    accent_foreground_token: &'static str,
    /// 确认按钮的容器类（error 红 / warning 琥珀实色，令牌类）。
    confirm_classes: &'static str,
}

fn palette(level: DangerLevel) -> DangerPalette {
    match level {
        DangerLevel::Error => DangerPalette {
            alert_classes: "rounded-md border border-destructive/30 bg-destructive/10 p-3",
            accent_token: "destructive",
            accent_foreground_token: "destructive-foreground",
            confirm_classes: "bg-destructive hover:opacity-90",
        },
        DangerLevel::Warning => DangerPalette {
            alert_classes: "rounded-md border border-warning/30 bg-warning/10 p-3",
            accent_token: "warning",
            accent_foreground_token: "warning-foreground",
            confirm_classes: "bg-warning hover:opacity-90",
        },
    }
}

/// 危险确认对话框（应用内模态）：`open` 是受控开合（读闭包，追踪信号），
/// 结局经 `on_outcome` 回给调用方（调用方负责关窗——set 回 None——与执行实际
/// 动作）。遮罩不关、ESC 可关、开/关上报抑制注册表。
pub fn danger_confirm(
    open: impl Fn() -> bool + Copy + 'static,
    spec: DangerConfirmSpec,
    on_outcome: impl Fn(DangerOutcome) + Copy + 'static,
) -> impl IntoElement {
    // 开/关上报（Overlay Suppression 注册表单一通道：追踪受控开合，全路径
    // 覆盖——ESC / 取消 / 确认三条关闭路径都走信号回写）；组件卸载时 Drop
    // 自动撤销（卸载兜底，与期间弹层同形）。
    let token = overlay::OverlayToken::new();
    effect(move || {
        let shown = open();
        token.report(shown);
    });
    let palette = palette(spec.level);
    // 主题令牌（警示句/确认按钮前景色与卡片文字色；缺失统一落中灰可读兜底，
    // 兜底常量全仓唯一住在 crate::theme——本模块零本地色常量）。令牌解析在
    // UI 构建期（引擎线程）进行。
    let accent = crate::theme::color(palette.accent_token);
    let fg = crate::theme::color("foreground");
    let muted = crate::theme::color("muted-foreground");
    let cancel = move || on_outcome(DangerOutcome::Dismissed);
    when(open, move || {
        overlay()
                .inset_0()
                // 遮罩层：半透明黑，遮罩点击不关（吞掉点击，#252 弹层关闭
                // 语义：确认/取消须显式点击）。
                .app_class("bg-black/50")
                .flex_col()
                .items_center()
                .justify_center()
                .on_press(move |_: &PressEvent| {})
                // ESC（弹层关闭语义）→ 放弃。
                .on_cancel(cancel)
                .child(
                    // 卡片（旧壳 cardSize md 分档的近形：限宽、圆角、描边）。
                    view()
                        .test_id("danger-dialog")
                        .flex_col()
                        .gap_3()
                        .app_class("bg-background rounded-md border border-border shadow-lg p-4 w-80")
                        .child(
                            text(spec.title.clone())
                                .text_size(px(16.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(fg),
                        )
                        .children(spec.lead.clone().map(|lead| {
                            text(lead).text_size(px(13.0)).text_color(muted)
                        }))
                        .children(spec.strong_warning.clone().map(|warning| {
                            view()
                                .flex_col()
                                .app_class(palette.alert_classes)
                                .child(
                                    text(warning)
                                        .text_size(px(13.0))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(accent),
                                )
                        }))
                        .children(spec.detail.clone().map(|detail| {
                            text(detail).text_size(px(13.0)).text_color(muted)
                        }))
                        .child(
                            view()
                                .flex_row()
                                .justify_end()
                                .gap_2()
                                .child(
                                    button()
                                        .app_class(
                                            "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent",
                                        )
                                        .child(text(spec.cancel_label.clone()).test_id("danger-cancel").text_color(fg))
                                        .on_press(cancel),
                                )
                                .child(
                                    button()
                                        .app_class(format!(
                                            "px-3 py-1.5 rounded-md {}",
                                            palette.confirm_classes
                                        ))
                                        .child(
                                            text(spec.confirm_label.clone())
                                                .test_id("danger-confirm")
                                                .text_color(crate::theme::color(
                                                    palette.accent_foreground_token,
                                                )),
                                        )
                                        .on_press(move || on_outcome(DangerOutcome::Confirmed)),
                                ),
                        ),
                )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两级形态的构造面闭集（ADR-0078 决策 2）：error 走 destructive 令牌类
    /// 与令牌键，warning 走 warning 令牌类与令牌键（issue #2100 收编后本
    /// 模块零本地色常量）；分级是封装件的一部分——删任一级的映射分支即红。
    #[test]
    fn both_levels_have_distinct_construction_faces() {
        let error = palette(DangerLevel::Error);
        let warning = palette(DangerLevel::Warning);
        assert!(
            error.alert_classes.contains("destructive"),
            "error 级警示块走红色（destructive）令牌类"
        );
        assert!(
            error.confirm_classes.contains("bg-destructive"),
            "error 级确认按钮红色"
        );
        assert!(
            warning.alert_classes.contains("warning"),
            "warning 级警示块走琥珀（warning）令牌类"
        );
        assert!(
            warning.confirm_classes.contains("bg-warning"),
            "warning 级确认按钮琥珀色"
        );
        assert_ne!(
            error.accent_token, warning.accent_token,
            "两级警示句前景色走不同令牌（可区分）"
        );
    }
}
