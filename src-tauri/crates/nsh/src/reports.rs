//! 报表视图（issue #2048 NSH 迁移⑰；旧壳 `ReportsView.vue` + `reports/`
//! MerchantRankingPanel.vue + reports-session.ts 的重生）。
//!
//! 三卡齐备（期间口径统一，取数直调报表域读路径、不经 IPC——与旧壳
//! monthly_summary / category_shares / merchant_shares 命令同一域函数）：
//! - **月度收支**：期间内按月分布（issue #411 期间化口径），每月三根语义色
//!   并列柱（收入/支出/退款，交易金额同源色，旧壳 #435 同口径），bar 图
//!   自绘落 [`crate::bar_chart`] 纵向形态（视图零绘制基元）；
//! - **支出分类构成**（#378 归并 + 未分类柱；#379 图内下钻 + #380 跳转
//!   下钻）：横向色彩行，净额降序、未分类灰、按 id 稳定配色；下钻态 =
//!   二级子分类 + 父直挂行（各柱合计恒等于父柱金额）+ 面包屑点根回基础态；
//!   点行名分派与旧壳 handleCategoryBarClick 同一裁决（未分类柱直达
//!   「仅无分类」列表——是柱不是层级；下钻分类中途被删解析不到不卡死）；
//! - **商户消费排行**（#192 → #588 → #618）：表格五列（商户名｜金额分布
//!   内嵌条｜金额｜占比%｜笔数），排序与 topN 截断全部后端收口、行内零
//!   口径逻辑；TopN 档位 Top 5 / Top 10 闭集二、会话内保留。
//!
//! 会话状态（旧壳 reports-session store 的重生，issue #427 平价）：期间快照 +
//! 图内下钻 + TopN 提升为进程级信号单例（[`store`]）——同一会话内离开报表页
//! 再回来 = 回到离开时的样子；冷启动回默认「当年」；不写存储。规则内化：
//! 期间写入「同值守卫 + 期间切换复位下钻」单点（[`apply_period`]，视图经
//! [_period_row::period_row] 的 on_apply 回调接线；步进/芯片全部经它）；
//! ESC 复位出口（#894）= 期间回当年 + 下钻回基础态 + TopN 回默认档，闸门 =
//! 抑制 + 幂等守卫（旧壳 registerViewReset 的本壳等价形态）。
//!
//! 跳转下钻（#380/#589）：消费导航单点接缝 [`nav::open`] + 筛选下钻载荷
//! 通道（#2037，[`tx_filter::DrillFilters`]），不自建——载荷 = 分类/商户 +
//! 期间首尾 + 收支类型集合（支出 + 退款，与参与聚合类型同源：退款继承原
//! 分类计入柱值；income 可携带商户但不进排行）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_core::prelude::*;

use crate::app::{amount_color, amount_privacy_enabled, db_slot};
use crate::assembly::{self, AmountSemantic, CategoryTable};
use crate::bar_chart::{
    self, HorizontalRowFace, VerticalBarFace, VerticalGroupFace, category_color, ranking_color,
    uncategorized_color,
};
use crate::boot;
use crate::nav;
use crate::nav_model::{NavRequest, ViewName};
use crate::{overlay, period, period_row, sidebar, tx_filter};

// ---------------------------------------------------------------------------
// 会话状态（旧壳 reports-session store 的重生：进程级信号单例）
// ---------------------------------------------------------------------------

/// 商户排行 TopN 档位闭集（issue #588：仅 Top 5 / Top 10 两档，不设「全部」）。
pub(crate) const MERCHANT_TOP_N_OPTIONS: [i64; 2] = [5, 10];

/// 商户排行 TopN 默认档（头部不被长尾稀释的最小档位；冷启动回此默认）。
const MERCHANT_TOP_N_DEFAULT: i64 = 5;

/// 会话态句柄（Copy；信号挂首次 [`store`] 调用所在组件实例——生产路径契约：
/// 报表视图体最先调用。视图卸载重挂不变：进程槽承担会话内保留）。
#[derive(Clone, Copy)]
pub(crate) struct ReportSession {
    /// 期间快照（双端必有界，ADR-0057：日期芯片闭集无「全部」）。
    pub period: Signal<Option<period::PeriodRange>>,
    /// 图内下钻的一级分类 id（None = 基础态；瞬时视图状态不持久化）。
    pub drilled_root_id: Signal<Option<String>>,
    /// 商户排行 TopN 档位。
    pub merchant_top_n: Signal<i64>,
}

/// 进程级槽（会话内跨视图卸载保留的唯一事实源；headless 多场景串行复位）。
/// 锁毒化取回内值——槽位本身不含不变量（nav / tx_filter 同形）。
static SESSION: std::sync::Mutex<Option<ReportSession>> = std::sync::Mutex::new(None);

fn session_slot() -> std::sync::MutexGuard<'static, Option<ReportSession>> {
    SESSION.lock().unwrap_or_else(|p| p.into_inner())
}

/// 报表页会话态（惰性初始化：冷启动回默认「当年」——「当年」在此现算，
/// 会话跨年存活时复位落在当下年份是旧壳同口径）。
pub(crate) fn store() -> ReportSession {
    let mut slot = session_slot();
    if let Some(s) = *slot {
        return s;
    }
    let s = ReportSession {
        period: signal(Some(default_period())),
        drilled_root_id: signal(None),
        merchant_top_n: signal(MERCHANT_TOP_N_DEFAULT),
    };
    *slot = Some(s);
    s
}

/// headless 测试接缝复位（导航 / 筛选会话态同形；下一场景回默认态）。
#[doc(hidden)]
pub(crate) fn reset_for_tests() {
    *session_slot() = None;
}

/// 下钻写入（None = 点面包屑根回基础态；期间不受牵连——旧壳 setDrilldown）。
fn set_drilldown(session: &ReportSession, id: Option<String>) {
    session.drilled_root_id.set(id);
}

/// 默认期间 = 「当下年份自然年」（本地日历；预设换算在合法年份恒有定义，
/// 分支缺席时的兜底回退单日区间——不遇 panic 面）。
fn default_period() -> period::PeriodRange {
    let t = today();
    period::preset_range(period::Preset::Year, t).unwrap_or_else(|| period::PeriodRange {
        from: t.to_string(),
        to: t.to_string(),
    })
}

/// 「今天」（本地时区日历日；期间预设与边界抬升的相对点，period_row 同口径）。
fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

/// ESC 复位出口（旧壳 resetToDefault 平价）：期间回默认「当年」、下钻回
/// 基础态、TopN 回默认档——复位即清除保留态本身。返回 (期间是否变化, TopN
/// 是否变化)：期间变 → 三卡重拉；TopN 变 → 商户卡重拉；下钻复位是纯视图
/// 投影不重拉（与旧壳 resetToDefault 后的 watch 驱动面同形态）。
fn reset_to_default(session: &ReportSession) -> (bool, bool) {
    let default = default_period();
    let period_changed = session.period.get_untracked().as_ref() != Some(&default);
    if period_changed {
        session.period.set(Some(default.clone()));
    }
    session.drilled_root_id.set(None);
    let top_changed = session.merchant_top_n.get_untracked() != MERCHANT_TOP_N_DEFAULT;
    if top_changed {
        session.merchant_top_n.set(MERCHANT_TOP_N_DEFAULT);
    }
    (period_changed, top_changed)
}

/// 幂等守卫（旧壳 isAtDefault 同口径：无保留状态时复位无操作）。
fn is_at_default(session: &ReportSession) -> bool {
    session.period.get_untracked().as_ref() == Some(&default_period())
        && session.drilled_root_id.get_untracked().is_none()
        && session.merchant_top_n.get_untracked() == MERCHANT_TOP_N_DEFAULT
}

// ---------------------------------------------------------------------------
// 视图
// ---------------------------------------------------------------------------

/// 分类份额行镜像（信号 Clone/PartialEq 约束面：报表域行集类型不带
/// Clone/PartialEq——镜像只承载字段值，口径仍在域行集与装配函数；重建面
/// 只为下钻投影的再装配，见 [`share_cn`] 注）。
/// 月度 + 分类构成共享装载批读面（月度格 + 分类份额镜像 + 分类表 + 期间
/// 边界，同批读取一次取回——新读维度改本结构一处，PageRead 先例同形）。
struct MainBatchRead {
    monthly_cells: Vec<assembly::MonthlyBarGroupCells>,
    share_snaps: Vec<ShareSnap>,
    table: CategoryTable,
    boundary: Option<ledger_reports::DateRange>,
}

#[derive(Clone, Debug, PartialEq)]
struct ShareSnap {
    category_id: Option<String>,
    category_name: String,
    amount_cents: i64,
}

impl ShareSnap {
    fn of_share(share: &ledger_reports::CategoryShare) -> Self {
        Self {
            category_id: if share.category_id.is_empty() {
                None
            } else {
                Some(share.category_id.clone())
            },
            category_name: share.category_name.clone(),
            amount_cents: share.amount_cents,
        }
    }

    fn to_share(&self) -> ledger_reports::CategoryShare {
        ledger_reports::CategoryShare {
            category_id: self.category_id.clone().unwrap_or_default(),
            category_name: self.category_name.clone(),
            amount_cents: self.amount_cents,
        }
    }
}

/// 镜像行集 → 域行集（下钻与基础投影形态仍以域类型为唯一入参口径）。
fn share_cn(snaps: &[ShareSnap]) -> Vec<ledger_reports::CategoryShare> {
    snaps.iter().map(ShareSnap::to_share).collect()
}

/// 分类构成行高（pt；旧壳 CATEGORY_ROW_HEIGHT 32 的量级）。
const CATEGORY_ROW_HEIGHT: f32 = 36.0;

/// 商户排行行高（pt）：五列读表行距；滚动上限随旧壳限高 320 同口径。
const MERCHANT_ROW_HEIGHT: f32 = 44.0;

/// 商户表数值三列定宽（pt）：金额 / 占比 / 笔数右对齐。
const MERCHANT_COL_WIDTH: f32 = 88.0;

/// 商户名列宽（pt；下钻入口列）。
const MERCHANT_NAME_WIDTH: f32 = 140.0;

/// 报表视图（导航骨架报表分支的真实组件）：期间行 + 月度收支卡 + 分类
/// 构成卡（两段下钻）+ 商户消费排行卡。
#[component]
pub(crate) fn ReportsView() -> impl IntoElement {
    let view_owner = Owner::current();
    let session = store();

    // ============================ 快照态 ============================
    // 装载读批产物（期间变化重拉；下钻与 TopN 变化按消费面重拉）。信号存回族
    // 层装配镜像（域行集类型不带 Clone/PartialEq 的信号约束面——镜像只承载
    // 字段值，口径仍在域行集与装配函数，重建仅供下钻投影再装配）。
    let monthly_cells = signal(Vec::<assembly::MonthlyBarGroupCells>::new());
    let share_snaps = signal(Vec::<ShareSnap>::new());
    let categories = signal(CategoryTable::default());
    let merchant_cells_raw = signal(Vec::<assembly::MerchantRankingCells>::new());
    let boundary_dates = signal(None::<(Option<String>, Option<String>)>);
    let privacy = signal(false);
    let main_error = signal(None::<String>);
    let merchant_error = signal(None::<String>);
    let main_seq = signal(0u64);
    let merchant_seq = signal(0u64);

    // 主题令牌（双态自适应；令牌缺失回退亮灰/中灰可读兜底，先例同形）。
    let tokens = nsh_core::element::tokens();
    let fg = tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    let muted = tokens
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into());
    let accent = tokens
        .color("primary")
        .unwrap_or_else(|| hsla(0.11, 0.82, 0.60, 1.0).into());
    let danger_fg = tokens
        .color("destructive")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into());
    // 月度图三柱语义色（与交易列表同类金额同源，色值单源 app::amount_color）。
    let income_color = amount_color(AmountSemantic::Income);
    let expense_color = amount_color(AmountSemantic::Expense);
    let refund_color = amount_color(AmountSemantic::Refund);

    // ============================ 装载批 ============================
    // 三卡随当前期间重算（旧壳 refresh / fetchMerchantReport 分批同形态；
    // 两批各自 latest-wins：序号失配迟到结果不落位）。月度图与分类构成共用
    // 一批（同一期间读侧），商户排行独立一批（TopN 变化只重拉商户卡）。
    let reload_main = move || {
        main_error.set(None);
        main_seq.update(|seq| *seq += 1);
        let seq = main_seq.get();
        let Some(range) = session.period.get_untracked() else {
            return;
        };
        let db = db_slot().clone();
        // 隐私开关装载时机读取（设备轻量设置项，概览视图同形）。
        let privacy_enabled = amount_privacy_enabled();
        view_owner.run(move || {
            spawn(async move {
                let Some(db) = db else {
                    main_error.set(Some("内部错误：账本库会话缺失".to_owned()));
                    return;
                };
                let outcome = blocking(move || -> ledger_infra::error::Result<MainBatchRead> {
                    let monthly_rows = boot::monthly_summary_for_period(&db, &range)?;
                    let monthly_cells =
                        assembly::assemble_monthly_bar_groups(&monthly_rows, privacy_enabled);
                    let category_shares = boot::category_shares_rows(&db, &range)?;
                    let share_snaps = category_shares.iter().map(ShareSnap::of_share).collect();
                    let table = CategoryTable::from_categories(&boot::category_rows(&db)?);
                    let boundary = boot::report_date_range(&db).ok();
                    Ok(MainBatchRead {
                        monthly_cells,
                        share_snaps,
                        table,
                        boundary,
                    })
                })
                .await;
                if main_seq.get() != seq {
                    return;
                }
                match outcome {
                    Ok(Ok(MainBatchRead {
                        monthly_cells: cells,
                        share_snaps: snaps,
                        table,
                        boundary,
                    })) => {
                        monthly_cells.set(cells);
                        share_snaps.set(snaps);
                        categories.set(table);
                        boundary_dates.set(boundary.map(|dr| (dr.min_date, dr.max_date)));
                        privacy.set(privacy_enabled);
                        main_error.set(None);
                    }
                    Ok(Err(e)) => main_error.set(Some(e.to_string())),
                    Err(_) => main_error.set(Some("内部错误：读任务异常退出".to_owned())),
                }
            });
        });
    };

    let reload_merchant = move || {
        merchant_error.set(None);
        merchant_seq.update(|seq| *seq += 1);
        let seq = merchant_seq.get();
        let Some(range) = session.period.get_untracked() else {
            return;
        };
        let top_n = session.merchant_top_n.get_untracked();
        let db = db_slot().clone();
        let privacy_enabled = amount_privacy_enabled();
        view_owner.run(move || {
            spawn(async move {
                let Some(db) = db else {
                    merchant_error.set(Some("内部错误：账本库会话缺失".to_owned()));
                    return;
                };
                let outcome =
                    blocking(move || boot::merchant_shares_report(&db, &range, Some(top_n))).await;
                if merchant_seq.get() != seq {
                    return;
                }
                match outcome {
                    Ok(Ok(report)) => {
                        privacy.set(privacy_enabled);
                        merchant_cells_raw.set(assembly::assemble_merchant_ranking_rows(
                            &report,
                            privacy_enabled,
                        ));
                        merchant_error.set(None);
                    }
                    Ok(Err(e)) => merchant_error.set(Some(e.to_string())),
                    Err(_) => merchant_error.set(Some("内部错误：读任务异常退出".to_owned())),
                }
            });
        });
    };

    // ============================ 装配投影（渲染期零格式化） ============================
    // 下钻态解析（已下钻但分类不在参考数据 → 面包屑消失回基础态，旧壳
    // drilledRoot computed 同守卫——点击不卡死）。
    let drilled_root = computed(move || {
        let table = categories.get();
        session.drilled_root_id.get().and_then(|id| {
            let name = table.name_of(&id)?;
            Some((id, name))
        })
    });

    // 分类构成图行（基础态/下钻态同一纯函数族，占比分母随层级单源装配——
    // 旧壳 categoryBarsData 同形态；下钻切换纯投影不重拉）。
    let category_bars = computed(move || {
        let table = categories.get();
        let share_rows = share_cn(&share_snaps.get());
        match drilled_root.get() {
            Some((id, _)) => {
                assembly::assemble_category_drilldown_bars(&share_rows, &table, &id, privacy.get())
            }
            None => assembly::assemble_category_bars(&share_rows, &table, privacy.get()),
        }
    });

    // 月度图行面（三柱语义色固定序 = 旧壳 datasets 固定序同形；行格即装载
    // 裹配产物，隐私面翻新只在热读侧）。
    let monthly_faces = computed(move || {
        monthly_cells
            .get()
            .iter()
            .map(|cells| VerticalGroupFace {
                label: cells.month.clone(),
                anchor_prefix: format!("reports-month-{}", cells.month),
                bars: vec![
                    VerticalBarFace {
                        cents: cells.income_cents,
                        color: income_color,
                    },
                    VerticalBarFace {
                        cents: cells.expense_cents,
                        color: expense_color,
                    },
                    VerticalBarFace {
                        cents: cells.refund_cents,
                        color: refund_color,
                    },
                ],
                tooltip: cells.lines.clone(),
            })
            .collect::<Vec<_>>()
    });

    // 分类行面（条长比例基准 = 全行集最大正值；负净额不画条、金额与占比
    // 照实——旧壳负值如实可见的行面近似；柱尾金额与气泡口径单源装配层）。
    let category_faces = computed(move || {
        let bars = category_bars.get();
        let max_positive = bars
            .iter()
            .map(|b| b.amount_cents)
            .fold(0i64, i64::max)
            .max(1);
        bars.iter()
            .map(|bar| {
                let color = match bar.id.as_deref() {
                    Some(id) => category_color(id),
                    None => uncategorized_color(),
                };
                let ratio = (bar.amount_cents as f64 / max_positive as f64).clamp(0.0, 1.0);
                let id_key = bar.id.clone().unwrap_or_else(|| "none".to_owned());
                HorizontalRowFace {
                    name_anchor: format!("reports-cat-{id_key}"),
                    bar_anchor: format!("reports-cat-bar-{id_key}"),
                    name: bar.name.clone(),
                    bar_ratio: ratio,
                    bar_color: color,
                    end_label: bar.amount_text.clone(),
                    tooltip: vec![bar.tooltip_label.clone()],
                    row_height: CATEGORY_ROW_HEIGHT,
                }
            })
            .collect::<Vec<_>>()
    });

    // 商户排行行（旧壳 merchantTableRows 单源零口径同义投影：行集即装载
    // 裹配产物，本 computed 只随隐私面翻新）。
    let merchant_cells = computed(move || merchant_cells_raw.get());

    // ============================ 跳转下钻（消费导航单点，不自建） ============================
    // 载荷 = 分类/商户 + 期间首尾 + 类型集合（支出 + 退款）——列表净值与
    // 柱值一致由载荷显式保证（旧壳同口径）。
    let go_category_transactions = move |category: tx_filter::CategoryFilter| {
        let Some(range) = session.period.get_untracked() else {
            return;
        };
        nav::open(NavRequest {
            target: ViewName::Transactions,
            container_tab: None,
            form_tab: None,
            focus: None,
            tx_filters: Some(tx_filter::DrillFilters {
                account: None,
                merchant: None,
                category: Some(category),
                date_from: Some(range.from),
                date_to: Some(range.to),
                kinds: Some(vec![
                    ledger_transaction::TransactionKind::Expense,
                    ledger_transaction::TransactionKind::Refund,
                ]),
            }),
        });
    };

    let go_merchant_transactions = move |merchant_id: String| {
        let Some(range) = session.period.get_untracked() else {
            return;
        };
        nav::open(NavRequest {
            target: ViewName::Transactions,
            container_tab: None,
            form_tab: None,
            focus: None,
            tx_filters: Some(tx_filter::DrillFilters {
                merchant: Some(merchant_id),
                date_from: Some(range.from),
                date_to: Some(range.to),
                kinds: Some(vec![
                    ledger_transaction::TransactionKind::Expense,
                    ledger_transaction::TransactionKind::Refund,
                ]),
                ..tx_filter::DrillFilters::default()
            }),
        });
    };

    // 点分类行名分派（#379 图内下钻 + #380 跳转下钻的裁决点，与旧壳
    // handleCategoryBarClick 同一决策）：基础态一级柱 → 下钻（或未分类直达）；
    // 下钻态行 → 直达该分类列表。
    let on_category_name_press = move |index: usize| {
        let bars = category_bars.get_untracked();
        let Some(bar) = bars.get(index) else {
            return;
        };
        if drilled_root.get_untracked().is_none() {
            match bar.id.as_deref() {
                None => go_category_transactions(tx_filter::CategoryFilter::UncategorizedOnly),
                Some(id) => {
                    // 下钻分类中途被删（参考数据解析不到）→ 点击无操作不卡死。
                    if categories.get_untracked().category_root(id).is_none() {
                        return;
                    }
                    set_drilldown(&session, Some(id.to_owned()));
                }
            }
            return;
        }
        let Some(id) = bar.id.as_deref() else {
            return;
        };
        go_category_transactions(tx_filter::CategoryFilter::Exact(id.to_owned()));
    };

    // 商户行名按压：直达按该商户过滤的交易列表（#589；含软删商户历史名）。
    let on_merchant_name_press = move |index: usize| {
        let rows = merchant_cells.get_untracked();
        let Some(row) = rows.get(index) else {
            return;
        };
        go_merchant_transactions(row.merchant_id.clone());
    };

    // TopN 档位切换：仅商户卡以新档位重拉，其余两卡不受牵连；同档重复拨动
    // 不重拉（旧壳 watch 同值守卫同形态；删除接线即红钉在 headless）。
    let on_top_n_change = move |n: i64| {
        let changed = session.merchant_top_n.get() != n;
        session.merchant_top_n.set(n);
        if changed {
            reload_merchant();
        }
    };

    // 期间应用（共用期间行 on_apply）：真的变化 → 期间切换复位下钻 → 三卡
    // 重拉；同值应用不动作（同值守卫双通道）。
    let on_period_apply = move |changed: bool| {
        if changed {
            set_drilldown(&session, None);
            reload_main();
            reload_merchant();
        }
    };

    // ============================ ESC 复位出口 ============================
    //（旧壳 registerViewReset(resetToDefault) 同语义：窗口行为守卫在无弹层
    // ESC 时消费——本壳等价形态 = 抑制闸门 + 幂等守卫；弹层开着 ESC 属弹层
    // 关闭面，不动报表态——删除闸门或守卫任一即红钉在 headless ESC 测试）。
    on_key("escape", move |_| {
        if overlay::has_open_overlay() {
            return;
        }
        if is_at_default(&session) {
            return;
        }
        let (period_changed, top_changed) = reset_to_default(&session);
        if period_changed {
            reload_main();
            reload_merchant();
        } else if top_changed {
            reload_merchant();
        }
    });

    // 落位接线：首次装载 + 下钻意图投递即弃（报表路由不读 focus——旧壳同
    // 语义；读一次封闸由 [nav::take_intent] 单点承担）。
    effect(move || {
        let _landed = nav::store().current.get();
        let _ = nav::take_intent();
        reload_main();
        reload_merchant();
    });

    // 期间行（共用期间行 include_all = false：四枚日期芯片无全部，ADR-0057；
    // 步进钳制与芯片选择全部在单点；快照应用回调见上方 on_period_apply 接线）。
    let (period_row_view, _apply_step) = period_row::period_row(
        "reports-period",
        false,
        false,
        session.period,
        boundary_dates,
        on_period_apply,
    );

    // ============================ 布局 ============================
    // 内容区行/条尺寸（窗口宽起扣侧栏与边距；分类表头引旧壳冷区宽形态）。
    let content_width = move || window().width.0 - sidebar::SIDEBAR_WIDTH - 40.0;

    // 分类图区高：行数超限高时容器滚动（全部分类不截断——旧壳 max-height 320
    // × 内部滚动同口径；下钻后行数变少高度收缩，纯视图投影）。
    let category_chart_height = computed(move || {
        let rows = category_bars.get().len().max(6);
        (rows as f32 * CATEGORY_ROW_HEIGHT).min(320.0)
    });

    // 商户表格区高（TopN 恒滚动收口；行数变少高度收缩——旧壳同形态）。
    let merchant_rows_height = computed(move || {
        let rows = merchant_cells.get().len().max(2);
        (rows as f32 * MERCHANT_ROW_HEIGHT + 36.0).min(320.0)
    });

    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(
            // 屏名与期间行（旧壳 QuickTimeRange 单控件：四枚日期芯片无全部
            // + 步进器，ADR-0057；钳制与「今天」在期间行单点）。
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .px_4()
                .py_2()
                .app_class("border-b border-border")
                .child(
                    text(ViewName::Reports.label())
                        .text_size(px(16.0))
                        .text_color(fg)
                        .test_id("view-title"),
                )
                .child(period_row_view),
        )
        .child(
            // 卡片纵列（旧壳 NSpace vertical 卡片页列；窗口内页滚动承载——
            // 三卡总高超视口时页滚动，内容区固定视口不裁卡）。
            scroll()
                .flex_1()
                .child(
                    // 卡片纵列（见上注）。
                    view()
                        .flex_col()
        .child(
            // 月度 + 分类批读错误条位（两卡同一批失败整排呈现；旧壳整页
            // NSpin + toast 的本壳等价：toast → 条位文案，人工对表）。
            when(
                move || main_error.get().is_some(),
                move || {
                    text(move || format!("加载失败：{}", main_error.get().unwrap_or_default()))
                        .text_size(px(12.0))
                        .text_color(danger_fg)
                        .test_id("reports-main-error")
                },
            )
            .otherwise(view),
        )
        .child(
            // ---- 月度收支卡 ----
            view()
                .test_id("reports-monthly-card")
                .flex_col()
                .gap_3()
                .rounded_lg()
                .mx_4()
                .my_2()
                .p_4()
                .app_class("border border-border bg-background")
                .child(text("月度收支").text_size(px(14.0)).text_color(fg))
                .child(
                    when(
                        move || monthly_cells.get().is_empty(),
                        move || {
                            text("本期暂无数据")
                                .text_size(px(13.0))
                                .text_color(muted)
                                .test_id("reports-monthly-empty")
                        },
                    )
                    .otherwise(move || {
                        view().h(px(210.0)).child(bar_chart::vertical_grouped_bars(monthly_faces))
                    }),
                ),
        )
        .child(
            // ---- 支出分类构成卡（含面包屑） ----
            view()
                .test_id("reports-category-card")
                .flex_col()
                .gap_3()
                .rounded_lg()
                .mx_4()
                .my_2()
                .p_4()
                .app_class("border border-border bg-background")
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .child(text("支出分类构成").text_size(px(14.0)).text_color(fg))
                        .child(
                            // 面包屑（issue #379）：下钻态显示当前位置，点根回基础态。
                            when(
                                move || drilled_root.get().is_some(),
                                move || {
                                    view()
                                        .flex_row()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            view()
                                                .test_id("reports-breadcrumb-root")
                                                .on_press(move |_: &PressEvent| {
                                                    set_drilldown(&session, None);
                                                })
                                                .rounded_md()
                                                .child(
                                                    text("全部分类")
                                                        .text_size(px(12.0))
                                                        .text_color(accent),
                                                ),
                                        )
                                        .child(text("›").text_size(px(11.0)).text_color(muted))
                                        .child(
                                            text(move || {
                                                drilled_root
                                                    .get()
                                                    .map(|(_, name)| name)
                                                    .unwrap_or_default()
                                            })
                                            .text_size(px(12.0))
                                            .text_color(fg)
                                            .test_id("reports-breadcrumb-current"),
                                        )
                                },
                            )
                            .otherwise(view),
                        ),
                )
                .child(
                    when(
                        move || category_bars.get().is_empty(),
                        move || {
                            text("暂无支出数据")
                                .text_size(px(13.0))
                                .text_color(muted)
                                .test_id("reports-category-empty")
                        },
                    )
                    .otherwise(move || {
                        // 平铺滚动：图高随行数增长（全部分类不截断），卡片内限高滚动。
                        view()
                            .test_id("reports-category-scroll")
                            .h(move || px(category_chart_height.get()))
                            .child(
                                scroll()
                                    .h(move || px(category_chart_height.get()))
                                    .child(bar_chart::horizontal_bars(
                                        category_faces,
                                        content_width,
                                        on_category_name_press,
                                    )),
                            )
                    }),
                ),
        )
        .child(
            // ---- 商户消费排行卡（TopN 档位 + 表格） ----
            view()
                .test_id("reports-merchant-card")
                .flex_col()
                .gap_3()
                .rounded_lg()
                .mx_4()
                .my_2()
                .p_4()
                .app_class("border border-border bg-background")
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(text("商户消费排行").text_size(px(14.0)).text_color(fg))
                        .child(
                            // TopN 档位（旧壳 NRadioButton 两枚同形态）。
                            view().flex_row().gap_1().children(
                                MERCHANT_TOP_N_OPTIONS.iter().copied().map(|n| {
                                    nsh_base::button()
                                        .selected(move || session.merchant_top_n.get() == n)
                                        .app_class(
                                            "px-2 py-1 rounded-md text-foreground hover:bg-accent selected:bg-accent",
                                        )
                                        .child(
                                            // test_id 落在按钮内文本上（headless press 命中
                                            // 可按压件或其内部文本，期间芯片先例同形）。
                                            text(format!("Top {n}"))
                                                .text_size(px(12.0))
                                                .text_color(fg)
                                                .test_id(format!("reports-topn-{n}").as_str()),
                                        )
                                        .on_press(move |_: &()| on_top_n_change(n))
                                }),
                            ),
                        ),
                )
                .child(
                    when(
                        move || merchant_error.get().is_some(),
                        move || {
                            text(move || {
                                format!("加载失败：{}", merchant_error.get().unwrap_or_default())
                            })
                            .text_size(px(12.0))
                            .text_color(danger_fg)
                            .test_id("reports-merchant-error")
                        },
                    )
                    .otherwise(move || {
                        when(
                            move || merchant_cells.get().is_empty(),
                            move || {
                                text("本期暂无商户消费")
                                    .text_size(px(13.0))
                                    .text_color(muted)
                                    .test_id("reports-merchant-empty")
                            },
                        )
                        .otherwise(move || {
                            view()
                                .test_id("reports-merchant-table")
                                .h(move || px(merchant_rows_height.get()))
                                .child(merchant_table(
                                    merchant_cells,
                                    fg,
                                    muted,
                                    accent,
                                    content_width,
                                    on_merchant_name_press,
                                ))
                        })
                    })
                ),
                        ),
                ),
        )
}

/// 商户排行表（表头 + 行；名 | 金额分布 | 金额 | 占比 | 笔数；行内零口径逻辑，
/// 后端返回序一一映射）。
fn merchant_table(
    cells: Memo<Vec<assembly::MerchantRankingCells>>,
    fg: AdaptiveColor,
    muted: AdaptiveColor,
    accent: AdaptiveColor,
    content_width: impl Fn() -> f32 + Copy + 'static,
    on_name_press: impl Fn(usize) + Copy + 'static,
) -> impl IntoElement {
    view()
        .flex_col()
        .gap_1()
        .child(
            // 列头（旧壳 NDataTable columns 词表）。
            view()
                .flex_row()
                .gap_2()
                .child(
                    text("商户名")
                        .text_size(px(12.0))
                        .text_color(muted)
                        .w(px(MERCHANT_NAME_WIDTH)),
                )
                .child(
                    text("金额分布")
                        .text_size(px(12.0))
                        .text_color(muted)
                        .flex_1(),
                )
                .child(
                    text("金额")
                        .text_size(px(12.0))
                        .text_color(muted)
                        .w(px(MERCHANT_COL_WIDTH)),
                )
                .child(
                    text("占比")
                        .text_size(px(12.0))
                        .text_color(muted)
                        .w(px(MERCHANT_COL_WIDTH)),
                )
                .child(
                    text("笔数")
                        .text_size(px(12.0))
                        .text_color(muted)
                        .w(px(MERCHANT_COL_WIDTH)),
                ),
        )
        .child(each(
            move || cells.get(),
            |r: &assembly::MerchantRankingCells| r.merchant_id.clone(),
            move |item: ReadSignal<assembly::MerchantRankingCells>, index: ReadSignal<usize>| {
                merchant_row(item, index, fg, muted, accent, content_width, on_name_press)
            },
        ))
}

/// 商户排行单行：下钻入口（商户名）| 金额分布内嵌条 | 金额 | 占比 | 笔数
///（列头同构布局；名次色与旧壳 merchantTableRows 同源——色板按名次取色）。
fn merchant_row(
    item: ReadSignal<assembly::MerchantRankingCells>,
    index: ReadSignal<usize>,
    fg: AdaptiveColor,
    muted: AdaptiveColor,
    accent: AdaptiveColor,
    content_width: impl Fn() -> f32 + Copy + 'static,
    on_name_press: impl Fn(usize) + Copy + 'static,
) -> impl IntoElement {
    let pos = index.get_untracked();
    let merchant_id = item.get_untracked().merchant_id.clone();
    let name_test_id = format!("reports-merchant-name-{merchant_id}");
    let bar_anchor = format!("reports-merchant-bar-{merchant_id}");
    // 条长（pt）= 比例 × 轨道宽（轨道 = 内容区宽 − 名列 − 三数值列）；
    // 负净额与 0 比例归 0 不画条。
    let bar_len = move || {
        let track = (content_width() - MERCHANT_NAME_WIDTH - 3.0 * MERCHANT_COL_WIDTH).max(0.0);
        track * item.get().bar_pct as f32 / 100.0
    };
    let amount_text = item.get_untracked().amount_text.clone();
    let share_text = item.get_untracked().share_text.clone();
    let count_text = item.get_untracked().count_text.clone();
    let color = ranking_color(pos);
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(MERCHANT_ROW_HEIGHT))
        .child(
            // 商户名（可点击下钻，MerchantLink 受控模式同位——旧壳 #589）。
            view()
                .test_id(name_test_id.as_str())
                .on_press(move |_: &PressEvent| on_name_press(pos))
                .w(px(MERCHANT_NAME_WIDTH))
                .child(
                    text(move || item.get().name.clone())
                        .text_size(px(13.0))
                        .text_color(accent)
                        .number_of_lines(1),
                ),
        )
        .child(
            // 金额分布内嵌条（first 名 = 色板首位；纯名次实色——旧壳 CSS
            // 渐变的观感差移人工对表子集记录）。
            view()
                .test_id(bar_anchor.as_str())
                .flex_1()
                .h(px(12.0))
                .items_center()
                .child(
                    view()
                        .h(px(10.0))
                        .rounded_full()
                        .w(move || px(bar_len()))
                        .bg(color),
                ),
        )
        .child(
            text(amount_text)
                .text_size(px(13.0))
                .text_color(fg)
                .w(px(MERCHANT_COL_WIDTH)),
        )
        .child(
            text(share_text)
                .text_size(px(13.0))
                .text_color(muted)
                .w(px(MERCHANT_COL_WIDTH)),
        )
        .child(
            text(count_text)
                .text_size(px(13.0))
                .text_color(muted)
                .w(px(MERCHANT_COL_WIDTH)),
        )
}
