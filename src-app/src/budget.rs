//! 预算视图（issue #2033 NSH 迁移②——封装族热身；#2073 自 app.rs 归位）：
//! 预算管理各分类预算 + 执行进度呈现。域读写直调 `ledger-budget` 等域 crate
//! 读路径（[`crate::boot`]，不经 IPC），失效信号接 [`crate::signals`] 预算族
//! （写路径后广播、订阅重读）；PinyinSelect / Modal / Popconfirm 三封装件首
//! 消费面（[`crate::pinyin_select`] / [`crate::modal`] /
//! [`crate::popconfirm`]，视图不直连框架基元，ADR-0118）。金额与进度口径归
//! 装配层预算族（[`crate::assembly`]，渲染期零格式化）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use ledger_infra::error::AppError;
use nsh_base::{data_table, input};
use nsh_core::prelude::*;

use crate::app::db_slot;
use crate::design::font_size;
use crate::transactions::{amount_color, amount_privacy_enabled, table_classes};
use crate::views::field_label;
use crate::{adaptive, assembly, boot, modal, nav, pinyin_select, popconfirm, signals};

// ---------------------------------------------------------------------------
// 预算视图（#2033 NSH 迁移②——封装族热身）：预算管理各分类预算 + 执行进度
// 呈现。域读写直调 `ledger-budget` 等域 crate 读路径（[`crate::boot`]，不经
// IPC），失效信号接 [`crate::signals`] 预算族（写路径后广播、订阅重读）；
// PinyinSelect / Modal / Popconfirm 三封装件首消费面（[`crate::pinyin_select`]
// / [`crate::modal`] / [`crate::popconfirm`]，视图不直连框架基元，ADR-0118）。
// 金额与进度口径归装配层预算族（[`crate::assembly`]，渲染期零格式化）。
// ---------------------------------------------------------------------------

/// 周期单选行（创建表单与编辑弹窗共用形态，词表旧壳 budget.period）：两个
/// radio（nsh-base 行为件），锚点前缀由调用方给（headless 选择器用）。
fn budget_period_radio(
    id_prefix: &'static str,
    period: Signal<ledger_budget::BudgetPeriod>,
    fg: AdaptiveColor,
) -> View {
    let option =
        |value: ledger_budget::BudgetPeriod, id_suffix: &'static str, label: &'static str| {
            view().flex_row().items_center().gap_1().child(
                nsh_base::radio()
                    .checked(move || period.get() == value)
                    .on_change(move |_: &bool| period.set(value))
                    .child(
                        text(label)
                            .text_size(px(font_size::SECONDARY))
                            .text_color(fg)
                            .test_id(format!("{}{}", id_prefix, id_suffix).as_str()),
                    ),
            )
        };
    view()
        .flex_row()
        .gap_3()
        .child(option(
            ledger_budget::BudgetPeriod::Monthly,
            "-period-monthly",
            "按月",
        ))
        .child(option(
            ledger_budget::BudgetPeriod::Yearly,
            "-period-yearly",
            "按年",
        ))
}

/// 进度条（本视图自绘：轨道 + 按百分比宽度的指示条，颜色随超支红/正常绿——
/// 旧壳 NProgress status 语义，色值取金额语义色表同源两值）。入参是响应式
/// 闭包：宽度与颜色随信号重算（汇总卡信号驱动，表行按格重绘静态闭包）。
fn progress_bar(
    percent: impl Fn() -> f32 + Copy + 'static,
    over_budget: impl Fn() -> bool + Copy + 'static,
    show_label: bool,
) -> View {
    let color = move || {
        if over_budget() {
            amount_color(assembly::AmountSemantic::Expense)
        } else {
            amount_color(assembly::AmountSemantic::Income)
        }
    };
    let bar = view().flex_row().items_center().gap_2().child(
        view()
            .w(px(160.0))
            .h(px(6.0))
            .rounded_full()
            .app_class("bg-muted-foreground/15")
            .child(
                view()
                    .h(px(6.0))
                    .w(move || px(160.0 * (percent().clamp(0.0, 100.0) / 100.0)))
                    .rounded_full()
                    .bg(color),
            ),
    );
    if show_label {
        bar.child(
            text(move || format!("{}%", percent() as i64))
                .text_size(px(font_size::CAPTION))
                .test_id("budget-progress-percent"),
        )
    } else {
        bar
    }
}

/// 预算视图（导航骨架预算分支的真实组件，#2033）：
///
/// - **新增预算卡**：分类（拼音可搜下拉，数据源 = 支出分类树选项）+ 周期
///   （按月/按年单选，缺省月度与后端缺省一致）+ 金额（元输入，整数分落定）
///   + 添加；必填与正数前置拦截（旧壳同词表），域码化错误条位呈现。
/// - **年度汇总卡**：年化总额 / 本年已支 + 进度条 + 说明（旧壳 #1948 形态）。
/// - **预算执行表**：分类（路径名或「未分类」回退）/ 周期 / 预算 / 已支出 /
///   进度 / 状态 / 操作（编辑 + 删除，删除经确认气泡）。
///
/// 数据流：装载（进度 + 年度汇总 + 支出分类字典一次读齐）→ 装配层一次性产出
/// 展示格；写路径（创建/编辑/删除）成功后广播预算失效信号，订阅重读——
/// 「失效信号→列表重读」的消费接线点（删除即红条目钉在
/// tests/app_headless.rs）。写重活走 blocking 后台线程，任务挂视图实例
/// owner（弹窗子树卸载不取消，交易删除先例同形）。
#[component]
pub(crate) fn BudgetView() -> impl IntoElement {
    let view_owner = Owner::current();
    let loading = signal(true);
    let error = signal(None::<String>);
    // 装配产物（渲染期零格式化：行格文本已是一次性装配结果）。
    let rows = signal(Vec::<assembly::BudgetRowCells>::new());
    let summary = signal(None::<assembly::BudgetSummaryCells>);
    // 支出分类树选项（拼音可搜下拉的数据源）。
    let options = signal(Vec::<assembly::CategoryOption>::new());

    // 创建表单态。
    let category_id = signal(None::<String>);
    let period = signal(ledger_budget::BudgetPeriod::Monthly);
    let amount_text = signal(String::new());
    let create_error = signal(None::<String>);
    // 编辑弹窗态（目标行 + 回填字段；显示由「意图非空」派生，关闭统一置回
    // None 终态——旧壳 ModalIntent 编排的本壳等价形态）。
    let edit_target = signal(None::<assembly::BudgetRowCells>);
    let edit_amount = signal(String::new());
    let edit_period = signal(ledger_budget::BudgetPeriod::Monthly);
    let edit_error = signal(None::<String>);
    // 删除确认态（气泡目标预算 id；按行匹配的气泡一次只开一个）。
    let confirm_id = signal(None::<String>);
    let delete_error = signal(None::<String>);

    // 主题令牌（双态自适应；缺失兜底统一归 crate::theme 单点）。
    let crate::views::ViewPalette {
        fg,
        muted,
        danger_fg,
    } = crate::views::view_palette();

    // 装载（首屏与失效信号重读共用一条路径）：进度行集 + 年度汇总 + 支出分类
    // 字典一次读齐（小表聚合，重读成本可忽略）；金额隐私开关随装载取设备态
    //（轻量设置项，同步读）。阻塞读走后台线程（SQLite IO 不占引擎线程）。
    let load = move || {
        loading.set(true);
        error.set(None);
        let db = db_slot().clone();
        let privacy = amount_privacy_enabled();
        spawn(async move {
            let Some(db) = db else {
                error.set(Some("内部错误：账本库会话缺失".to_owned()));
                loading.set(false);
                return;
            };
            let outcome = blocking(move || -> ledger_infra::error::Result<(
                Vec<ledger_budget::BudgetProgress>,
                ledger_budget::BudgetAnnualSummary,
                Vec<ledger_categories::Category>,
            )> {
                let progress = boot::budget_progress(&db)?;
                let annual = boot::budget_annual_summary(&db)?;
                let categories = boot::category_rows(&db)?;
                Ok((progress, annual, categories))
            })
            .await;
            match outcome {
                Ok(Ok((progress, annual, category_rows))) => {
                    let table = assembly::CategoryTable::from_categories(&category_rows);
                    rows.set(assembly::assemble_budget_rows(&progress, &table, privacy));
                    summary.set(Some(assembly::assemble_budget_summary(&annual, privacy)));
                    options.set(assembly::expense_category_options(&category_rows));
                }
                Ok(Err(e)) => error.set(Some(e.to_string())),
                Err(_) => error.set(Some("内部错误：读任务异常退出".to_owned())),
            }
            loading.set(false);
        });
    };

    // 失效信号订阅（预算写路径成功后广播 → 本视图重读）：订阅句柄随视图实例
    // 存亡（句柄移入 effect 闭包，交易视图同形——直放 let 会在体结束时 Drop）。
    let reload_subscription = signals::subscribe_budgets_changed(load);
    effect(move || {
        let _held_for_view_lifetime: &signals::BudgetsChangedSubscription = &reload_subscription;
    });

    // 下钻意图投递即弃（预算视图无定位形态——旧壳预算路由不读 focus 同语义；
    // 读一次封闸语义由 take_intent 单点承担）。
    effect(move || {
        let _current_landed = nav::store().current.get();
        let _ = nav::take_intent();
    });

    // 首屏装载（组件任务运行一次；boot 成功先于本视图呈现，DB 恒已就位）。
    load();

    // 创建预算（前置拦截 → 域公开写入口直调 → 成功广播失效信号并清表单）。
    let perform_create = move || {
        let cents = assembly::budget_yuan_to_cents(&amount_text.get());
        let category = category_id.get();
        let failure = match (category.as_ref(), cents) {
            (None, _) | (_, None) => Some("请填写分类和金额".to_owned()),
            (Some(_), Some(c)) if c <= 0 => Some("预算金额必须为正数".to_owned()),
            _ => None,
        };
        if let Some(message) = failure {
            create_error.set(Some(message));
            return;
        }
        create_error.set(None);
        let input = ledger_budget::BudgetInput {
            category_id: category.unwrap_or_default(),
            period: Some(period.get_untracked()),
            amount_cents: cents.unwrap_or_default(),
            start_date: boot::today_string(),
        };
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::create_budget(db, &input),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(_)) => {
                        category_id.set(None);
                        amount_text.set(String::new());
                        period.set(ledger_budget::BudgetPeriod::Monthly);
                        // 失效信号广播（「写路径后刷新」的广播接线点，删除
                        // 即红条目钉在 tests/app_headless.rs）。
                        signals::notify_budgets_invalidated();
                    }
                    Ok(Err(e)) => create_error.set(Some(format!("创建失败: {e}"))),
                    Err(_) => {
                        create_error.set(Some("创建失败: 内部错误：创建任务异常退出".to_owned()))
                    }
                }
            });
        });
    };

    // 编辑保存（金额与周期一并提交；分类不可改，改法为删旧建新——旧壳同
    // 语义；保存成功关窗 + 广播失效信号）。
    let perform_update = move || {
        let Some(target) = edit_target.get_untracked() else {
            return;
        };
        let cents = assembly::budget_yuan_to_cents(&edit_amount.get());
        let failure = match cents {
            None => Some("预算金额必须为正数".to_owned()),
            Some(c) if c <= 0 => Some("预算金额必须为正数".to_owned()),
            Some(_) => None,
        };
        if let Some(message) = failure {
            edit_error.set(Some(message));
            return;
        }
        edit_error.set(None);
        let next_period = edit_period.get_untracked();
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::update_budget(
                        db,
                        &target.id,
                        cents.unwrap_or_default(),
                        Some(next_period),
                    ),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        edit_target.set(None);
                        signals::notify_budgets_invalidated();
                    }
                    Ok(Err(e)) => edit_error.set(Some(format!("更新失败: {e}"))),
                    Err(_) => {
                        edit_error.set(Some("更新失败: 内部错误：保存任务异常退出".to_owned()))
                    }
                }
            });
        });
    };

    // 删除执行（气泡确认后；成功广播失效信号，失败条位呈现不静默）。
    let perform_delete = move |id: String| {
        delete_error.set(None);
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::delete_budget(db, &id),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => signals::notify_budgets_invalidated(),
                    Ok(Err(e)) => delete_error.set(Some(format!("删除失败: {e}"))),
                    Err(_) => {
                        delete_error.set(Some("删除失败: 内部错误：删除任务异常退出".to_owned()))
                    }
                }
            });
        });
    };

    // 表格自绘格（进度条 / 状态色 / 操作列）：格文本仍是装配产物，本闭包只
    // 补形态与交互（编辑回填、删除目标回写）。行标识用 Arc<str>（Copy）——
    // 封装件闭包界要求 Copy 捕获。
    let render_budget_cell =
        move |cell: &nsh_base::DataTableCell| -> nsh_core::element::AnyElement {
            let row_index = cell.row as usize;
            match cell.key.as_ref() {
                "progress" => {
                    let snapshot = rows.get_untracked();
                    let row = snapshot.get(row_index);
                    let percent = row.map(|r| r.percent).unwrap_or(0.0);
                    let over = row.map(|r| r.over_budget).unwrap_or(false);
                    progress_bar(move || percent, move || over, true).into_element()
                }
                "status" => {
                    let snapshot = rows.get_untracked();
                    let row = snapshot.get(row_index);
                    let over = row.map(|r| r.over_budget).unwrap_or(false);
                    let status = row.map(|r| r.status).unwrap_or("");
                    text(status)
                        .text_size(px(font_size::CAPTION))
                        .text_color(if over {
                            amount_color(assembly::AmountSemantic::Expense)
                        } else {
                            amount_color(assembly::AmountSemantic::Income)
                        })
                        .into_element()
                }
                "actions" => {
                    let snapshot = rows.get_untracked();
                    let Some(row) = snapshot.get(row_index) else {
                        return view().into_element();
                    };
                    let row = row.clone();
                    let row_id: std::rc::Rc<str> = row.id.as_str().into();
                    let delete_id = row_id.clone();
                    let open_id = row_id.clone();
                    view()
                        .flex_row()
                        .gap_1()
                        .child(
                            nsh_base::button()
                                .app_class("px-2 py-1 rounded-(--radius) text-foreground hover:bg-accent")
                                .child(
                                    text("编辑")
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(fg)
                                        .test_id("budget-edit"),
                                )
                                .on_press(move |_: &()| {
                                    edit_amount
                                        .set(assembly::budget_cents_to_yuan_text(row.amount_cents));
                                    edit_period.set(row.period);
                                    edit_error.set(None);
                                    edit_target.set(Some(row.clone()));
                                }),
                        )
                        .child(
                            // 轻量确认气泡（App* 封装族 Popconfirm 首消费）：删除
                            // 目标按行回写，气泡开合与确认归封装（抑制上报自含）。
                            popconfirm::popconfirm(
                                "budget-del",
                                move || confirm_id.get().as_deref() == Some(&*open_id),
                                move |open: bool| {
                                    if open {
                                        confirm_id.set(Some(row_id.to_string()));
                                    } else {
                                        confirm_id.set(None);
                                    }
                                },
                                popconfirm::PopconfirmSpec {
                                    message: "确认删除？".to_owned(),
                                    confirm_label: "删除".to_owned(),
                                    cancel_label: "取消".to_owned(),
                                },
                                // 触发件用纯视图（nsh-base Button 自含按压语义
                                // 会吞掉外层按压——封装的按压源是外层包裹视图）。
                                view()
                                    .app_class(
                                        "px-2 py-1 rounded-(--radius) text-foreground hover:bg-accent",
                                    )
                                    .child(
                                        text("删除")
                                            .text_size(px(font_size::CAPTION))
                                            .text_color(fg)
                                            .test_id("budget-delete"),
                                    ),
                                move || perform_delete(delete_id.to_string()),
                            ),
                        )
                        .into_element()
                }
                _ => text(cell.text.clone()).number_of_lines(1).into_element(),
            }
        };

    // 预算执行表列（装配列定义 → 宿主列；进度列吸收富余宽——内容区宽单点
    // [`crate::adaptive::content_width`]，断点口径唯一）。
    let columns = computed(move || {
        let width = adaptive::content_width();
        let mut columns: Vec<nsh_base::DataTableColumn> = assembly::BUDGET_COLUMNS
            .iter()
            .map(|def| {
                let mut column = nsh_base::DataTableColumn::default();
                column.key = def.key.into();
                column.title = Some(def.title.into());
                column.width = Some(Points(def.width));
                column.align = match def.align {
                    assembly::ColumnAlign::Start => nsh_base::DataTableAlign::Start,
                    assembly::ColumnAlign::End => nsh_base::DataTableAlign::End,
                };
                column
            })
            .collect();
        let suggested: f32 = assembly::BUDGET_COLUMNS.iter().map(|def| def.width).sum();
        let extra = (width - suggested).max(0.0);
        if extra > 0.0
            && let Some(progress) = columns.iter_mut().find(|col| col.key == "progress")
            && let Some(w) = progress.width
        {
            progress.width = Some(Points(w.0 + extra));
        }
        columns
    });

    // 创建表单行（分类 / 周期 / 金额 / 添加）。
    let create_form = view()
        .flex_row()
        .items_center()
        .gap_3()
        .flex_wrap()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(field_label("分类", muted))
                .child(pinyin_select::pinyin_select(
                    "budget-category",
                    move || options.get(),
                    category_id,
                    "支出分类",
                    false,
                )),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(field_label("周期", muted))
                .child(budget_period_radio("budget", period, fg)),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(field_label("金额", muted))
                .child(
                    input()
                        .test_id("budget-amount")
                        .value(move || amount_text.get())
                        .placeholder("0.00")
                        .optional_label("预算金额")
                        .text_color(fg)
                        .w(px(120.0))
                        .app_class(
                            "bg-background rounded-(--radius) border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(move |text: &str| amount_text.set(text.to_owned())),
                ),
        )
        .child(
            nsh_base::button()
                .app_class("px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90")
                .child(
                    text("添加")
                        .text_size(px(font_size::SECONDARY))
                        .text_color(crate::theme::color("primary-foreground"))
                        .test_id("budget-add"),
                )
                .on_press(move |_: &()| perform_create()),
        );

    // 年度汇总卡体（汇总在册才呈现——取数失败不渲染空卡，错误由条位呈现）。
    let summary_card = when(
        move || summary.get().is_some(),
        move || {
            view()
                .flex_col()
                .gap_2()
                .app_class("rounded-(--radius-card) border border-border p-3")
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            text("本年度预算")
                                .text_size(px(font_size::SECONDARY))
                                .font_weight(nsh_core::element::FontWeight::SEMIBOLD)
                                .text_color(fg),
                        )
                        .child(
                            text(move || summary.get().map(|c| c.status).unwrap_or_default())
                                .text_size(px(font_size::CAPTION))
                                .text_color(move || {
                                    if summary.get().map(|c| c.over_budget).unwrap_or(false) {
                                        amount_color(assembly::AmountSemantic::Expense)
                                    } else {
                                        amount_color(assembly::AmountSemantic::Income)
                                    }
                                }),
                        ),
                )
                .child(
                    // 响应式统计行（#2057：窄窗纵向堆叠，旧壳移动档三分列收合
                    // 同构；#2033 移动档偏差的本票支付面）。
                    adaptive::responsive_row(32.0, move || {
                        vec![
                            view()
                                .flex_col()
                                .child(
                                    text("年度预算总额")
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(muted),
                                )
                                .child(
                                    text(move || {
                                        summary
                                            .get()
                                            .map(|c| c.annual_total.clone())
                                            .unwrap_or_default()
                                    })
                                    .text_size(px(font_size::TITLE))
                                    .text_color(fg)
                                    .test_id("budget-summary-total"),
                                )
                                .into_element(),
                            view()
                                .flex_col()
                                .child(
                                    text("本年已支")
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(muted),
                                )
                                .child(
                                    text(move || {
                                        summary.get().map(|c| c.spent.clone()).unwrap_or_default()
                                    })
                                    .text_size(px(font_size::LEADING))
                                    .text_color(fg)
                                    .test_id("budget-summary-spent"),
                                )
                                .into_element(),
                        ]
                    }),
                )
                .child(progress_bar(
                    move || summary.get().map(|c| c.percent).unwrap_or(0.0),
                    move || summary.get().map(|c| c.over_budget).unwrap_or(false),
                    false,
                ))
                .child(
                    text("月预算按 ×12 年化计入总额；未设预算的分类支出不计入已支。")
                        .text_size(px(font_size::CAPTION))
                        .text_color(muted),
                )
        },
    )
    .otherwise(view);

    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(
            // 屏名（内容区标题，同容器壳形态）。
            view().px_4().py_2().child(
                text("预算")
                    .text_size(px(font_size::TITLE))
                    .text_color(fg)
                    .test_id("view-title"),
            ),
        )
        .child(
            // 装载失败条位（码化错误 zh-CN 原样，不静默）。
            when(
                move || error.get().is_some(),
                move || {
                    text(move || format!("加载失败：{}", error.get().unwrap_or_default()))
                        .text_size(px(font_size::CAPTION))
                        .text_color(fg)
                        .test_id("budget-error")
                        .app_class("px-4 py-1")
                },
            )
            .otherwise(view),
        )
        .child(
            // 内容（表卡占剩余高度——DataTable 虚拟化按自身高度裁行，不进
            // 滚动容器；窄窗下的整页滚动随窄窗横切票）。
            view()
                .flex_1()
                .flex_col()
                .gap_3()
                .p_4()
                    .child(
                        // 新增预算卡。
                        view()
                            .flex_col()
                            .gap_2()
                            .app_class("rounded-(--radius-card) border border-border p-3")
                            .child(
                                text("新增预算")
                                    .text_size(px(font_size::SECONDARY))
                                    .font_weight(nsh_core::element::FontWeight::SEMIBOLD)
                                    .text_color(fg),
                            )
                            .child(create_form)
                            .child(
                                // 创建错误/提示条位（前置拦截与域错误同位；下次
                                // 尝试开始时清空）。
                                when(
                                    move || create_error.get().is_some(),
                                    move || {
                                        text(move || {
                                            create_error.get().unwrap_or_default()
                                        })
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(danger_fg)
                                        .test_id("budget-create-error")
                                    },
                                )
                                .otherwise(view),
                            ),
                    )
                    .child(summary_card)
                    .child(
                        // 预算执行卡：表格 + 删除失败条位 + 空态。
                        view()
                            .flex_1()
                            .flex_col()
                            .gap_2()
                            .app_class("rounded-(--radius-card) border border-border p-3")
                            .child(
                                text("预算执行")
                                    .text_size(px(font_size::SECONDARY))
                                    .font_weight(nsh_core::element::FontWeight::SEMIBOLD)
                                    .text_color(fg),
                            )
                            .child(
                                view()
                                    .test_id("budget-table")
                                    .flex_1()
                                    .child(
                                        data_table()
                                            .columns(columns)
                                            .data(move || {
                                                let mut data = nsh_base::DataTableRows::new(rows.get().len());
                                                data = data
                                                    .texts("category", rows.get().iter().map(|r| r.category.as_str()))
                                                    .texts("period", rows.get().iter().map(|r| r.period_label))
                                                    .texts("amount", rows.get().iter().map(|r| r.amount.as_str()))
                                                    .texts("spent", rows.get().iter().map(|r| r.spent.as_str()))
                                                    .texts("progress", rows.get().iter().map(|_| ""))
                                                    .texts("status", rows.get().iter().map(|_| ""))
                                                    .texts("actions", rows.get().iter().map(|_| ""));
                                                data
                                            })
                                            .render_cell(move |cell: &nsh_base::DataTableCell| {
                                                render_budget_cell(cell)
                                            })
                                            .loading(move || loading.get())
                                            .loading_view(text("正在加载…").text_color(muted))
                                            .empty(text("暂无数据").text_color(muted))
                                            .classes(table_classes())
                                            .flex_1(),
                                    ),
                            )
                            .child(
                                when(
                                    move || delete_error.get().is_some(),
                                    move || {
                                        text(move || delete_error.get().unwrap_or_default())
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(danger_fg)
                                        .test_id("budget-delete-error")
                                    },
                                )
                                .otherwise(view),
                            ),
                    ),
        )
        .child(
            // 编辑弹窗（App* 封装族 Modal 首消费）：分类只读行 + 周期 + 金额
            // + 取消/保存；ESC / ✕ / 取消三路径关闭不写（弹层语义归封装）。
            modal::app_modal(
                move || edit_target.get().is_some(),
                || "编辑预算".to_owned(),
                move || {
                    edit_target.set(None);
                },
                move || {
                    let category = edit_target
                        .get()
                        .map(|row| row.category.clone())
                        .unwrap_or_default();
                    view()
                        .flex_col()
                        .gap_3()
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(field_label("分类", muted))
                                // 分类只读（改法为删旧建新，旧壳同语义）。
                                .child(text(category).text_size(px(font_size::SECONDARY)).text_color(fg)),
                        )
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(field_label("周期", muted))
                                .child(budget_period_radio("budget-edit", edit_period, fg)),
                        )
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(field_label("金额", muted))
                                .child(
                                    input()
                                        .test_id("budget-edit-amount")
                                        .value(move || edit_amount.get())
                                        .placeholder("0.00")
                                        .optional_label("预算金额")
                                        .text_color(fg)
                                        .w(px(160.0))
                                        .app_class(
                                            "bg-background rounded-(--radius) border border-border px-2 py-1 \
                                             placeholder:text-muted-foreground",
                                        )
                                        .on_change(move |text: &str| {
                                            edit_amount.set(text.to_owned())
                                        }),
                                ),
                        )
                        .child(
                            when(
                                move || edit_error.get().is_some(),
                                move || {
                                    text(move || edit_error.get().unwrap_or_default())
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(danger_fg)
                                        .test_id("budget-edit-error")
                                },
                            )
                            .otherwise(view),
                        )
                        .child(
                            view()
                                .flex_row()
                                .justify_end()
                                .gap_2()
                                .child(
                                    nsh_base::button()
                                        .app_class(
                                            "px-3 py-1.5 rounded-(--radius) border border-border text-foreground hover:bg-accent",
                                        )
                                        .child(
                                            text("取消")
                                                .text_size(px(font_size::SECONDARY))
                                                .text_color(fg)
                                                .test_id("budget-cancel"),
                                        )
                                        .on_press(move |_: &()| edit_target.set(None)),
                                )
                                .child(
                                    nsh_base::button()
                                        .app_class("px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90")
                                        .child(
                                            text("保存")
                                                .text_size(px(font_size::SECONDARY))
                                                .text_color(crate::theme::color("primary-foreground"))
                                                .test_id("budget-save"),
                                        )
                                        .on_press(move |_: &()| perform_update()),
                                ),
                        )
                },
            ),
        )
}
