//! 口径说明气泡封装（`App*` 封装族，issue #2039 首建；ADR-0118 纪律的 NSH
//! 延续——视图不直连框架基元）。
//!
//! 形态：锚定触发件的说明气泡（多行文本，`max-w` 收窄换行），点按触发件开/
//! 再按或 ESC/点外关（nsh-base Popover 行为件：宿主承担 ESC 与点外关闭）。
//! 旧壳指针轴用悬停 tooltip、触控轴用点按气泡（同一文案两轴分面，ADR-0088
//! 决策 6）；本壳桌面档点按形态与旧壳触控轴同构，headless 可测（引擎无
//! hover 步骤，nsh-testing 能力盘点 §1.4）。
//!
//! 弹层纪律：受控开合（本封装持有 open 信号，调用方零状态）——触发件按压
//! 切换、宿主关闭路径（ESC / 点外）经 `on_open_change` 回写；开/关上报
//! Overlay Suppression 注册表（[`crate::overlay`]，每实例一个 token；effect
//! 追踪受控 open 信号，全关闭路径覆盖，受控关闭不产生 Popover 事件故只听
//! 事件会漏报——popconfirm 同款），组件卸载时 Drop 自动撤销。文案全部由
//! 调用方传入（调用方持词表），本封装零业务语义。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{PopoverEvent, popover};
use nsh_core::prelude::*;

use crate::design::font_size;
use crate::overlay;

/// 气泡说明文案锚点后缀（headless `id:` 选择器用；调用方给前缀）。
pub(crate) const MESSAGE_ID_SUFFIX: &str = "-message";

/// 口径说明气泡：触发件点按开合，`lines` 逐行呈现（调用方持词表与行序）。
/// 触发件外包一层按压视图（开气泡的按压源，锚定与命中区以它为准）。`id`
/// 允许动态键（#2048 bar 图逐柱锚点：柱 id 为运行期数据，非 &'static 闭集
/// ——presto headless 锚点互不串扰即可）。
pub fn popover_tip(
    id_prefix: impl Into<String>,
    lines: Vec<String>,
    trigger: impl IntoElement,
) -> impl IntoElement {
    // 受控开合信号 + 开/关上报（Overlay Suppression 注册表单一通道：追踪
    // 受控开合，全路径覆盖——点按 / ESC / 点外三类关闭都走信号）；组件卸载
    // 时 Drop 自动撤销（卸载兜底，期间弹层同形）。
    let open = signal(false);
    let token = overlay::OverlayToken::new();
    let open_for_effect = open.read_only();
    effect(move || {
        token.report(open_for_effect.get());
    });
    let id_prefix = id_prefix.into();
    let message_id = format!("{}{}", id_prefix, MESSAGE_ID_SUFFIX);
    popover()
        .open(move || Some(open.get()))
        .on_open_change(move |e: &PopoverEvent| {
            if let PopoverEvent::OpenChange(shown, _) = *e {
                open.set(shown);
            }
        })
        .popup_class("bg-background border border-border rounded-(--radius) shadow-md p-3")
        .trigger(
            view()
                // test_id 落在按压包裹视图上（headless id: 锚点须命中可按压
                // 件，period-label 先例同形）；内层触发件归调用方自饰。
                .test_id(id_prefix.as_str())
                .on_press(move |_: &PressEvent| {
                    open.set(!open.get_untracked());
                })
                .child(trigger.into_element()),
        )
        .child(
            view()
                .flex_col()
                .gap_1()
                .max_w(px(340.0))
                // 锚点落首行文本（headless id: 选择器匹配文本节点；容器无
                // 文本锚点不可寻址——popconfirm 单行文本同形）。
                .children(lines.into_iter().enumerate().map(|(index, line)| {
                    // 说明行（字阶 CAPTION 档，design.rs font_size 同源——DESIGN.md
                    // §3 「辅助小字」下限档）；前景经 [`crate::theme::color`] 单点
                    // 消费（零本地色常量）。
                    let line_text = text(line)
                        .text_size(px(font_size::CAPTION))
                        .text_color(crate::theme::color("foreground"));
                    if index == 0 {
                        line_text.test_id(message_id.as_str())
                    } else {
                        line_text
                    }
                })),
        )
}
