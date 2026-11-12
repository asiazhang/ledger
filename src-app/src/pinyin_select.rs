//! 拼音可搜下拉（issue #2033；`App*` 封装族，ADR-0118 纪律的 NSH 延续——旧壳
//! `PinyinSelect` 的 NSH 对应件，视图不直连框架基元）。
//!
//! 形态：触发件（显示选中项 label / 占位文案）+ 锚定弹层（搜索输入框自动聚焦
//! + 选项行集）。
//!
//! 过滤语义消费统一模糊搜索语义的域内唯一定义点：`ledger_transaction` 的
//! `search_text`（ADR-0027 全库唯一）。命中判据：原文连续子串（大小写不敏感）
//! 或词条是该字段拼音首字母串的子序列；词条间 AND。投资域下拉（ADR-0113
//! 决策 2 先例）与交易搜索同源，本封装是第三消费方，不在壳内另立第二份拼音
//! 匹配。
//!
//! 弹层纪律：开/关上报 Overlay Suppression 注册表（[`crate::overlay`]，每实例
//! 一个 token；effect 追踪受控 open 信号，全关闭路径覆盖）；ESC / 点外关闭由
//! 弹层宿主承担（[`crate::app`] 期间弹层同款 popover 形态）；选项选中即关并
//! 清查询（下次打开回完整列表——旧壳 NSelect filterable 同语义）。
//!
//! 数据源归调用方（预算视图消费支出分类树选项
//! [`crate::assembly::expense_category_options`]），本封装零业务语义。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{PopoverEvent, button, input, popover};
use nsh_core::prelude::*;

use crate::{assembly::CategoryOption, design::font_size, overlay};

/// 弹层内选项行的稳定锚点后缀（headless `id:` 选择器用；调用方给前缀）。
pub(crate) const OPTION_ID_SUFFIX: &str = "-option";
/// 搜索输入框的稳定锚点后缀。
pub(crate) const SEARCH_ID_SUFFIX: &str = "-search";

/// 统一模糊搜索语义过滤（ADR-0027 唯一定义点的直接消费；查询级组合逻辑住
/// [`ledger_transaction::query_matches_text`] 单点）：空查询恒全量（恢复完整
/// 列表），命中保序。纯函数、可离线单测。
pub fn filter_options(query: &str, options: &[CategoryOption]) -> Vec<CategoryOption> {
    options
        .iter()
        .filter(|option| ledger_transaction::query_matches_text(query, &option.label))
        .cloned()
        .collect()
}

/// 拼音可搜下拉：`id_prefix` 是 headless 锚点前缀（搜索框
/// `{prefix}-search`、选项行 `{prefix}-option`）；动态前缀（表单③子行逐行锚
/// 点）以 `Cow` 承载，静态常量调用点零改动。`options` 是数据源闭包；
/// `selected` 是选中值信号（调用方持有、读写皆可）；`placeholder` 是未选中时
/// 的触发件文案；`clearable` 开启可清空形态（选中时触发件右侧渲染 ✕——独立
/// 按压件，清除落 None 不开弹层；分类等可选字段的「无选择」语义，#2034）。
pub fn pinyin_select(
    id_prefix: impl Into<std::borrow::Cow<'static, str>>,
    options: impl Fn() -> Vec<CategoryOption> + Copy + 'static,
    selected: Signal<Option<String>>,
    placeholder: &'static str,
    clearable: bool,
) -> impl IntoElement {
    let id_prefix = id_prefix.into();
    // 后缀拼接预派生（子行动态前缀：各 move 闭包捕获自己的 String，不共用
    // 一个非 Copy 值避免 move 冲突）。
    let prefix_search = format!("{}{}", id_prefix, SEARCH_ID_SUFFIX);
    let prefix_option = format!("{}{}", id_prefix, OPTION_ID_SUFFIX);
    let prefix_empty = format!("{}-empty", id_prefix);
    let prefix_clear = format!("{}-clear", id_prefix);
    let open = signal(false);
    let query = signal(String::new());
    // 主题令牌（双态自适应）：色值住 theme.json 令牌表，读取经
    // [`crate::theme::color`] 单点，缺失落可读兜底——本模块零本地色常量
    // （issue #2107 视觉收口）。
    let fg = crate::theme::color("foreground");
    let muted = crate::theme::color("muted-foreground");

    // 开/关上报（Overlay Suppression 注册表单一通道：追踪受控 open 信号，全
    // 关闭路径覆盖——ESC / 点外 / 选中三条关闭路径都经信号回写）；组件卸载时
    // Drop 自动撤销（卸载兜底，danger.rs 同形）。
    let token = overlay::OverlayToken::new();
    effect(move || {
        let shown = open.get();
        token.report(shown);
    });

    // 过滤后的选项行集（纯函数单点，弹层打开时随查询响应式）。
    let shown = computed(move || filter_options(&query.get(), &options()));

    // 选中项 label（触发件呈现；选项被过滤掉时按 value 回查全量数据源）。
    let selected_label = move || {
        let id = selected.get()?;
        options()
            .iter()
            .find(|o| o.id == id)
            .map(|o| o.label.clone())
    };

    let choose = move |option: CategoryOption| {
        selected.set(Some(option.id));
        open.set(false);
        query.set(String::new());
    };

    let body = popover()
        .open(move || Some(open.get()))
        .on_open_change(move |e: &PopoverEvent| {
            if let PopoverEvent::OpenChange(shown, _) = *e {
                open.set(shown);
                if !shown {
                    // 关闭即清查询：下次打开回完整列表（旧壳 NSelect 同语义）。
                    query.set(String::new());
                }
            }
        })
        .popup_class("bg-background border border-border rounded-(--radius) shadow-md p-1")
        .trigger(
            view()
                .test_id(id_prefix.as_ref())
                .flex_row()
                .items_center()
                .gap_1()
                .min_w(px(160.0))
                .px_2()
                .py_1()
                // 控件圆角走主题令牌基档（8pt，rounded_md 6pt 规格外值退役——
                // 归入 app_class 单点书写）。
                .app_class("rounded-(--radius) border border-border hover:bg-accent")
                .on_press(move |_: &PressEvent| open.set(true))
                .child(
                    // 触发件文案（字阶 SECONDARY 档，design.rs font_size 同源——
                    // DESIGN.md §3 「菜单/控件标签」档；占位态呈弱化色）。
                    text(move || selected_label().unwrap_or_else(|| placeholder.to_owned()))
                        .text_size(px(font_size::SECONDARY))
                        .text_color(move || {
                            if selected_label().is_some() {
                                fg
                            } else {
                                muted
                            }
                        })
                        .number_of_lines(1),
                )
                .child(
                    text("▾")
                        .text_size(px(font_size::CAPTION))
                        .text_color(muted),
                ),
        )
        .child(
            view()
                .flex_col()
                .gap_1()
                .child(
                    // 搜索框（自动聚焦：打开即输入——旧壳 NSelect filterable 的
                    // 触发件内输入在本壳的弹层内等价形态）。
                    input()
                        .test_id(prefix_search.as_str())
                        .auto_focus(true)
                        .value(move || query.get())
                        .placeholder("输入关键字或拼音首字母")
                        .text_color(fg)
                        .app_class(
                            "bg-background rounded-(--radius) border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(move |text: &str| query.set(text.to_owned())),
                )
                .child(each(
                    move || shown.get(),
                    |o: &CategoryOption| o.id.clone(),
                    move |item: ReadSignal<CategoryOption>, _index: ReadSignal<usize>| {
                        let option = item.get();
                        // 选项载体视图（组件元素无 test_id 槽，period-label 先例
                        // 同形）：载体持稳定锚点，headless 按序号命中行。
                        view().test_id(prefix_option.as_str()).child(
                            button()
                                .app_class(
                                    "w-full px-2 py-1 rounded-(--radius) text-left text-foreground \
                                         hover:bg-accent selected:bg-accent",
                                )
                                .child(
                                    // 选项行（字阶 SECONDARY 档，design.rs font_size 同源）。
                                    text(option.label.clone())
                                        .text_size(px(font_size::SECONDARY))
                                        .text_color(fg),
                                )
                                .on_press(move |_: &()| choose(option.clone())),
                        )
                    },
                ))
                .child(
                    // 空态（无匹配查询）：与旧壳 NSelect 空态同词；字形 = 字阶
                    // CAPTION 档。
                    when(
                        move || shown.get().is_empty(),
                        move || {
                            text("无数据")
                                .text_size(px(font_size::CAPTION))
                                .text_color(muted)
                                .test_id(prefix_empty.as_str())
                                .app_class("px-2 py-1")
                        },
                    )
                    .otherwise(view),
                ),
        );

    // 可清空形态（#2034）：选中时触发件右侧渲染 ✕（独立按压件，清除落 None
    // 不开弹层；与 select.rs / date_select 的清除形态同位）。
    if clearable {
        view().flex_row().items_center().gap_1().child(body).child(
            nsh_base::button()
                .accessibility_label("清除")
                .app_class("px-1 py-0.5 rounded-sm text-muted-foreground hover:bg-accent")
                .child(
                    text("✕")
                        .text_size(px(font_size::CAPTION))
                        .text_color(muted)
                        .test_id(prefix_clear.as_str()),
                )
                .on_press(move |_: &()| selected.set(None)),
        )
    } else {
        view().child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(id: &str, label: &str) -> CategoryOption {
        CategoryOption {
            id: id.to_owned(),
            label: label.to_owned(),
        }
    }

    fn seed() -> Vec<CategoryOption> {
        vec![
            opt("c1", "餐饮"),
            opt("c2", "餐饮 > 午餐"),
            opt("c3", "交通"),
            opt("c4", "工资"),
        ]
    }

    #[test]
    fn empty_query_shows_every_option() {
        let shown = filter_options("", &seed());
        assert_eq!(shown.len(), 4, "空查询恒命中（恢复完整列表）");
    }

    #[test]
    fn raw_substring_hits_case_insensitively() {
        let shown = filter_options("午餐", &seed());
        assert_eq!(
            shown.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
            ["c2"],
            "原文连续子串命中"
        );
        assert_eq!(filter_options("餐", &seed()).len(), 2, "父名命中父与子");
    }

    #[test]
    fn pinyin_initials_subsequence_hits() {
        // 「午餐」首字母串 = wc；词条是首字母串的子序列即命中。
        let shown = filter_options("wc", &seed());
        assert_eq!(
            shown.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
            ["c2"],
            "拼音首字母命中子分类"
        );
        // 「餐饮 > 午餐」首字母串 = cywc（标点与空格跳过）；「cywc」全串命中。
        assert_eq!(filter_options("cywc", &seed())[0].id, "c2");
        // 子序列允许跳字：「cw」= c(y)w(c) 命中。
        assert_eq!(filter_options("cw", &seed())[0].id, "c2");
        // ASCII 保留：「交通」→ jt。
        assert_eq!(filter_options("jt", &seed())[0].id, "c3");
    }

    #[test]
    fn terms_are_anded_across_whitespace() {
        let shown = filter_options("餐饮 午餐", &seed());
        assert_eq!(
            shown.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
            ["c2"],
            "词条之间 AND：两个词条都要命中同一 label"
        );
        assert!(
            filter_options("餐饮 工资", &seed()).is_empty(),
            "无交集即空"
        );
    }

    #[test]
    fn ascii_mixed_input_falls_back_to_raw_substring() {
        // 混合输入（含汉字 + ASCII）对纯首字母串的子序列匹配自然失败，落原文
        // 子串路径（ADR-0027 契约）。
        assert!(filter_options("餐wc", &seed()).is_empty());
        assert_eq!(filter_options("工资 salary", &seed()).len(), 0);
        assert_eq!(filter_options("ABC", &seed()).len(), 0);
    }

    #[test]
    fn no_match_shows_none() {
        assert!(filter_options("不存在的分类", &seed()).is_empty());
    }
}
