//! 交易搜索视图（issue #2040 NSH 迁移⑨；旧壳 `SearchView.vue` 的重生）。
//!
//! **职责边界**（与交易列表的分工按旧壳核实平价）：本视图是洞察组的独立
//! 检索入口——只承载「关键词 + 金额区间 + 期间」三维的只读检索面，无交易
//! 列表的筛选维度（账户/商户/分类/类型，域搜索读路径不支持，#2037 定型为
//! 规格）、无任何写入口（无右键菜单、无表单；编辑走交易列表/表单）。旧壳
//! 搜索页行为面（ADR-0070 决策 3）：关键词防抖自动搜索（300ms 单源，与
//! 交易列表共用 [`crate::app::SEARCH_DEBOUNCE_MS`]）、五枚期间芯片 +
//! 步进器、金额区间（元输入）、已应用筛选描述 + 清除筛选、结果只读分页；
//! **状态源全视图本地、会话内保留明确不做**（离开再回 = 全新占位态——旧壳
//! 同口径，本壳视图切换即卸载，重挂天然归位）。
//!
//! 竞态与加载语义（旧壳 Loadable + useDelayedLoading 的重生）：后发覆盖
//! 先发（装载序号 latest-wins，迟到结果不落位）；快搜索不闪加载指示器
//! （延迟出场 + 最小驻留整流，[`delayed_loading`]）。
//!
//! 结果呈现复用交易列表的表格映射族（列集 / 自绘格 / 弹性列宽 / 部件主题，
//! [`crate::app`]）与装配层行装配（购买项命中整单展开，旧壳搜索页
//! expandPurchases 同口径）；行点击/下钻委托（账户/商户/来源格跳转）属
//! 表格级发送端 #2061（交易列表同缺口，一并落地），本视图先平价只读。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::time::Duration;

use nsh_base::{button, data_table, input, pagination};
use nsh_core::prelude::*;

use crate::app::{amount_privacy_enabled, db_slot, hit_total_text};
use crate::assembly::{self, CurrencyTable, RowCells, RowContext};
use crate::boot;
use crate::nav;
use crate::nav_model::ViewName;
use crate::{adaptive, app, period, period_row, tx_cards};

/// 搜索视图（导航骨架搜索分支的真实组件，#2040）：关键词防抖自动搜索 +
/// 期间快捷筛选 + 金额区间 + 只读结果分页 + 占位/无命中两态空态。
#[component]
pub(crate) fn SearchView() -> impl IntoElement {
    let view_owner = Owner::current();
    // 结果呈现态（视图本地：旧壳搜索页状态源本地、会话内保留明确不做
    // ——ADR-0070 决策 3；视图卸载即回占位态，旧壳重挂同口径）。
    let rows = signal(Vec::<RowCells>::new());
    let total = signal(0i64);
    let page = signal(1u32);
    let loading = signal(false);
    let error = signal(None::<String>);
    // 是否已完成至少一次搜索（区分「占位提示」与「空结果」两种空态，旧壳
    // searched 门控同口径）。
    let searched = signal(false);
    // 输入态：关键词（受控）、金额区间元文本、期间快照（搜索视图本地，
    // 不进交易筛选会话态——ADR-0070 决策 3「不引入 store」同口径）。
    let keyword = signal(String::new());
    let amount_min_text = signal(String::new());
    let amount_max_text = signal(String::new());
    let search_range = signal(None::<period::PeriodRange>);
    let boundary_dates = signal(None::<(Option<String>, Option<String>)>);
    // 装配参考表（随装载读批落位；行装配与已应用筛选描述的消费面）。
    let currencies = signal(CurrencyTable::default());
    // 防抖调度序号（新输入作废在途定时器——关键词/金额/期间共用同一防抖，
    // 旧壳搜索页单 watcher 同形）与装载序号（后发作废先发）。
    let search_seq = signal(0u64);
    let load_seq = signal(0u64);

    // 主题令牌（双态自适应；令牌缺失回退亮灰/中灰可读兜底，先例同形）。
    let tokens = nsh_core::element::tokens();
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    let muted = tokens
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into());

    // 查询具备判定（旧壳 hasQuery 同口径：关键词非空或任一筛选在场——
    // 仅筛选也可出结果）。
    let has_query = move || {
        assembly::search_has_query(
            &keyword.get(),
            assembly::amount_text_to_cents(&amount_min_text.get()),
            assembly::amount_text_to_cents(&amount_max_text.get()),
            search_range.get().as_ref().map(|r| r.from.as_str()),
            search_range.get().as_ref().map(|r| r.to.as_str()),
        )
    };

    // 装载一页（防抖到期、回车与翻页共用一条路径）：恒走搜索读路径（旧壳
    // 搜索页同一命令面——空查询 + 仅筛选 = 仅筛选查询，域语义空结果）。
    // 参考数据行集与数据期间边界随同批读取（行装配与期间步进钳制的消费
    // 面，刷新成本可忽略）；竞态 latest-wins（后发已接管即过期不落位）。
    // 读重活走 blocking 后台线程，任务挂视图实例 owner（弹层触发面按下后
    // 随状态翻转卸载不取消，交易视图先例同形）。
    let load = move |page_no: u32| {
        loading.set(true);
        error.set(None);
        load_seq.update(|seq| *seq += 1);
        let seq = load_seq.get();
        let query = keyword.get().trim().to_owned();
        let amount_min = assembly::amount_text_to_cents(&amount_min_text.get());
        let amount_max = assembly::amount_text_to_cents(&amount_max_text.get());
        let range = search_range.get_untracked();
        let privacy = amount_privacy_enabled();
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let Some(db) = db else {
                    error.set(Some("内部错误：账本库会话缺失".to_owned()));
                    loading.set(false);
                    return;
                };
                let outcome = blocking(move || {
                    boot::transaction_search(
                        &db,
                        &query,
                        page_no,
                        amount_min,
                        amount_max,
                        range.as_ref(),
                    )
                    .map(|result| (result.items, result.total))
                    .and_then(|(items, total)| {
                        let date_range = boot::report_date_range(&db).ok();
                        let currency_rows = boot::currency_rows(&db)?;
                        let category_rows = boot::category_rows(&db)?;
                        let account_rows = boot::account_rows(&db)?;
                        let merchant_rows = boot::merchant_rows(&db)?;
                        Ok((
                            items,
                            total,
                            date_range,
                            currency_rows,
                            category_rows,
                            account_rows,
                            merchant_rows,
                        ))
                    })
                })
                .await;
                // 过期装载不落位（后发已接管 rows/loading，先发不得回写）。
                if load_seq.get() != seq {
                    return;
                }
                match outcome {
                    Ok(Ok((
                        items,
                        total_count,
                        date_range,
                        currency_rows,
                        category_rows,
                        account_rows,
                        merchant_rows,
                    ))) => {
                        let currencies_table = CurrencyTable::from_currencies(&currency_rows);
                        currencies.set(currencies_table.clone());
                        let ctx = RowContext {
                            currencies: &currencies_table,
                            categories: &assembly::CategoryTable::from_categories(&category_rows),
                            accounts: &assembly::AccountTable::from_accounts(&account_rows),
                            merchants: &assembly::MerchantTable::from_merchants(&merchant_rows),
                            privacy_enabled: privacy,
                        };
                        rows.set(assembly::assemble_rows(&items, &ctx));
                        total.set(total_count);
                        boundary_dates.set(date_range.map(|dr| (dr.min_date, dr.max_date)));
                        searched.set(true);
                    }
                    Ok(Err(e)) => error.set(Some(e.to_string())),
                    Err(_) => error.set(Some("内部错误：读任务异常退出".to_owned())),
                }
                loading.set(false);
            });
        });
    };

    // 输入变化统一出口（旧壳单 watcher 同形：关键词/金额/期间任一变化走
    // 同一决策——无查询回占位态并作废在途；有查询防抖重查第一页）。
    let react_to_change = move || {
        if !has_query() {
            // 作废在途防抖与装载（旧壳 clearTimeout + invalidate 同义），
            // 结果态归零回占位。
            search_seq.update(|seq| *seq += 1);
            load_seq.update(|seq| *seq += 1);
            rows.set(Vec::new());
            total.set(0);
            page.set(1);
            searched.set(false);
            loading.set(false);
            return;
        }
        page.set(1);
        search_seq.update(|seq| *seq += 1);
        let seq = search_seq.get();
        // 防抖定时器挂视图实例 owner：触发面（清除按钮 / 期间芯片）按下后
        // 随状态翻转即时自卸载（#2037「按下即自卸载」竞态的本视图变体），
        // 直呼 set_timeout 会把定时器挂到子树 owner 上随卸载被取消——防抖
        // 到期不装载、结果静默不刷新（修复前 headless 清除旅程确定性复现）。
        view_owner.run(move || {
            set_timeout(Duration::from_millis(app::SEARCH_DEBOUNCE_MS), move || {
                if search_seq.get() == seq {
                    load(1);
                }
            });
        });
    };

    let on_keyword_change = move |text: &str| {
        keyword.set(text.to_owned());
        react_to_change();
    };
    let on_amount_min_change = move |text: &str| {
        amount_min_text.set(text.to_owned());
        react_to_change();
    };
    let on_amount_max_change = move |text: &str| {
        amount_max_text.set(text.to_owned());
        react_to_change();
    };

    // 回车立即搜索（不等防抖，旧壳 onEnter 同语义——关键词或筛选任一存在
    // 才动作，无查询回车无操作）。
    let on_submit = move |_: &str| {
        if !has_query() {
            return;
        }
        search_seq.update(|seq| *seq += 1);
        page.set(1);
        load(1);
    };

    // 期间快照应用回调（共用期间行）：翻页归零；快照真的变化走统一出口
    //（防抖语义，旧壳芯片点击经 watcher 防抖重查同形）。
    let (period_row_view, _unused_step) = period_row::period_row(
        "search-period",
        false,
        true,
        search_range,
        boundary_dates,
        move |changed: bool| {
            page.set(1);
            if changed {
                react_to_change();
            }
        },
    );

    // 清除筛选（旧壳 clearFilters 同语义：金额与期间归零，关键词不动）；
    // 清后无查询回占位、有查询防抖重查（统一出口承担）。
    let clear_filters = move || {
        amount_min_text.set(String::new());
        amount_max_text.set(String::new());
        search_range.set(None);
        react_to_change();
    };

    // 筛选激活判定（已应用筛选描述与清除按钮的显隐输入；旧壳 filtersActive
    // 同口径：金额任一端或期间任一端在场）。
    let filters_active = move || {
        assembly::search_filters_active(
            assembly::amount_text_to_cents(&amount_min_text.get()),
            assembly::amount_text_to_cents(&amount_max_text.get()),
            search_range.get().as_ref().map(|r| r.from.as_str()),
            search_range.get().as_ref().map(|r| r.to.as_str()),
        )
    };

    // 下钻意图投递即弃（搜索视图无定位形态——旧壳搜索路由不读 focus 同
    // 语义；读一次封闸语义由 take_intent 单点承担）。
    effect(move || {
        let _current_landed = nav::store().current.get();
        let _ = nav::take_intent();
    });

    // 已应用筛选描述（旧壳 activeFilterDescriptions 同口径拼接：金额双端/
    // 单端、期间起止；文案单点在装配层，拼接与展示分离）。
    let applied_filters_text = move || {
        let parts = assembly::search_filter_descriptions(
            assembly::amount_text_to_cents(&amount_min_text.get_untracked()),
            assembly::amount_text_to_cents(&amount_max_text.get_untracked()),
            search_range
                .get_untracked()
                .as_ref()
                .map(|r| r.from.as_str()),
            search_range.get_untracked().as_ref().map(|r| r.to.as_str()),
        );
        format!("已应用筛选：{}", parts.join("、"))
    };

    // 响应式列集（备注列吸收富余宽，交易列表同形）与延迟出场加载指示
    //（快搜索不闪指示器，旧壳 useDelayedLoading 语义重建）。
    let columns = computed(move || app::elastic_columns(adaptive::content_width()));
    let visible_loading = delayed_loading(loading);

    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(
            // 屏名（内容区标题，同容器壳形态）。
            view().px_4().py_2().child(
                text(ViewName::Search.label())
                    .text_size(px(16.0))
                    .text_color(fg)
                    .test_id("view-title"),
            ),
        )
        .child(
            // 关键词行（旧壳 search.keywordPlaceholder 同文案；回车立即搜索）。
            view()
                .flex_row()
                .px_4()
                .py_2()
                .app_class("border-b border-border")
                .child(
                    input()
                        .test_id("search-keyword")
                        .value(move || keyword.get())
                        .placeholder(app::SEARCH_PLACEHOLDER)
                        .optional_label("搜索交易")
                        .text_color(fg)
                        .app_class(
                            "flex-1 bg-background rounded-md border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(on_keyword_change)
                        .on_submit(on_submit),
                ),
        )
        .child(period_row_view)
        .child(
            // 金额区间行 + 已应用筛选描述 + 清除筛选（旧壳搜索页同一行形态）。
            view()
                .flex_row()
                .items_center()
                .flex_wrap()
                .gap_2()
                .px_4()
                .py_2()
                .app_class("border-b border-border")
                .child(
                    input()
                        .test_id("search-amount-min")
                        .value(move || amount_min_text.get())
                        .placeholder("最低金额（元）")
                        .text_color(fg)
                        .w(px(150.0))
                        .app_class(
                            "bg-background rounded-md border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(on_amount_min_change)
                        .on_submit(on_submit),
                )
                .child(
                    input()
                        .test_id("search-amount-max")
                        .value(move || amount_max_text.get())
                        .placeholder("最高金额（元）")
                        .text_color(fg)
                        .w(px(150.0))
                        .app_class(
                            "bg-background rounded-md border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(on_amount_max_change)
                        .on_submit(on_submit),
                )
                .child(
                    // 已应用筛选描述 + 清除筛选（筛选激活才呈现，旧壳同口径）。
                    // test_id 落在文本/按钮内文本上（headless id: 锚点先例同形）。
                    when(
                        filters_active,
                        move || {
                            view()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(
                                    text(applied_filters_text)
                                        .text_size(px(12.0))
                                        .text_color(muted)
                                        .test_id("search-applied-filters"),
                                )
                                .child(
                                    button()
                                        .app_class(
                                            "px-2 py-1 rounded-md text-foreground hover:bg-accent",
                                        )
                                        .child(
                                            text("清除筛选")
                                                .text_size(px(12.0))
                                                .text_color(fg)
                                                .test_id("search-clear-filters"),
                                        )
                                        .on_press(move |_: &()| clear_filters()),
                                )
                        },
                    )
                    .otherwise(view),
                ),
        )
        .child(
            // 结果区（旧壳 searched 门控同口径：未搜索呈占位空态，搜索后呈
            // 只读表格——无行右键菜单、无编辑入口）。断点双渲染（#2057 /
            // ADR-0088 决策 9）：窄窗移动档 = 只读卡片形态（#846 移动档卡片
            // 复用——与交易列表同构的 tx_cards::readonly_cards，格文本同一
            // 装配产物、零写入口）；桌面档表格一字不动。空态同词表同判据。
            when(
                move || searched.get(),
                move || {
                    view()
                        .flex_1()
                        .flex_col()
                        .child(
                            when(
                                adaptive::is_narrow,
                                move || {
                                    when(
                                        move || rows.get().is_empty(),
                                        move || {
                                            view()
                                                .flex_1()
                                                .items_center()
                                                .justify_center()
                                                .child(
                                                    text(assembly::search_empty_text(true))
                                                        .text_size(px(13.0))
                                                        .text_color(muted),
                                                )
                                        },
                                    )
                                    .otherwise(move || tx_cards::readonly_cards(rows))
                                },
                            )
                            .otherwise(move || {
                                view()
                                    .test_id("search-table")
                                    .flex_1()
                                    .child(
                                        data_table()
                                        .columns(columns)
                                        .data(move || app::rows_to_data(&rows.get()))
                                        // 只读平价（#2040 模块注：行点击/下钻委托属表格级
                                        // 发送端 #2061，交易列表先行；本视图不挂委托，
                                        // 自绘格退回纯文本形态）。
                                        .render_cell(app::render_cell(rows, None))
                                        .loading(move || visible_loading.get())
                                        .loading_view(
                                            text("正在加载…").text_color(muted),
                                        )
                                        .empty(
                                            text(assembly::search_empty_text(true))
                                                .text_color(muted),
                                        )
                                        .classes(app::table_classes())
                                        .flex_1(),
                                    )
                            }),
                        )
                        .child(
                            // 底栏（searched 分支内恒显，旧壳搜索页 searched 即显示
                            // hitCount 同形，0 命中也呈现）：读失败优先呈现错误；否则
                            // 命中计数 + 分页条（单页收起；翻页携当前关键词与筛选重搜）。
                            view()
                                .flex_row()
                                .items_center()
                                .gap_3()
                                .px_4()
                                .py_2()
                                .app_class("border-t border-border")
                                .child(
                                    when(
                                        move || error.get().is_some(),
                                        move || {
                                            text(move || {
                                                format!(
                                                    "加载失败：{}",
                                                    error.get().unwrap_or_default()
                                                )
                                            })
                                            .text_size(px(12.0))
                                            .text_color(fg)
                                            .test_id("search-error")
                                        },
                                    )
                                    .otherwise(move || {
                                        text(move || hit_total_text(total.get()))
                                            .text_size(px(12.0))
                                            .text_color(muted)
                                            .test_id("search-total")
                                    }),
                                )
                                .child(
                                    when(
                                        move || {
                                            error.get().is_none()
                                                && assembly::total_pages(total.get()) > 1
                                        },
                                        move || {
                                            view().test_id("search-pagination").flex_row().child(
                                                pagination()
                                                    .accessibility_label("搜索分页")
                                                    .previous_accessibility_label("上一页")
                                                    .next_accessibility_label("下一页")
                                                    .page(move || page.get())
                                                    .total_pages(move || {
                                                        assembly::total_pages(total.get())
                                                    })
                                                    .disabled(move || loading.get())
                                                    .item_class(
                                                        "px-2 py-1 rounded-md text-foreground hover:bg-accent selected:bg-accent disabled:opacity-40",
                                                    )
                                                    .on_change(move |&picked: &u32| {
                                                        page.set(picked);
                                                        load(picked);
                                                    }),
                                            )
                                        },
                                    )
                                    .otherwise(view),
                                ),
                        )
                },
            )
            .otherwise(move || {
                // 占位空态（未搜索：旧壳 search.emptyPlaceholder 同文案）。
                view()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .child(
                        text(assembly::search_empty_text(false))
                            .text_size(px(13.0))
                            .text_color(muted)
                            .test_id("search-empty"),
                    )
            }),
        )
}

// ---------------------------------------------------------------------------
// 延迟出场加载指示（旧壳 useDelayedLoading 的本壳重生，#2010 收尾件语义）。
// ---------------------------------------------------------------------------

/// 出场延迟：原始 loading 持续超过该时长才显示指示器（毫秒，旧壳缺省同值）。
const LOADING_SHOW_AFTER_MS: u64 = 120;

/// 最小驻留：指示器一旦显示至少保持该时长再隐去（过场完整淡出，旧壳缺省同值）。
const LOADING_MIN_SHOW_MS: u64 = 250;

/// 延迟出场加载指示整流：把「原始 loading」整流成「可见 loading」——快加载
/// 全程不可见（内容直接换，零闪烁），慢加载延迟显示且驻留至过场完整淡出
///（AGENTS.md 体验标尺：用户使用时基本感受不到迟滞感）。纯时序整流，不持
/// 业务语义：请求发起与数据落位归调用方。
///
/// 边沿语义（旧壳 useDelayedLoading 同形）：上升沿不立即显示——`show_after`
/// 后仍在加载才登场；下降沿未登场零痕迹退场、已登场补足最小驻留再隐去。
/// 定时器以序号作废（新边沿使在途定时器失配自弃，与防抖序号同形）；
/// 驻留计时取引擎时钟（manual clock 语义一致，headless 探针可驱动）。
fn delayed_loading(raw: Signal<bool>) -> Signal<bool> {
    let visible = signal(false);
    let show_gen = signal(0u64);
    let hide_gen = signal(0u64);
    let shown_at = signal(None::<nsh_core::time::Instant>);
    effect(move || {
        if raw.get() {
            // 上升沿：作废在途隐藏定时器，show_after 后仍在加载才登场。
            hide_gen.update(|g| *g += 1);
            show_gen.update(|g| *g += 1);
            let seq = show_gen.get_untracked();
            set_timeout(Duration::from_millis(LOADING_SHOW_AFTER_MS), move || {
                if show_gen.get_untracked() == seq && raw.get_untracked() {
                    shown_at.set(Some(nsh_core::time::current().now()));
                    visible.set(true);
                }
            });
        } else {
            // 下降沿：作废在途出场定时器；未登场零痕迹；已登场补足最小驻留。
            show_gen.update(|g| *g += 1);
            if !visible.get_untracked() {
                return;
            }
            let elapsed_ms = shown_at
                .get_untracked()
                .map(|at| {
                    nsh_core::time::current()
                        .now()
                        .duration_since(at)
                        .as_millis() as u64
                })
                .unwrap_or(0);
            let remaining = LOADING_MIN_SHOW_MS.saturating_sub(elapsed_ms);
            hide_gen.update(|g| *g += 1);
            let seq = hide_gen.get_untracked();
            set_timeout(Duration::from_millis(remaining), move || {
                if hide_gen.get_untracked() == seq && !raw.get_untracked() {
                    visible.set(false);
                }
            });
        }
    });
    visible
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use nsh_core::time::Clock;
    use nsh_engine::{EngineConfig, Environment, Idiom, Os, Points, Root, Size};
    use nsh_testing::Headless;

    use super::*;

    /// 探针根组件：原始 loading 开关按钮（引擎线程真实按压路径）+ 整流后的
    /// 可见性文本（断言面）。
    #[component]
    fn LoadingProbe() -> impl IntoElement {
        let raw = signal(false);
        let visible = delayed_loading(raw);
        view()
            .flex_1()
            .flex_col()
            .gap_2()
            .child(
                view()
                    .test_id("probe-loading-on")
                    .on_press(move |_: &()| raw.set(true))
                    .child(text("开")),
            )
            .child(
                view()
                    .test_id("probe-loading-off")
                    .on_press(move |_: &()| raw.set(false))
                    .child(text("关")),
            )
            .child(
                text(move || {
                    if visible.get() {
                        "指示器在"
                    } else {
                        "指示器隐"
                    }
                })
                .test_id("loading-indicator"),
            )
    }

    /// manual clock 桌面配置（run_for/fast_forward 驱动引擎定时器的硬前提）。
    fn probe_config() -> EngineConfig {
        EngineConfig {
            viewport: Size::new(Points(640.0), Points(480.0)),
            environment: Environment {
                os: Os::Linux,
                idiom: Idiom::Desktop,
                ..Environment::default()
            },
            clock: Clock::manual(SystemTime::UNIX_EPOCH),
            ..EngineConfig::new(Root::of::<LoadingProbe>())
        }
    }

    /// 慢加载：上升沿不立即显示——出场延迟（120ms）后仍在加载才登场；下降沿
    /// 已登场补足最小驻留（250ms）再隐去（旧壳 useDelayedLoading 边沿语义）。
    #[test]
    fn delayed_loading_shows_after_delay_and_holds_min_show() {
        let mut app = Headless::start(probe_config()).unwrap();
        app.wait_for("id:loading-indicator").unwrap();
        assert!(
            app.shows("指示器隐"),
            "初始隐藏：{:?}",
            app.screen().texts()
        );

        // 上升沿：50ms 未达出场延迟，不显示。
        app.press("id:probe-loading-on").unwrap();
        app.run_for(std::time::Duration::from_millis(50)).unwrap();
        assert!(
            app.shows("指示器隐"),
            "出场延迟内不显示：{:?}",
            app.screen().texts()
        );
        // 累计 160ms（> 120ms）且仍在加载 → 登场。
        app.run_for(std::time::Duration::from_millis(110)).unwrap();
        assert!(
            app.shows("指示器在"),
            "出场延迟后登场：{:?}",
            app.screen().texts()
        );

        // 下降沿：已登场 → 补足最小驻留再隐去。
        app.press("id:probe-loading-off").unwrap();
        app.run_for(std::time::Duration::from_millis(100)).unwrap();
        assert!(
            app.shows("指示器在"),
            "驻留期内不隐去：{:?}",
            app.screen().texts()
        );
        app.run_for(std::time::Duration::from_millis(300)).unwrap();
        assert!(
            app.shows("指示器隐"),
            "驻留期满隐去：{:?}",
            app.screen().texts()
        );
    }

    /// 快加载全程不可见（#2040 AC5「快搜索不闪加载指示器」）：上升沿后在
    /// 出场延迟内就下降（本地读毫秒级完成）→ 登场定时器失配自弃，零痕迹。
    #[test]
    fn delayed_loading_fast_loads_never_flash() {
        let mut app = Headless::start(probe_config()).unwrap();
        app.wait_for("id:loading-indicator").unwrap();

        // 开 → 10ms 内关（快加载形态）→ 推进过全部定时器点位。
        app.press("id:probe-loading-on").unwrap();
        app.run_for(std::time::Duration::from_millis(10)).unwrap();
        app.press("id:probe-loading-off").unwrap();
        app.run_for(std::time::Duration::from_millis(500)).unwrap();
        assert!(
            app.shows("指示器隐"),
            "快加载全程不可见：{:?}",
            app.screen().texts()
        );

        // 再来一次更短的窗口（下一事件循环拍即关）。
        app.press("id:probe-loading-on").unwrap();
        app.press("id:probe-loading-off").unwrap();
        app.run_for(std::time::Duration::from_millis(500)).unwrap();
        assert!(
            app.shows("指示器隐"),
            "零窗口快加载同样零痕迹：{:?}",
            app.screen().texts()
        );
    }
}
