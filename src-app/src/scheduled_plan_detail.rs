//! 期次详情弹层（issue #2042；旧壳 `PlanDetailModal.vue` 的重生——订阅 /
//! 分期 / 定时转账三页签通用，分期 / 转账随定时②复用）。
//!
//! 数据走既有计划详情读接缝（[`crate::boot::scheduled_plan_detail`]，含全量
//! 期次按 scheduled_date 升序）；操作仅两个——
//! - 失败期次「重试」：走既有单期执行写接缝（仅对 failed 状态开放，不做
//!   提前执行 pending；执行成功产生真实交易行，交易失效信号的广播归消费
//!   侧，本模块零信号依赖）；
//! - 「展开更多期次」：走既有期次展开写接缝（预生成窗口之外）。
//!
//! 弹窗按 id 独立取数（不受清单状态过滤影响——已取消计划照常可开，旧壳
//! usePlanFocusLanding 注释同口径）；重试 / 展开成功后同弹重拉详情，并经
//! `on_changed` 回调消费侧（清单 / 花费面板重拉收口在消费侧）。
//!
//! 界面文案仅 zh-CN；码化错误 zh 原样呈现。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_core::prelude::*;

use crate::assembly::{ColumnAlign, CurrencyTable};
use crate::boot;
use crate::design::font_size;
use crate::modal;
use crate::scheduled::{amount_text, occurrence_status_label};

/// 弹层标题（旧壳 `scheduled.detail.title`）。
const DETAIL_TITLE: &str = "期次详情";

/// 单个期次的可比较快照（行呈现 / 重试目标承载）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OccSnap {
    pub id: String,
    pub scheduled_date: String,
    pub status: String,
    pub amount_cents: i64,
}

/// 计划详情弹层装载快照（弹窗按 id 独立取数后的投影）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DetailSnap {
    pub plan_id: String,
    pub note: Option<String>,
    pub currency_code: String,
    pub status: String,
    /// 计划总期数（分期 / 有限期数定时转账有值；订阅与无限循环 None）。
    pub total_occurrences: Option<i64>,
    /// 全量期次（含 pending/processing/completed/failed/cancelled），按
    /// scheduled_date 升序。
    pub occurrences: Vec<OccSnap>,
}

impl DetailSnap {
    /// 失败期次（重试门控的判据载体）。
    pub(crate) fn failed_occurrence(&self, id: &str) -> Option<&OccSnap> {
        self.occurrences
            .iter()
            .find(|occ| occ.id == id && occ.status == "failed")
    }
}

/// 弹层内反馈文案（旧壳 `scheduled.detail.*` 同词表；新动作开始时清回空）。
pub(crate) const MSG_RETRY_OK: &str = "重试成功，交易已入账";
pub(crate) const MSG_EXPAND_NONE: &str = "没有更多期次了";
pub(crate) const MSG_LOAD_FAILED: &str = "加载失败";

/// 弹层装载态（视图信号持有；关闭即整弹窗卸载归默认）。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DetailState {
    /// 装载快照（None = 在途 / 失败——失败时呈现「加载失败」）。
    pub detail: Option<DetailSnap>,
    pub load_failed: bool,
    /// 重试 / 展开的反馈条文案（None = 无）。
    pub note: Option<String>,
    /// 正在重试的期次 id（防重复点击）。
    pub retrying: Option<String>,
    /// 正在展开期次（防重复点击）。
    pub expanding: bool,
}

/// 域详情读路径 → 弹层快照（装载批单点投影）。排序对准旧壳
/// `occurrenceRows`（scheduled_date 升序）；总期数从扩展字段解析（已生成
/// 数 == 全量期次行数，旧壳 occurrenceRows.value.length 同口径）。
pub(crate) fn detail_snap_of(detail: ledger_scheduled::ScheduledTransactionDetail) -> DetailSnap {
    let total_occurrences = detail
        .extension
        .get("total_occurrences")
        .and_then(|v| v.as_i64());
    let mut occurrences = detail
        .occurrences
        .iter()
        .map(|occ| OccSnap {
            id: occ.id.clone(),
            scheduled_date: occ.scheduled_date.clone(),
            status: occ.status.clone(),
            amount_cents: occ.amount_cents,
        })
        .collect::<Vec<_>>();
    occurrences.sort_by(|a, b| a.scheduled_date.cmp(&b.scheduled_date));
    let core = detail.core;
    DetailSnap {
        plan_id: core.id.clone(),
        note: core.note.clone(),
        currency_code: core.currency_code.clone(),
        status: core.status.clone(),
        total_occurrences,
        occurrences,
    }
}

/// 「展开更多期次」门控（旧壳 canExpand 同口径）：仅 active 计划可展开（与
/// 后端同口径）；有限期数计划期次已全部生成时不再可展开——避免无意义的
/// 空展开。
fn can_expand(detail: &DetailSnap) -> bool {
    if detail.status != "active" {
        return false;
    }
    match detail.total_occurrences {
        None => true,
        Some(total) => detail.occurrences.len() < total as usize,
    }
}

/// 弹层表数据（消费面闭包读弹层信号快照；重试目标按行持有）。追踪读（
/// DataTable 的 data prop 响应式闭包，订阅页签清单先例同口径）。
fn occ_rows_data(
    state: &Signal<DetailState>,
    currencies: &Signal<CurrencyTable>,
    privacy: &Signal<bool>,
) -> nsh_base::DataTableRows {
    let snapshot = state.get();
    let Some(detail) = snapshot.detail.as_ref() else {
        return nsh_base::DataTableRows::new(0).texts("date", std::iter::empty::<&str>());
    };
    let fmt = currencies.get().format_of(&detail.currency_code);
    let mask = privacy.get();
    nsh_base::DataTableRows::new(detail.occurrences.len())
        .texts(
            "date",
            detail.occurrences.iter().map(|o| o.scheduled_date.as_str()),
        )
        .texts(
            "amount",
            detail
                .occurrences
                .iter()
                .map(|o| amount_text(o.amount_cents, &fmt, mask)),
        )
        .texts(
            "status",
            detail
                .occurrences
                .iter()
                .map(|o| occurrence_status_label(&o.status)),
        )
        .texts("actions", detail.occurrences.iter().map(|_| ""))
}

/// 期次表列定义（旧壳 columns 四列：日期 / 金额 / 状态 / 操作）。
fn occ_columns() -> Vec<nsh_base::DataTableColumn> {
    let defs = [
        ("date", "日期", 90.0, ColumnAlign::Start),
        ("amount", "金额", 70.0, ColumnAlign::End),
        ("status", "状态", 70.0, ColumnAlign::Start),
        ("actions", "操作", 60.0, ColumnAlign::Start),
    ];
    defs.into_iter()
        .map(|(key, title, width, align)| {
            let mut column = nsh_base::DataTableColumn::default();
            column.key = key.into();
            column.title = Some(title.into());
            column.width = Some(Points(width));
            column.align = match align {
                ColumnAlign::Start => nsh_base::DataTableAlign::Start,
                ColumnAlign::End => nsh_base::DataTableAlign::End,
            };
            column
        })
        .collect()
}

/// 期次详情弹层（消费面：订阅页签的「期次」行动作与来源落点的 focus 开窗）。
///
/// `open_id` 受控开合——Some(id) = 开窗并按 id 拉详情，None = 关闭
///（调用方置回 None 终态）；重试 / 展开成功经 `on_changed` 回调消费侧（清单 / 花费面板
/// 重拉与交易落库的失效广播收口在订阅页签，本模块零信号依赖）。
///
/// 删除即红条目（headless 钉点）：开窗装载 effect（删 → 期次表永不呈现）；
/// 重试接线（删 → 交易不落库、反馈不呈现）；重拉接线（删 → 行前旧值）。
pub(crate) fn plan_detail_modal(
    open_id: Signal<Option<String>>,
    currencies: Signal<CurrencyTable>,
    privacy: Signal<bool>,
    on_changed: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    let view_owner = Owner::current();
    let state = signal(DetailState::default());
    // 详情重拉序号（重试 / 展开成功后递增驱动同弹重读——开窗 effect 键是
    // (open_id, epoch)：重拉不清空重试 / 反馈等弹窗内既有状态）。
    let reload_epoch = signal(0u64);
    let load_seq = signal(0u64);

    // 开窗 / 重拉装载（读一次封闸语义归 open_id；按 id 独立取数）。阻塞读
    // 走后台线程，任务挂视图实例 owner（弹窗子树因编辑回报关闭时任务不
    // 中断——交易视图 perform_delete 先例同形）。
    effect(move || {
        let Some(id) = open_id.get() else {
            return;
        };
        reload_epoch.get_untracked();
        load_seq.update(|seq| *seq += 1);
        let seq = load_seq.get_untracked();
        let db = crate::app::db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::scheduled_plan_detail(db, &id),
                    None => Err(ledger_infra::error::AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                // 过期装载不落位（浏览与重拉竞态的 latest-wins，搜索视图先例）。
                if load_seq.get_untracked() != seq {
                    return;
                }
                state.update(|s| match outcome {
                    Ok(Ok(detail)) => {
                        s.detail = Some(detail_snap_of(detail));
                        s.load_failed = false;
                    }
                    Ok(Err(_)) | Err(_) => {
                        s.detail = None;
                        s.load_failed = true;
                    }
                });
            });
        });
    });

    // 重试（仅 failed 期次开放；防重复点击）：执行成功 = 交易落库 → 弹窗内
    // 成功反馈 + 重拉详情 + 向消费侧回报 changed（清单 / 花费重拉与交易失效
    // 广播在消费侧收口）。失败呈现反馈条位不重拉（期次保持原状态可重试）。
    let perform_retry = move |occ_id: String| {
        let snapshot = state.get_untracked();
        let Some(detail) = snapshot.detail.as_ref() else {
            return;
        };
        if snapshot.retrying.is_some() {
            return;
        }
        let Some(occ) = detail.failed_occurrence(&occ_id) else {
            return;
        };
        let _ = occ;
        state.update(|s| {
            s.retrying = Some(occ_id.clone());
            s.note = None;
        });
        let db = crate::app::db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::execute_scheduled_occurrence(db, &occ_id),
                    None => Err(ledger_infra::error::AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                let landed = matches!(&outcome, Ok(Ok(_)));
                state.update(|s| {
                    s.retrying = None;
                    match outcome {
                        Ok(Ok(_)) => s.note = Some(MSG_RETRY_OK.to_owned()),
                        Ok(Err(e)) => s.note = Some(format!("重试失败: {e}")),
                        Err(_) => s.note = Some("重试失败: 内部错误：写入任务异常退出".to_owned()),
                    }
                });
                if landed {
                    reload_epoch.update(|n| *n += 1);
                    on_changed();
                }
            });
        });
    };

    // 展开更多期次（门控 canExpand；成功 / 空转反馈 + 重拉详情）。
    let perform_expand = move || {
        let snapshot = state.get_untracked();
        let Some(detail) = snapshot.detail.as_ref() else {
            return;
        };
        if !can_expand(detail) || snapshot.expanding {
            return;
        }
        let id = detail.plan_id.clone();
        state.update(|s| {
            s.expanding = true;
            s.note = None;
        });
        let db = crate::app::db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::expand_scheduled_occurrences(db, &id),
                    None => Err(ledger_infra::error::AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                let expanded = matches!(&outcome, Ok(Ok(_)));
                state.update(|s| {
                    s.expanding = false;
                    match outcome {
                        Ok(Ok(ids)) => {
                            s.note = Some(if ids.is_empty() {
                                MSG_EXPAND_NONE.to_owned()
                            } else {
                                format!("已生成 {} 期", ids.len())
                            });
                        }
                        Ok(Err(e)) => s.note = Some(format!("展开失败: {e}")),
                        Err(_) => s.note = Some("展开失败: 内部错误：写入任务异常退出".to_owned()),
                    }
                });
                if expanded {
                    reload_epoch.update(|n| *n += 1);
                }
            });
        });
    };

    let (fg, muted, danger_fg) = detail_palette();

    // 弹层表自绘格（重试按钮列按行固定目标：failed 行渲染「重试」、其余
    // 占位「—」；正在重试的行按钮先禁用防重复）。格文本已是装配产物。
    let render_cell = move |cell: &nsh_base::DataTableCell| -> nsh_core::element::AnyElement {
        let detail_snapshot = state.get_untracked();
        let Some(detail) = detail_snapshot.detail.clone() else {
            return text(cell.text.clone()).into_element();
        };
        let Some(occ) = detail.occurrences.get(cell.row as usize).cloned() else {
            return text(cell.text.clone()).into_element();
        };
        match cell.key.as_ref() {
            "date" => text(cell.text.clone())
                .text_size(px(font_size::CAPTION))
                .text_color(fg)
                .test_id(format!("occ-date-{}", occ.id).as_str())
                .into_element(),
            "status" => text(cell.text.clone())
                .text_size(px(font_size::CAPTION))
                .text_color(if occ.status == "failed" {
                    danger_fg
                } else {
                    fg
                })
                .test_id(format!("occ-status-{}", occ.id).as_str())
                .into_element(),
            "actions" => {
                if occ.status == "failed" {
                    let occ_id = occ.id.clone();
                    nsh_base::button()
                        .disabled(move || state.get().retrying.as_deref() == Some(&occ_id))
                        .app_class(
                            "px-2 py-1 rounded-md text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .child(
                            text("重试")
                                .text_size(px(font_size::CAPTION))
                                .text_color(fg)
                                .test_id(format!("occ-retry-{}", occ.id).as_str()),
                        )
                        .on_press(move |_: &()| perform_retry(occ.id.clone()))
                        .into_element()
                } else {
                    text("—")
                        .text_size(px(font_size::CAPTION))
                        .text_color(muted)
                        .into_element()
                }
            }
            _ => text(cell.text.clone()).number_of_lines(1).into_element(),
        }
    };

    // 弹层内容：常挂框架 + 状态位分段（when 分支承载未装载 / 失败 / 摘要 /
    // 展开的显隐与渲染），未装载段以空占位收场——结构性响应式只能由 when
    // 承担（内容闭包首渲染后不重跑，信号变化的显隐归 when 的响应式子树，
    // 交易表单错误条位同形）。
    let body = move || {
        view()
            .flex_col()
            .gap_2()
            // 读失败失败态（不静默——失败时摘要与期次缺席）。
            .child(
                when(
                    move || state.get().load_failed,
                    move || {
                        text(MSG_LOAD_FAILED)
                            .text_size(px(font_size::CAPTION))
                            .text_color(muted)
                            .test_id("occ-load-failed")
                    },
                )
                .otherwise(view),
            )
            // 计划摘要（备注 + 总期数在册呈现；未装载期间整行缺席）。
            .child(
                when(
                    move || state.get().detail.is_some(),
                    move || {
                        view()
                            .flex_row()
                            .items_center()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                text(move || match state.get().detail {
                                    Some(detail) => {
                                        detail.note.unwrap_or_else(|| "（无备注）".to_owned())
                                    }
                                    None => String::new(),
                                })
                                .text_size(px(font_size::SECONDARY))
                                .test_id("occ-plan-note"),
                            )
                            .child(
                                when(
                                    move || {
                                        state
                                            .get()
                                            .detail
                                            .as_ref()
                                            .and_then(|d| d.total_occurrences)
                                            .is_some()
                                    },
                                    move || {
                                        text(move || match state.get().detail {
                                            Some(detail) => detail
                                                .total_occurrences
                                                .map(|n| format!("共 {n} 期"))
                                                .unwrap_or_default(),
                                            None => String::new(),
                                        })
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(muted)
                                        .test_id("occ-total")
                                    },
                                )
                                .otherwise(view),
                            )
                    },
                )
                .otherwise(view),
            )
            // 期次表（弹窗内定高：虚拟化需确定高；数据闭包未装载期自带 0 行，
            // 装载完成即行即现）。
            .child(
                view().test_id("occ-table").flex_col().h(px(320.0)).child(
                    nsh_base::data_table()
                        .columns(occ_columns())
                        .data(move || occ_rows_data(&state, &currencies, &privacy))
                        .render_cell(render_cell)
                        .classes(crate::transactions::table_classes())
                        .flex_1(),
                ),
            )
            .child(
                when(
                    move || {
                        state
                            .get()
                            .detail
                            .as_ref()
                            .is_some_and(|d| d.occurrences.is_empty())
                    },
                    move || {
                        text("暂无期次")
                            .text_size(px(font_size::CAPTION))
                            .text_color(muted)
                            .test_id("occ-empty")
                    },
                )
                .otherwise(view),
            )
            .child(
                when(
                    move || state.get().note.is_some(),
                    move || {
                        text(move || state.get().note.unwrap_or_default())
                            .text_size(px(font_size::CAPTION))
                            .text_color(muted)
                            .test_id("occ-note")
                    },
                )
                .otherwise(view),
            )
            .child(
                when(
                    move || {
                        let snapshot = state.get();
                        snapshot
                            .detail
                            .as_ref()
                            .is_some_and(|d| can_expand(d) && !d.occurrences.is_empty())
                    },
                    move || {
                        nsh_base::button()
                            .disabled(move || state.get().expanding)
                            .app_class(
                                "px-3 py-2 rounded-md border border-border text-foreground \
                                 hover:bg-accent disabled:opacity-40",
                            )
                            .child(
                                text("展开更多期次")
                                    .text_size(px(font_size::SECONDARY))
                                    .test_id("occ-expand"),
                            )
                            .on_press(move |_: &()| perform_expand())
                    },
                )
                .otherwise(view),
            )
    };

    modal::app_modal(
        move || open_id.get().is_some(),
        || DETAIL_TITLE.to_owned(),
        move || open_id.set(None),
        body,
    )
}

/// 弹层令牌便捷面（前景 / 弱化 / 危险）。
fn detail_palette() -> (
    nsh_core::element::AdaptiveColor,
    nsh_core::element::AdaptiveColor,
    nsh_core::element::AdaptiveColor,
) {
    (
        crate::theme::color("foreground"),
        crate::theme::color("muted-foreground"),
        crate::theme::color("destructive"),
    )
}
