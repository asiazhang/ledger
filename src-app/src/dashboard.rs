//! 概览视图（issue #2039；冻结点三视图的最后一屏，ADR-0145 修订注记——本票
//! 落地即功能冻结点达成，此后新功能只进原生壳）。
//!
//! 五卡齐备的旧壳 DashboardView 重生（读路径直调域入口，取数口径单点在域）：
//! - **净资产总览卡**（旧壳 #143）：后端 `dashboard_overview` 单点折算的本位币
//!   单一主数字 + 口径说明弹层 + 「查看账户明细」下钻（总览屏唯一动作）；
//! - **本月收支卡**（旧壳 #144）：收入 / 净支出（毛支出 − 退款）/ 结余；
//! - **投资概览卡**（旧壳 #145/#1797）：总市值 / 持仓收益 / 累计收益三卡，
//!   缺料三态显式分离（缺现价计数小字、缺折算汇率整卡警告 + 重试）、无持仓
//!   空态占位；
//! - **财务自由度卡**（旧壳 #344，ADR-0048）：比例 + 阶段标签 + 进度条 +
//!   口径两行与覆盖年数 + 四行口径说明弹层；零分母占位引导去预算页；
//! - **预算年度摘要卡**（旧壳 #1948 的首页聚合入口）：年化总额 / 本年已支 +
//!   进度条，与预算页消费同一命令（数字必然一致）。
//!
//! 取数时机与失效面（旧壳概览只挂载首读；NSH 壳内失效信号按卡片消费面订阅，
//! 比旧壳即时）：交易失效 → 全部五卡（余额/收支/持仓/自由度分子/本年已支）；
//! 预算失效 → 自由度分母与预算卡；投资失效 → 投资卡与自由度分子。挂载首读
//! 与导航返回重挂即最新（旧壳同语义）。
//!
//! 装配纪律：格文本一次性装配（渲染期零格式化），金额换算与折算口径以域为
//! 唯一真源（[`crate::assembly`] 概览卡格家族只消费域行集）；隐私掩码与交易
//! /预算视图同源（设备轻量设置项，装载时机读取）。文案仅 zh-CN。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use chrono::Datelike;
use nsh_core::prelude::*;

use crate::adaptive;
use crate::app::{amount_privacy_enabled, db_slot};
use crate::assembly::{self, CurrencyTable, FreedomStage, PnlSemantic};
use crate::boot;
use crate::nav;
use crate::nav_model::{NavRequest, ViewName};
use crate::popover::popover_tip;
use crate::signals;

/// 净资产口径说明（旧壳 dashboard.netWorth.tooltip 同词表，文案单源两轴
/// 同一份）。
const NET_WORTH_TIP: &str = "净资产 = 账户余额合计（含投资账户现金；隐藏与黑洞账户不计入）+ 持仓市值合计（缺现价或缺汇率的持仓不计入）+ 在持实物资产估值（已处置、软删的资产不计入）。外币按当期汇率折算为本位币，缺汇率时整体报错、不显示残缺合计。与可投资资产的区别：净资产还包含生活现金与实物资产，负债账户的负余额直接抵减。";

/// 自由度口径说明四行（旧壳 dashboard.freedom.formula 等同词表）。
const FREEDOM_TIP_LINES: [&str; 4] = [
    "自由度 = 可投资资产 × 3% ÷ 年度预算总额",
    "分子 = 持仓市值 + 投资账户余额（折算为本位币，生活现金与负债不计入）",
    "分母 = 月度预算 × 12 + 年度预算（按计划节奏年化，不回退实际支出）",
    "3% 为保守安全提取率，≥100% 即财务自由",
];

/// 预算年度摘要说明行（旧壳 budget.summary.note 同词表）。
const BUDGET_SUMMARY_NOTE: &str = "月预算按 ×12 年化计入总额；未设预算的分类支出不计入已支。";

/// 投资卡单批读载荷（行集 + 币种字典 + 累计收益命令透传 / 错误文案；两路
/// 读互不拖累的返回形状，[`DashboardView::load_stats`] 消费）。
type PortfolioLoad = (
    Vec<ledger_investment::Holding>,
    CurrencyTable,
    Option<ledger_investment::CumulativePnlNativeTotal>,
    Option<String>,
);

/// 「今天」（本地时区年与月键，与旧壳 onMounted 的本年本月同口径）。
fn now_ym() -> (i64, String) {
    let now = chrono::Local::now();
    (i64::from(now.year()), now.format("%Y-%m").to_string())
}

/// 概览视图（导航骨架概览分支的真实组件）：五卡 + 口径说明弹层 + 下钻。
/// 「删除即红」负向条目钉在 tests/app_headless.rs（五卡呈现 / 下钻接线 /
/// 弹层开合）与本模块 lib 探针（失效信号重读）。
#[component]
pub(crate) fn DashboardView() -> impl IntoElement {
    // 卡面信号束（每卡独立装载/错误：错误呈现归卡内，一卡报错不拖垮同屏——
    // 旧壳各卡独立 Loadable 同形）。
    let net_worth = signal(None::<assembly::NetWorthCells>);
    let net_worth_error = signal(None::<String>);
    let monthly = signal(assembly::assemble_monthly_flow(None, false));
    let stats = signal(None::<assembly::PortfolioStatsCells>);
    let stats_error = signal(None::<String>);
    let freedom = signal(None::<assembly::FreedomCells>);
    let freedom_error = signal(None::<String>);
    let budget = signal(None::<assembly::BudgetSummaryCells>);
    let budget_error = signal(None::<String>);
    let currencies = signal(CurrencyTable::default());

    // 主题令牌（双态自适应；令牌缺失回退亮灰/中灰可读兜底，先例同形）。
    let tokens = nsh_core::element::tokens();
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    let muted = tokens
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into());
    let danger_fg = tokens
        .color("destructive")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    let info_fg = AdaptiveColor::new(rgb(0x2080f0), rgb(0x63a8f2));
    // 进度条正常色（#2033 预算视图同款取舍：正常态用账本语义绿，超标才交给
    // 错误色——旧壳强调色琥珀在 NSH 无对应令牌）。
    let bar_fg = crate::app::amount_color(assembly::AmountSemantic::Income);
    // 盈亏涨跌色（红涨绿跌，与交易类型语义色正交；色值随旧壳 PNL_COLORS）。
    let gain_fg = AdaptiveColor::new(rgb(0xd03050), rgb(0xe88080));
    let loss_fg = AdaptiveColor::new(rgb(0x18a058), rgb(0x63e2b7));

    // 各卡装载（首屏与失效重读共用一条路径；阻塞读走后台线程，SQLite IO 不占
    // 引擎线程）。每卡一读闭包，域单点取数；币种字典随卡读取（小表，成本可
    // 忽略，旧壳 reference store 口径）。隐私开关装载时机读取（设备轻量设置
    // 项，BudgetView 同形）。
    let load_net_worth = move || {
        net_worth_error.set(None);
        // 隐私开关是响应式存储读（Storage::get_item 走引擎线程的 reactive
        // store），只在引擎侧预读、blocking 任务内不触碰（BudgetView 同形）。
        let privacy = amount_privacy_enabled();
        let db = db_slot().clone();
        spawn(async move {
            let Some(db) = db else {
                net_worth_error.set(Some("内部错误：账本库会话缺失".to_owned()));
                return;
            };
            let outcome = blocking(move || -> ledger_infra::error::Result<(
                ledger_dashboard::DashboardOverview,
                Vec<ledger_currencies::Currency>,
            )> {
                let overview = boot::dashboard_overview(&db)?;
                let currencies = boot::currency_rows(&db)?;
                Ok((overview, currencies))
            })
            .await;
            match outcome {
                Ok(Ok((overview, rows))) => {
                    let table = CurrencyTable::from_currencies(&rows);
                    currencies.set(table.clone());
                    net_worth.set(Some(assembly::assemble_net_worth(
                        &overview, &table, privacy,
                    )));
                }
                Ok(Err(e)) => net_worth_error.set(Some(e.to_string())),
                Err(_) => net_worth_error.set(Some("内部错误：读任务异常退出".to_owned())),
            }
        });
    };

    let load_monthly = move || {
        let privacy = amount_privacy_enabled();
        let db = db_slot().clone();
        spawn(async move {
            let Some(db) = db else {
                return;
            };
            // 行缺失（当月无交易）呈现三零（旧壳 ?? 0 同口径），读失败同形
            // 归零——旧壳本卡无错误呈现面，平价保留。
            let outcome = blocking(move || boot::monthly_summary_rows(&db, now_ym().0)).await;
            let month_key = now_ym().1;
            let current = outcome
                .ok()
                .and_then(|result| result.ok())
                .and_then(|rows| rows.into_iter().find(|row| row.month == month_key));
            monthly.set(assembly::assemble_monthly_flow(current.as_ref(), privacy));
        });
    };

    let load_stats = move || {
        stats_error.set(None);
        let privacy = amount_privacy_enabled();
        let db = db_slot().clone();
        spawn(async move {
            let Some(db) = db else {
                stats_error.set(Some("内部错误：账本库会话缺失".to_owned()));
                return;
            };
            // 两路读同旧壳 usePortfolioOverview：行集（list_holdings）与累计
            // 收益（cumulative_pnl_native_total）各自独立、互不拖累——累计
            // 失败只降级累计卡（警告 + 重试），行集失败降级整卡（旧壳 toast
            // 面在本壳呈卡内警告，本壳无 toast 基建）。
            let outcome = blocking(move || -> ledger_infra::error::Result<PortfolioLoad> {
                let rows = boot::portfolio_rows(&db)?;
                let table = CurrencyTable::from_currencies(&boot::currency_rows(&db)?);
                match boot::cumulative_pnl_native_total(&db) {
                    Ok(total) => Ok((rows, table, Some(total), None)),
                    Err(e) => Ok((rows, table, None, Some(e.to_string()))),
                }
            })
            .await;
            match outcome {
                Ok(Ok((rows, table, cumulative, cumulative_error))) => {
                    currencies.set(table.clone());
                    stats.set(Some(assembly::assemble_portfolio_stats(
                        &rows,
                        cumulative.as_ref(),
                        cumulative_error,
                        &table,
                        privacy,
                    )));
                }
                Ok(Err(e)) => stats_error.set(Some(e.to_string())),
                Err(_) => stats_error.set(Some("内部错误：读任务异常退出".to_owned())),
            }
        });
    };

    let load_freedom = move || {
        freedom_error.set(None);
        let privacy = amount_privacy_enabled();
        let db = db_slot().clone();
        spawn(async move {
            let Some(db) = db else {
                freedom_error.set(Some("内部错误：账本库会话缺失".to_owned()));
                return;
            };
            let outcome = blocking(move || -> ledger_infra::error::Result<(
                ledger_investment::FinancialFreedomOverview,
                Vec<ledger_currencies::Currency>,
            )> {
                let freedom = boot::financial_freedom(&db)?;
                let currencies = boot::currency_rows(&db)?;
                Ok((freedom, currencies))
            })
            .await;
            match outcome {
                Ok(Ok((overview, rows))) => {
                    let table = CurrencyTable::from_currencies(&rows);
                    currencies.set(table.clone());
                    freedom.set(Some(assembly::assemble_financial_freedom(
                        &overview, &table, privacy,
                    )));
                }
                Ok(Err(e)) => freedom_error.set(Some(e.to_string())),
                Err(_) => freedom_error.set(Some("内部错误：读任务异常退出".to_owned())),
            }
        });
    };

    let load_budget = move || {
        budget_error.set(None);
        let privacy = amount_privacy_enabled();
        let db = db_slot().clone();
        spawn(async move {
            let Some(db) = db else {
                budget_error.set(Some("内部错误：账本库会话缺失".to_owned()));
                return;
            };
            let outcome = blocking(move || {
                boot::budget_annual_summary(&db)
                    .map(|s| assembly::assemble_budget_summary(&s, privacy))
            })
            .await;
            match outcome {
                Ok(Ok(cells)) => budget.set(Some(cells)),
                Ok(Err(e)) => budget_error.set(Some(e.to_string())),
                Err(_) => budget_error.set(Some("内部错误：读任务异常退出".to_owned())),
            }
        });
    };

    // 失效信号订阅（按卡片消费面分，句柄随视图实例存亡——移入 effect 闭包，
    // 直放 let 会在体结束时 Drop）：交易 → 全部五卡（余额/收支/持仓/自由度
    // 分子/本年已支）；预算 → 自由度分母 + 预算卡；投资 → 净资产持仓腿 +
    // 投资卡 + 自由度分子。删除任一订阅接线即红（lib 探针钉住）。
    let tx_sub = signals::subscribe_transactions_changed(move || {
        load_net_worth();
        load_monthly();
        load_stats();
        load_freedom();
        load_budget();
    });
    let budget_sub = signals::subscribe_budgets_changed(move || {
        load_freedom();
        load_budget();
    });
    let invest_sub = signals::subscribe_investments_changed(move || {
        load_net_worth();
        load_stats();
        load_freedom();
    });
    effect(move || {
        let _held_for_view_lifetime: (
            &signals::TransactionsChangedSubscription,
            &signals::BudgetsChangedSubscription,
            &signals::InvestmentsChangedSubscription,
        ) = (&tx_sub, &budget_sub, &invest_sub);
    });

    // 下钻意图投递即弃（概览无定位形态——旧壳概览路由不读 focus 同语义；
    // 读一次封闸语义由 take_intent 单点承担）。
    effect(move || {
        let _current_landed = nav::store().current.get();
        let _ = nav::take_intent();
    });

    // 首屏装载（组件任务运行一次；boot 成功先于本视图呈现，DB 恒已就位）。
    load_net_worth();
    load_monthly();
    load_stats();
    load_freedom();
    load_budget();

    // ---- 净资产总览卡（置顶主数字 + 口径说明 + 下钻） ----
    let net_worth_card = view()
        .test_id("net-worth-card")
        .flex_col()
        .gap_2()
        .rounded_lg()
        .p_4()
        .app_class("border border-border bg-background")
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .child(text("净资产").text_size(px(12.0)).text_color(muted))
                .child(popover_tip(
                    "net-worth-info",
                    vec![NET_WORTH_TIP.to_owned()],
                    info_dot(muted),
                )),
        )
        .child(
            // 主数字 / 错误态（缺汇率等报错显示提示文案而非空数字，旧壳同形；
            // 旧壳无重试入口，平价保留）。
            when(
                move || net_worth.get().is_some(),
                move || {
                    text(move || net_worth.get().map(|c| c.net_worth).unwrap_or_default())
                        .text_size(px(28.0))
                        .text_color(fg)
                        .test_id("net-worth-value")
                },
            )
            .otherwise(move || {
                error_line(
                    move || net_worth_error.get().unwrap_or_default(),
                    "net-worth-error",
                    danger_fg,
                )
            }),
        )
        .child(
            // 行容器承载起点对齐（flex_col 子项默认横向拉伸）。
            view().flex_row().child(
                view()
                    .test_id("net-worth-view-accounts")
                    .on_press(move |_: &()| nav::open(NavRequest::bare(ViewName::Accounts)))
                    .rounded_md()
                    .px_1()
                    .py_0p5()
                    .app_class("hover:bg-accent")
                    .child(text("查看账户明细").text_size(px(13.0)).text_color(fg)),
            ),
        );

    // ---- 本月收支卡（收入 / 净支出 / 结余三格；当月无行三零）——响应式统
    // 计行（#2057：窄窗纵向堆叠，旧壳移动档 NGrid cols=1 同构）----
    let monthly_card = card(
        "本月收支",
        adaptive::responsive_row(32.0, move || {
            vec![
                amount_cell(
                    "收入",
                    move || monthly.get().income,
                    "monthly-income",
                    fg,
                    muted,
                )
                .into_element(),
                amount_cell(
                    "净支出",
                    move || monthly.get().net_expense,
                    "monthly-expense",
                    fg,
                    muted,
                )
                .into_element(),
                amount_cell(
                    "结余",
                    move || monthly.get().balance,
                    "monthly-balance",
                    fg,
                    muted,
                )
                .into_element(),
            ]
        }),
    );

    // ---- 投资概览卡（三卡或空态；缺料三态显式分离） ----
    let stats_body = when(
        move || stats.get().as_ref().is_some_and(|c| c.has_holdings),
        move || {
            view()
                .flex_col()
                .gap_2()
                .child(adaptive::responsive_row(32.0, move || {
                    vec![
                        investment_stat(
                            "总市值",
                            stats.read_only(),
                            |c| &c.market_value,
                            "dashboard-total-market-value",
                            InvestStatPalette {
                                pnl: None,
                                neutral: fg,
                                muted,
                                danger: danger_fg,
                            },
                        )
                        .into_element(),
                        investment_stat(
                            "持仓收益",
                            stats.read_only(),
                            |c| &c.unrealized_pnl,
                            "dashboard-total-unrealized-pnl",
                            InvestStatPalette {
                                pnl: Some((gain_fg, loss_fg)),
                                neutral: fg,
                                muted,
                                danger: danger_fg,
                            },
                        )
                        .into_element(),
                        investment_stat(
                            "累计收益",
                            stats.read_only(),
                            |c| &c.cumulative_pnl,
                            "dashboard-total-cumulative-pnl",
                            InvestStatPalette {
                                pnl: Some((gain_fg, loss_fg)),
                                neutral: fg,
                                muted,
                                danger: danger_fg,
                            },
                        )
                        .into_element(),
                    ]
                }))
                .child(
                    // 行集读失败警告（旧壳 toast 面的卡内等价呈现）+ 重试。
                    when(
                        move || stats_error.get().is_some(),
                        move || {
                            retry_line(
                                move || stats_error.get().unwrap_or_default(),
                                "investment-rows-error",
                                "investment-rows-retry",
                                danger_fg,
                                muted,
                                load_stats,
                            )
                        },
                    )
                    .otherwise(view),
                )
        },
    )
    .otherwise(move || {
        text("暂无持仓")
            .text_size(px(13.0))
            .text_color(muted)
            .test_id("investment-empty")
    });
    let investment_card = card("投资概览", stats_body);

    // ---- 预算年度摘要卡（与预算页同一命令同一口径，数字必然一致） ----
    let budget_body = when(
        move || budget_error.get().is_some(),
        move || {
            retry_line(
                move || budget_error.get().unwrap_or_default(),
                "dashboard-budget-error",
                "dashboard-budget-retry",
                danger_fg,
                muted,
                load_budget,
            )
        },
    )
    .otherwise(move || {
        view()
            .flex_col()
            .gap_2()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        view()
                            .flex_row()
                            .gap_8()
                            .child(amount_cell(
                                "年度预算总额",
                                move || budget.get().map(|c| c.annual_total).unwrap_or_default(),
                                "dashboard-budget-total",
                                fg,
                                muted,
                            ))
                            .child(amount_cell(
                                "本年已支",
                                move || budget.get().map(|c| c.spent).unwrap_or_default(),
                                "dashboard-budget-spent",
                                fg,
                                muted,
                            )),
                    )
                    .child(
                        // 状态标签（已超标错误色 / 正常绿；装载中空位）。
                        text(move || budget.get().map(|c| c.status).unwrap_or_default())
                            .text_size(px(12.0))
                            .text_color(move || match budget.get() {
                                Some(c) if c.over_budget => danger_fg,
                                Some(_) => loss_fg,
                                None => muted,
                            })
                            .test_id("dashboard-budget-stage"),
                    ),
            )
            .child(
                when(
                    move || budget.get().is_some(),
                    move || {
                        progress_bar(
                            move || budget.get().map(|c| c.percent).unwrap_or(0.0),
                            move || match budget.get() {
                                Some(c) if c.over_budget => danger_fg,
                                _ => bar_fg,
                            },
                        )
                    },
                )
                .otherwise(view),
            )
            .child(
                text(BUDGET_SUMMARY_NOTE)
                    .text_size(px(12.0))
                    .text_color(muted),
            )
    });
    let budget_card = card("本年度预算", budget_body);

    // ---- 财务自由度卡（比例 + 阶段 + 进度 + 口径四行弹层；零分母占位引导） ----
    let freedom_card = view()
        .test_id("financial-freedom-card")
        .flex_col()
        .gap_2()
        .rounded_lg()
        .p_4()
        .app_class("border border-border bg-background")
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(text("财务自由度").text_size(px(14.0)).text_color(fg))
                .child(popover_tip(
                    "financial-freedom-info",
                    FREEDOM_TIP_LINES.iter().map(|s| (*s).to_owned()).collect(),
                    info_dot(muted),
                )),
        )
        .child(
            when(
                move || freedom_error.get().is_some(),
                move || {
                    retry_line(
                        move || freedom_error.get().unwrap_or_default(),
                        "financial-freedom-error",
                        "financial-freedom-retry",
                        danger_fg,
                        muted,
                        load_freedom,
                    )
                },
            )
            .otherwise(move || {
                when(
                    move || freedom.get().as_ref().is_some_and(|c| c.zero_denominator),
                    move || {
                        // 零分母（未设预算）：占位引导跳预算页，口径不回退实际支出。
                        view()
                            .flex_col()
                            .gap_2()
                            .child(
                                text("设置预算后解锁财务自由度")
                                    .text_size(px(13.0))
                                    .text_color(muted)
                                    .test_id("freedom-zero-denominator"),
                            )
                            .child(
                                view().flex_row().child(
                                    view()
                                        .test_id("freedom-go-budget")
                                        .on_press(move |_: &()| {
                                            nav::open(NavRequest::bare(ViewName::Budget))
                                        })
                                        .rounded_md()
                                        .px_2()
                                        .py_1()
                                        .app_class("bg-primary hover:opacity-90")
                                        .child(
                                            text("去设置预算")
                                                .text_size(px(13.0))
                                                .text_color(rgb(0xffffff)),
                                        ),
                                ),
                            )
                    },
                )
                .otherwise(move || {
                    view()
                        .flex_col()
                        .gap_2()
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(
                                    text(move || {
                                        freedom.get().map(|c| c.ratio_text).unwrap_or_default()
                                    })
                                    .text_size(px(28.0))
                                    .text_color(fg)
                                    .test_id("financial-freedom-ratio"),
                                )
                                .child(
                                    text(move || {
                                        freedom
                                            .get()
                                            .and_then(|c| c.stage)
                                            .map(|s| s.label())
                                            .unwrap_or_default()
                                    })
                                    .text_size(px(12.0))
                                    .text_color(move || match freedom.get().and_then(|c| c.stage) {
                                        Some(FreedomStage::Free) => loss_fg,
                                        Some(FreedomStage::Approaching) => info_fg,
                                        _ => muted,
                                    })
                                    .test_id("financial-freedom-stage"),
                                ),
                        )
                        .child(
                            when(
                                move || freedom.get().is_some(),
                                move || {
                                    progress_bar(
                                        move || {
                                            freedom
                                                .get()
                                                .map(|c| c.percent.clamp(0.0, 100.0))
                                                .unwrap_or(0.0)
                                        },
                                        move || bar_fg,
                                    )
                                },
                            )
                            .otherwise(view),
                        )
                        .child(
                            text(move || {
                                freedom.get().map(|c| c.numerator_line).unwrap_or_default()
                            })
                            .text_size(px(12.0))
                            .text_color(muted),
                        )
                        .child(
                            text(move || {
                                freedom
                                    .get()
                                    .map(|c| c.denominator_line)
                                    .unwrap_or_default()
                            })
                            .text_size(px(12.0))
                            .text_color(muted),
                        )
                        .child(
                            text(move || {
                                freedom.get().map(|c| c.coverage_line).unwrap_or_default()
                            })
                            .text_size(px(12.0))
                            .text_color(muted),
                        )
                })
            }),
        );

    view()
        .flex_1()
        .flex_col()
        .gap_4()
        .p_4()
        .app_class("bg-background")
        .child(
            // 总览两卡同排（旧壳桌面档 2 列栅格：两张「一句话数字」卡同底边，
            // 读出顺序不变）。
            view()
                .flex_row()
                .gap_4()
                .child(net_worth_card.w(px(360.0)))
                .child(monthly_card),
        )
        .child(investment_card)
        .child(budget_card)
        .child(freedom_card)
}

/// 卡片壳（标题 + 内容体的统一容器；旧壳 NCard size=small 的本壳等价形态）。
fn card(title: &'static str, body: impl IntoElement) -> impl IntoElement {
    let tokens = nsh_core::element::tokens();
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    view()
        .flex_col()
        .gap_2()
        .rounded_lg()
        .p_4()
        .app_class("border border-border bg-background")
        .child(text(title).text_size(px(14.0)).text_color(fg))
        .child(body)
}

/// 口径说明触发点（ⓘ 小字；说明文案与开合归 [`popover_tip`]）。
fn info_dot(muted: AdaptiveColor) -> impl IntoElement {
    text("ⓘ").text_size(px(12.0)).text_color(muted)
}

/// 金额格（标签小字 + 数值；旧壳 NText depth=3 + strong 同形态）。
fn amount_cell(
    label: &'static str,
    value: impl Fn() -> String + Copy + 'static,
    test_id: &'static str,
    fg: AdaptiveColor,
    muted: AdaptiveColor,
) -> impl IntoElement {
    view()
        .flex_col()
        .gap_1()
        .child(text(label).text_size(px(12.0)).text_color(muted))
        .child(
            text(value)
                .text_size(px(20.0))
                .text_color(fg)
                .test_id(test_id),
        )
}

/// 投资三卡单卡的色板（中性/警告/弱化 + 涨跌色对；涨跌色对 `None` = 市值
/// 中性——涨跌色不外溢到市值，旧壳同取舍）。
#[derive(Clone, Copy)]
struct InvestStatPalette {
    /// 涨跌色对（涨, 跌）；`None` 恒中性。
    pnl: Option<(AdaptiveColor, AdaptiveColor)>,
    /// 数值前景（无可计入值降级为此色）。
    neutral: AdaptiveColor,
    /// 标签与缺价计数小字。
    muted: AdaptiveColor,
    /// 警告文案。
    danger: AdaptiveColor,
}

/// 投资三卡单卡（标签 + 说明弹层 + 数值/警告/缺价计数三态）。`field` 取本卡
/// 格（响应式读同一卡面信号，失效重读后随信号更新）。
fn investment_stat(
    label: &'static str,
    stats: ReadSignal<Option<assembly::PortfolioStatsCells>>,
    field: impl Fn(&assembly::PortfolioStatsCells) -> &assembly::PortfolioStatCells + Copy + 'static,
    test_id: &'static str,
    palette: InvestStatPalette,
) -> impl IntoElement {
    let tip_body = match label {
        "总市值" => {
            "各持仓按「数量 × 最新价格」计算市值的合计；缺现价或缺汇率的持仓不计入；不含投资账户里的现金。"
        }
        "持仓收益" => {
            "当前市值减去持仓成本（FIFO 批次成本，含买入手续费）的未实现盈亏，随行情变动；卖出后才转为已实现盈亏。"
        }
        _ => "持仓收益（未实现）＋已实现盈亏＋累计分红。",
    };
    let info_id: &'static str = match test_id {
        "dashboard-total-market-value" => "dashboard-total-market-value-info",
        "dashboard-total-unrealized-pnl" => "dashboard-total-unrealized-pnl-info",
        _ => "dashboard-total-cumulative-pnl-info",
    };
    let pnl_color = move || {
        let cell = stats.get().as_ref().map(|c| field(c).clone());
        match (cell.and_then(|c| c.pnl), palette.pnl) {
            (Some(PnlSemantic::Gain), Some((gain, _))) => gain,
            (Some(PnlSemantic::Loss), Some((_, loss))) => loss,
            _ => palette.neutral,
        }
    };
    view()
        .flex_col()
        .gap_1()
        .flex_1()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .child(text(label).text_size(px(12.0)).text_color(palette.muted))
                .child(popover_tip(
                    info_id,
                    vec![investment_tip(tip_body)],
                    info_dot(palette.muted),
                )),
        )
        .child(
            // 警告态优先（缺汇率/命令报错 → 整卡警告 + 重试，不给半截数字）。
            when(
                move || {
                    stats
                        .get()
                        .as_ref()
                        .map(field)
                        .is_some_and(|c| c.warning.is_some())
                },
                move || {
                    view()
                        .flex_col()
                        .gap_1()
                        .child(
                            text(move || {
                                stats
                                    .get()
                                    .as_ref()
                                    .map(field)
                                    .and_then(|c| c.warning.clone())
                                    .unwrap_or_default()
                            })
                            .text_size(px(12.0))
                            .text_color(palette.danger)
                            .test_id(format!("{test_id}-warning").as_str()),
                        )
                        .child(
                            text("重试")
                                .text_size(px(12.0))
                                .text_color(palette.muted)
                                .test_id(format!("{test_id}-retry").as_str()),
                        )
                },
            )
            .otherwise(move || {
                view()
                    .flex_col()
                    .gap_1()
                    .child(
                        text(move || {
                            stats
                                .get()
                                .as_ref()
                                .map(field)
                                .and_then(|c| c.text.clone())
                                .unwrap_or_else(|| "-".to_owned())
                        })
                        .text_size(px(20.0))
                        .text_color(move || {
                            if stats
                                .get()
                                .as_ref()
                                .map(field)
                                .is_some_and(|c| c.text.is_some())
                            {
                                pnl_color()
                            } else {
                                palette.neutral
                            }
                        })
                        .test_id(test_id),
                    )
                    .child(
                        when(
                            move || {
                                stats
                                    .get()
                                    .as_ref()
                                    .map(field)
                                    .is_some_and(|c| c.missing_price_note.is_some())
                            },
                            move || {
                                text(move || {
                                    stats
                                        .get()
                                        .as_ref()
                                        .map(field)
                                        .and_then(|c| c.missing_price_note.clone())
                                        .unwrap_or_default()
                                })
                                .text_size(px(11.0))
                                .text_color(palette.muted)
                                .test_id(format!("{test_id}-missing-price").as_str())
                            },
                        )
                        .otherwise(view),
                    )
            }),
        )
}

/// 投资三卡概念说明（旧壳 investments.concepts.* 同词表；说明 = 正文 + 全账本
/// 口径段拼接，tipTemplate "{body}{scope}" 同形）。
fn investment_tip(body: &str) -> String {
    format!(
        "{body}金额折算到全局默认币种（本位币）后合并为单个数值；覆盖全账本的持仓，不随搜索或过滤收窄；隐藏账户照常计入。"
    )
}

/// 进度条（比例读数与色随响应式闭包；旧壳 NProgress type=line 的本壳等价
/// 形态——轨道 240pt 定宽，与 #2033 预算视图条体同形）。
fn progress_bar(
    percent: impl Fn() -> f32 + Copy + 'static,
    color: impl Fn() -> AdaptiveColor + Copy + 'static,
) -> impl IntoElement {
    view().flex_row().items_center().child(
        view()
            .w(px(240.0))
            .h(px(6.0))
            .rounded_full()
            .bg(hsla(0.0, 0.0, 0.5, 0.15))
            .child(
                view()
                    .h(px(6.0))
                    .w(move || px(240.0 * (percent().clamp(0.0, 100.0) / 100.0)))
                    .rounded_full()
                    .bg(color()),
            ),
    )
}

/// 卡内错误行（警告色文案；旧壳 NAlert type=warning 的本壳等价呈现）。
fn error_line(
    message: impl Fn() -> String + Copy + 'static,
    test_id: &'static str,
    danger_fg: AdaptiveColor,
) -> impl IntoElement {
    text(message)
        .text_size(px(12.0))
        .text_color(danger_fg)
        .test_id(test_id)
}

/// 卡内错误行 + 重试（重试即重发同一条读命令，不产生任何写入——旧壳
/// dashboard.freedom.retry 同语义）。
fn retry_line(
    message: impl Fn() -> String + Copy + 'static,
    error_id: &'static str,
    retry_id: &'static str,
    danger_fg: AdaptiveColor,
    muted: AdaptiveColor,
    retry: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(error_line(message, error_id, danger_fg))
        .child(
            view()
                .on_press(move |_: &()| retry())
                .rounded_md()
                .px_1()
                .py_0p5()
                .app_class("hover:bg-accent")
                .child(
                    text("重试")
                        .text_size(px(12.0))
                        .text_color(muted)
                        .test_id(retry_id),
                ),
        )
}

#[cfg(test)]
mod tests;
