//! 日期选择器封装（issue #2034；`App*` 封装族，ADR-0118 纪律的 NSH 延续——
//! 旧壳 `AppDatePicker` 的 NSH 对应件，视图不直连框架基元）。
//!
//! 形态：触发件（可键入的日期输入框，显示选中日 `YYYY-MM-DD`）+ 锚定日历弹层
//! （消费 nsh-base `DatePicker` 行为件：日历网格、键盘移动、外部按压 / ESC
//! 关闭）。两条输入路并存（spec #2031 用户故事 2）：日历点选、输入框直接键入
//! （完整 `YYYY-MM-DD` 即落值，宽容零填充；解析收口 [`parse_date_text`]
//! 单点）。交易日期无时刻语义：值域是本地日历日（`chrono::NaiveDate`，域写入
//! 契约的 `YYYY-MM-DD` 原样承载）。
//!
//! 弹层纪律：开/关上报 Overlay Suppression 注册表（[`crate::overlay`]，每实例
//! 一个 token；effect 追踪受控 open 信号，全关闭路径覆盖），组件卸载时 Drop
//! 自动撤销。本封装零业务语义：值归调用方信号持有。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{DatePickerValue, date_picker, input};
use nsh_core::prelude::*;

use crate::overlay;

/// 日期文本 → 本地日历日（键入口径的单点解析）：完整 `YYYY-MM-DD` 即落值，
/// 宽容零填充（`2026-1-5` 合法）；空白 / 残缺 / 非法日期返回 None（不静默落
/// 旧值，呈现由触发件文本原样保留）。与域写入契约 `YYYY-MM-DD` 同形。
pub fn parse_date_text(text: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok()
}

/// 日期选择器：`selected` 是选中日信号（调用方持有、读写皆可；缺省 = 今天，
/// 旧壳 NDatePicker 缺省同语义）。日历点选与键入落值都写 `selected`。
pub fn date_select(id: &'static str, selected: Signal<chrono::NaiveDate>) -> impl IntoElement {
    let open = signal(false);
    // 键入中的原样文本（受控输入跟随键入；完整日期落值后由回显 effect 归一）。
    let typed = signal(selected.get().to_string());
    // 主题令牌（双态自适应；令牌缺失回退亮灰/中灰可读兜底，先例同形）。
    let tokens = nsh_core::element::tokens();
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    let muted = tokens
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into());

    // 开/关上报（Overlay Suppression 注册表单一通道：追踪受控 open 信号，全
    // 关闭路径覆盖——点选 / ESC / 点外都经信号回写）；组件卸载时 Drop 自动撤销。
    let token = overlay::OverlayToken::new();
    effect(move || {
        let shown = open.get();
        token.report(shown);
    });

    // 键入落值（解析单点）：完整日期才写 `selected`；触发件文本原样保留（不
    // 静默改写键入中的残缺文本）。
    let on_type = move |value: &str| {
        typed.set(value.to_owned());
        if let Some(date) = parse_date_text(value) {
            selected.set(date);
        }
    };
    // 选中日 → 触发件回显（覆盖日历点选与程序化置值两条路径；键入路径文本已
    // 与归一形态一致或尚未完整，完整键入经 parse 落值后回显归一零填充）。
    effect(move || {
        typed.set(selected.get().to_string());
    });

    date_picker()
        // 受控值（调用方信号是唯一事实源；锚点落在触发件输入框上——组件元素
        // 无 test_id 槽，先例同形）。
        .value(move || DatePickerValue::Controlled(selected.get().into()))
        .open(move || Some(open.get()))
        .on_open_change(move |e: &nsh_base::DatePickerEvent| {
            if let nsh_base::DatePickerEvent::OpenChange(shown, _) = *e {
                open.set(shown);
            }
        })
        .on_change(move |picked: &nsh_base::PickedDate| {
            if let Some(at) = picked.value.start() {
                selected.set(at.date());
            }
        })
        .trigger(
            view()
                .flex_row()
                .items_center()
                .min_w(px(160.0))
                .child(
                    input()
                        .test_id(format!("{id}-input").as_str())
                        .value(move || typed.get())
                        .optional_label("交易日期")
                        .placeholder("YYYY-MM-DD")
                        .text_color(fg)
                        .app_class(
                            "w-full bg-background rounded-md border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(on_type),
                )
                .child(text("▾").text_size(px(11.0)).text_color(muted)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_dates_parse_leniently() {
        assert_eq!(
            parse_date_text("2026-03-15"),
            chrono::NaiveDate::from_ymd_opt(2026, 3, 15),
            "完整 ISO 日期落值"
        );
        assert_eq!(
            parse_date_text("2026-3-5"),
            chrono::NaiveDate::from_ymd_opt(2026, 3, 5),
            "宽容零填充（键入中途的短形态）"
        );
        assert_eq!(
            parse_date_text(" 2026-03-15 "),
            chrono::NaiveDate::from_ymd_opt(2026, 3, 15),
            "两端空白容差"
        );
    }

    #[test]
    fn incomplete_or_invalid_typed_dates_do_not_land() {
        assert_eq!(parse_date_text(""), None, "空串不落值");
        assert_eq!(
            parse_date_text("2026-03"),
            None,
            "残缺不落值（文本原样保留）"
        );
        assert_eq!(parse_date_text("2026-03-15x"), None);
        assert_eq!(parse_date_text("abc"), None);
        assert_eq!(parse_date_text("2026-02-30"), None, "不存在的日历日拒绝");
        assert_eq!(parse_date_text("2026-13-01"), None);
    }
}
