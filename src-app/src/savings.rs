//! 储蓄目标视图（issue #2056 NSH 迁移㉕；旧壳 `SavingsGoalsView.vue` +
//! `SavingsGoalFormModal.vue` + `savingsGoals` store 的重生，spec #1750 系列 /
//! ADR-0133）：目标清单卡（进行中）+ 蓄水进度（已存 / 还差 / 达成态，域读路径
//! 单点产出、装配层产格文本，本壳零自算进度）+ 双向推算列（节奏 / ETA /
//! 所需月存，推算算术归域读数）+ 归档列表卡（取消归档 / 删除）+ 新建与编辑
//! 表单弹层（Modal 封装；编辑复用同表单四字段全量替换 + 手填计划月存仅编辑
//! 形态出现）+ 生命周期守卫（删除经二次确认；余额非零由域守卫码化拒绝——
//! 守卫住域写入口单正文，本壳不自建守卫语义）。
//!
//! 进度全部来自后端读数（[`boot::savings_goal_progress_rows`]，旧壳
//! `savingsGoalProgress` 命令消费的同一域函数）；写路径直调域公开写入口
//! （不经 IPC，ADR-0145 决策 4），与旧壳 savings goal 族命令同一执行协议。
//! 达成行仍显带符号差值（词汇表「蓄水进度」），计划提示行只指向计划页处理、
//! 目标域不改计划状态（两域边界）。零新封装件（spec #2031 just-in-time 策略，
//! 组件全量复用 Modal / Danger / Select / DatePicker 已建套件；可清空日期形态
//! [`date_select::date_option_select`] 随本票消费面开门）。
//!
//! 界面文案仅 zh-CN（旧壳 savingsGoals.* zh-CN 词表原样承载）；码化错误 zh
//! 原样呈现；视图本地快照、离开再回 = 全新装载（旧壳 onMounted 同口径）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use ledger_infra::error::AppError;
use nsh_base::{DataTableAlign, DataTableColumn, DataTableRows, button, data_table, input};
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::adaptive;
use crate::app::db_slot;
use crate::assembly::{
    self, CurrencyTable, SAVINGS_GOAL_ARCHIVED_COLUMNS, SAVINGS_GOAL_COLUMNS, SavingsGoalRowCells,
};
use crate::boot;
use crate::danger::{self, DangerOutcome};
use crate::date_select;
use crate::design::font_size;
use crate::modal;
use crate::nav;
use crate::nav_model::ViewName;
use crate::transactions::{amount_color, amount_privacy_enabled, table_classes};
use crate::views::field_label;
use ledger_infra::error::Result as InfraResult;

// ---------------------------------------------------------------------------
// 词表（旧壳 savingsGoals.* zh-CN 词条原样；i18n 框架级方案不在本票，字面承载）。
// ---------------------------------------------------------------------------
const CARD_TITLE: &str = "攒钱";
const NEW_BUTTON: &str = "新建目标";
const EMPTY_GUIDE: &str = "还没有攒钱目标——新建一个，账本会为它自动开一个专属账户";
const ARCHIVED_TITLE: &str = "已归档";
const ACHIEVED_PLAN_HINT: &str = "关联计划仍在执行，请到计划页处理";
const MSG_NAME_REQUIRED: &str = "请填写目标名称";
const MSG_TARGET_INVALID: &str = "目标金额必须大于 0";
const MSG_PLANNED_INVALID: &str = "计划月存必须大于 0";
const MSG_CREATED: &str = "目标已创建，专属账户已就位";
const MSG_UPDATED: &str = "目标已更新";
const MSG_ARCHIVED: &str = "目标已归档，历史与流水保留可查";
const MSG_UNARCHIVED: &str = "目标已恢复为进行中";
const MSG_DELETED: &str = "目标已删除，专属账户一并收起，历史交易仍可查";
const FORM_TITLE_CREATE: &str = "新建攒钱目标";
const FORM_TITLE_EDIT: &str = "编辑攒钱目标";
const PLACEHOLDER_NAME: &str = "如：买车基金";
const PLACEHOLDER_TARGET: &str = "如：50000";
const PLACEHOLDER_PLANNED: &str = "不填即不设计划月存";

/// 储蓄目标视图（导航骨架攒钱分支的真实组件，#2056）。
#[component]
pub(crate) fn SavingsGoalsView() -> impl IntoElement {
    let view_owner = Owner::current();

    // ============================ 快照态 ============================
    // 视图本地快照（离开即回占位态，旧壳重挂同口径）：装配后格行集直持
    //（呈现 / 编辑回填 / 分区的单一事实源；域行无全量 Clone 语义外的面板由
    // 装配行携带原值字段接住——见 `assembly::SavingsGoalRowCells`）。
    let goals = signal(Vec::<SavingsGoalRowCells>::new());
    let currencies = signal(CurrencyTable::default());
    let privacy = signal(false);
    let loading = signal(true);
    let error = signal(None::<String>);
    let load_seq = signal(0u64);

    // 装载批：蓄水进度（域读路径单点口径，同屏多语句读闭包收口在域内
    // ensure_transaction）+ 币种字典，同批读取。任一失败升级为装载错误条位
    // （进度口径缺失不静默回退）。竞态 latest-wins（账户视图先例）。
    let reload = move || {
        loading.set(true);
        error.set(None);
        load_seq.update(|seq| *seq += 1);
        let seq = load_seq.get();
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let Some(db) = db else {
                    error.set(Some("内部错误：账本库会话缺失".to_owned()));
                    loading.set(false);
                    return;
                };
                let outcome = blocking(move || -> GoalLoads {
                    let progress = boot::savings_goal_progress_rows(&db)?;
                    let currency_rows = boot::currency_rows(&db)?;
                    Ok((progress, currency_rows))
                })
                .await;
                if load_seq.get() != seq {
                    return;
                }
                match outcome {
                    Ok(Ok((progress, currency_rows))) => {
                        let currency_table = CurrencyTable::from_currencies(&currency_rows);
                        goals.set(assemble_cells(
                            &progress,
                            &currency_table,
                            privacy.get_untracked(),
                        ));
                        currencies.set(currency_table);
                        privacy.set(amount_privacy_enabled());
                    }
                    Ok(Err(e)) => error.set(Some(e.to_string())),
                    Err(_) => error.set(Some("内部错误：读任务异常退出".to_owned())),
                }
                loading.set(false);
            });
        });
    };

    // 落位：首次装载 + 下钻意图消费（读一次封闸：攒钱路由无自身定位形态，
    // 意图投递即弃——旧壳 savingsGoals 路由不读 focus 同语义）。
    effect(move || {
        let _landed = nav::store().current.get();
        let _ = nav::take_intent();
        reload();
    });

    // 主题令牌（双态自适应；缺失兜底统一归 crate::theme 单点）。
    let crate::views::ViewPalette {
        fg,
        muted,
        danger_fg,
    } = crate::views::view_palette();
    let success_fg = amount_color(assembly::AmountSemantic::Income);

    // 操作反馈条位（NSH 无 toast——#2038 裁决；旧壳 message.success/error 的
    // 条位对应物：新动作开始时清回空）。
    let status = signal(None::<String>);

    // ============================ 行装配与双分区 ============================
    let cells = computed(move || goals.get());
    let partitions = computed(move || assembly::partition_savings_goal_rows(&cells.get()));
    let active_rows = computed(move || partitions.get().0);
    let archived_rows = computed(move || partitions.get().1);
    // 行快照反查（编辑 / 删除的目标）：装配行快照内单点；行缺席（并发删除）
    // 动作静默退场（弹窗不呈现半态）。
    let raw_of = move |goal_id: &str| {
        goals
            .get_untracked()
            .iter()
            .find(|row| row.goal_id == goal_id)
            .cloned()
    };

    // ============================ 表单弹层状态 ============================
    // 新建 / 编辑共用同一弹层语义（编辑复用同表单，回填初值）：`form_open`
    // 控制开合，`edit_goal_id` 区分模式（None = 新建，旧壳 ModalIntent 单成员
    // 意图闭集 + `:key=formSeq` 重建的对应形态——关闭即回占位，字段归信号）。
    let form_open = signal(false);
    let edit_goal_id = signal(None::<String>);
    let form_name = signal(String::new());
    let form_target_text = signal(String::new());
    let form_deadline = signal(None::<chrono::NaiveDate>);
    let form_planned_text = signal(String::new());
    let form_error = signal(None::<String>);
    let is_edit = move || edit_goal_id.get_untracked().is_some();

    let open_create = move || {
        form_name.set(String::new());
        form_target_text.set(String::new());
        form_deadline.set(None);
        form_planned_text.set(String::new());
        form_error.set(None);
        edit_goal_id.set(None);
        form_open.set(true);
    };

    let open_edit = move |goal_id: String| {
        let Some(raw) = raw_of(&goal_id) else {
            return;
        };
        form_name.set(raw.name.clone());
        // 金额回填：按币种小数位从分转元（账户视图先例同口径）。原值字段住
        // 装配行（快照内单点，不回读域行）。
        let dp = currencies
            .get_untracked()
            .format_of(&raw.currency_code)
            .decimal_places;
        form_target_text.set(assembly::cents_to_amount_text(raw.target_cents, dp));
        form_deadline.set(
            raw.deadline_raw
                .as_deref()
                .and_then(date_select::parse_date_text),
        );
        form_planned_text.set(
            raw.planned_monthly_cents
                .map(|cents| assembly::cents_to_amount_text(cents, dp))
                .unwrap_or_default(),
        );
        form_error.set(None);
        edit_goal_id.set(Some(goal_id));
        form_open.set(true);
    };

    let close_form = move || form_open.set(false);

    let perform_save = move || {
        form_error.set(None);
        status.set(None);
        let name = form_name.get_untracked().trim().to_owned();
        if name.is_empty() {
            form_error.set(Some(MSG_NAME_REQUIRED.to_owned()));
            return;
        }
        let Some(target_cents) = assembly::budget_yuan_to_cents(&form_target_text.get_untracked())
        else {
            form_error.set(Some(MSG_TARGET_INVALID.to_owned()));
            return;
        };
        if target_cents <= 0 {
            form_error.set(Some(MSG_TARGET_INVALID.to_owned()));
            return;
        }
        let deadline = form_deadline.get_untracked().map(|date| date.to_string());
        let editing = edit_goal_id.get_untracked().clone();
        let is_update = editing.is_some();

        // 计划月存（结构性只在编辑模式出现）：空 = 清除（None）；填写必须为
        // 正数（与后端 planned-monthly 同校验，客户端双保险防呆）。
        let planned_monthly_cents = if is_update {
            let planned_text = form_planned_text.get_untracked();
            if planned_text.trim().is_empty() {
                None
            } else {
                let Some(cents) = assembly::budget_yuan_to_cents(&planned_text) else {
                    form_error.set(Some(MSG_PLANNED_INVALID.to_owned()));
                    return;
                };
                if cents <= 0 {
                    form_error.set(Some(MSG_PLANNED_INVALID.to_owned()));
                    return;
                }
                Some(cents)
            }
        } else {
            None
        };

        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match (db.as_ref(), editing.as_deref()) {
                    (Some(db), None) => boot::create_savings_goal(
                        db,
                        &ledger_savings_goal::SavingsGoalInput {
                            name,
                            target_amount_cents: target_cents,
                            deadline,
                        },
                    )
                    .map(|_id| ()),
                    (Some(db), Some(id)) => boot::update_savings_goal(
                        db,
                        id,
                        &ledger_savings_goal::SavingsGoalUpdateInput {
                            name,
                            target_amount_cents: target_cents,
                            deadline,
                            planned_monthly_cents,
                        },
                    ),
                    (None, _) => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        close_form();
                        // 成功反馈：编辑 / 新建消息闭集二值（旧壳同词表）。
                        status.set(Some(if is_update {
                            MSG_UPDATED.to_owned()
                        } else {
                            MSG_CREATED.to_owned()
                        }));
                        reload();
                    }
                    Ok(Err(e)) => {
                        // 域码化错误 zh 原样呈现（旧壳 errorMessage 同口径，
                        // 不加二级前缀）；弹窗不关、内容不丢。
                        form_error.set(Some(e.to_string()))
                    }
                    Err(_) => form_error.set(Some("内部错误：写入任务异常退出".to_owned())),
                }
            });
        });
    };

    // ============================ 归档 / 取消归档 / 删除 ============================
    // 归档 / 取消归档即时执行（旧壳无确认）；删除经二次确认（与账户删除同
    // 语义的 danger 对话框）。目标域守卫在写入口先行（boot 同位），错误落
    // 反馈条位。删除目标会级联软删专属账户（余额非零由域守卫码化拒绝）。
    let delete_target = signal(None::<String>);

    let perform_set_archived = move |goal_id: String, archived: bool| {
        status.set(None);
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => {
                        if archived {
                            boot::archive_savings_goal(db, &goal_id)
                        } else {
                            boot::unarchive_savings_goal(db, &goal_id)
                        }
                    }
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        status.set(Some(
                            if archived {
                                MSG_ARCHIVED
                            } else {
                                MSG_UNARCHIVED
                            }
                            .to_owned(),
                        ));
                        reload();
                    }
                    Ok(Err(e)) => status.set(Some(format!(
                        "{}: {e}",
                        if archived {
                            "归档失败"
                        } else {
                            "恢复失败"
                        }
                    ))),
                    Err(_) => status.set(Some("内部错误：写入任务异常退出".to_owned())),
                }
            });
        });
    };

    let perform_delete = move |goal_id: String| {
        status.set(None);
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::delete_savings_goal(db, &goal_id),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        status.set(Some(MSG_DELETED.to_owned()));
                        reload();
                    }
                    Ok(Err(e)) => status.set(Some(format!("删除失败: {e}"))),
                    Err(_) => status.set(Some("内部错误：写入任务异常退出".to_owned())),
                }
            });
        });
    };

    let close_delete = move || delete_target.set(None);
    let delete_spec = move || delete_spec_for(&delete_target.get_untracked(), &raw_of);

    // ============================ 列映射与渲染 ============================
    // 列定义映射（装配列 → 宿主列；富余宽全给名称列——账户视图先例同形）。
    let columns = computed(move || map_columns(SAVINGS_GOAL_COLUMNS, adaptive::content_width()));
    let archived_columns =
        computed(move || map_columns(SAVINGS_GOAL_ARCHIVED_COLUMNS, adaptive::content_width()));

    // 分区表数据 → 宿主表格数据（按列装配，格文本已是装配产物）。
    let data_of = move |partition: Memo<Vec<SavingsGoalRowCells>>, archived: bool| {
        move || {
            let snapshot = partition.get();
            let mut data = DataTableRows::new(snapshot.len())
                .texts("name", snapshot.iter().map(|r| r.name.as_str()))
                .texts("target", snapshot.iter().map(|r| r.target.as_str()))
                .texts("saved", snapshot.iter().map(|r| r.saved.as_str()));
            if !archived {
                data = data
                    .texts("remaining", snapshot.iter().map(|r| r.remaining.as_str()))
                    .texts("projection", snapshot.iter().map(|_| ""))
                    .texts("deadline", snapshot.iter().map(|r| r.deadline.as_str()))
                    .texts("status", snapshot.iter().map(|r| r.status));
            }
            data.texts("actions", snapshot.iter().map(|_| ""))
        }
    };

    // 行操作（旧壳操作列 NButton 同职责）：编辑 / 归档 / 删除（进行中）与
    // 取消归档 / 删除（已归档）。同一行的动作闭包由行快照固定目标。
    let actions_of = move |row: &SavingsGoalRowCells, archived: bool| -> nsh_core::element::View {
        let goal_id = row.goal_id.clone();
        view()
            .flex_row()
            .gap_2()
            .child(action_press("sg-edit", "编辑", fg, move || {
                open_edit(goal_id.clone())
            }))
            .child(if archived {
                let id = row.goal_id.clone();
                action_press("sg-unarchive", "取消归档", fg, move || {
                    perform_set_archived(id.clone(), false)
                })
            } else {
                let id = row.goal_id.clone();
                action_press("sg-archive", "归档", fg, move || {
                    perform_set_archived(id.clone(), true)
                })
            })
            .child({
                let id = row.goal_id.clone();
                action_press("sg-delete", "删除", danger_fg, move || {
                    delete_target.set(Some(id.clone()))
                })
            })
    };

    // 分区表自绘格：推算列 = 逐行小字（各行一条，达成态整列为空）；状态列 =
    // 达成态绿色标签（+ 计划提示行随推算列）；操作列 = 行动作钮。渲染期零
    // 格式化（格文本已装配）。
    let cell_of = move |partition: Memo<Vec<SavingsGoalRowCells>>, archived: bool| {
        move |cell: &nsh_base::DataTableCell| -> nsh_core::element::AnyElement {
            let index = cell.row as usize;
            let assembled = partition.get_untracked().get(index).cloned();
            match cell.key.as_ref() {
                "projection" => {
                    let achieved_plan = assembled
                        .as_ref()
                        .is_some_and(|r| r.achieved && r.plan_hint);
                    let lines = assembled.map(|r| r.projection).unwrap_or_default();
                    view()
                        .flex_col()
                        .test_id("sg-projection")
                        .children(lines.iter().map(|line| {
                            text(line.clone())
                                .text_size(px(font_size::CAPTION))
                                .text_color(muted)
                                .number_of_lines(2)
                        }))
                        .child(
                            // 达成 + 计划仍执行：推算退场但计划提示在场
                            //（只指向计划页处理，目标域不改计划状态）。
                            when(
                                move || achieved_plan,
                                move || {
                                    text(ACHIEVED_PLAN_HINT)
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(muted)
                                        .test_id("sg-plan-hint")
                                },
                            )
                            .otherwise(view),
                        )
                        .into_element()
                }
                "status" => {
                    let achieved = assembled.as_ref().is_some_and(|r| r.achieved);
                    text(cell.text.clone())
                        .text_size(px(font_size::SECONDARY))
                        .text_color(if achieved { success_fg } else { muted })
                        .number_of_lines(1)
                        .into_element()
                }
                "actions" => match assembled {
                    Some(row) => actions_of(&row, archived).into_element(),
                    None => view().into_element(),
                },
                _ => text(cell.text.clone()).number_of_lines(1).into_element(),
            }
        }
    };

    // 分区表构造（两分区同构：同渲染器，数据 / 列随分区；卡片自身 flex_1
    // 给内嵌虚拟化表格撑确定高——#2041 先例）。空态：进行中卡接空态引导
    //（旧壳 empty 槽同文案），归档卡折叠零行（本壳归档卡只在非空渲染）。
    let active_card = move || {
        view()
            .flex_1()
            .flex_col()
            .gap_2()
            .app_class("rounded-(--radius-card) border border-border p-3")
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        view().flex_1().child(
                            text(CARD_TITLE)
                                .text_size(px(font_size::TITLE))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(fg),
                        ),
                    )
                    .child(
                        button()
                            .app_class("px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90")
                            .child(
                                text(NEW_BUTTON)
                                    .text_size(px(font_size::SECONDARY))
                                    .text_color(crate::theme::color("primary-foreground"))
                                    .test_id("sg-new"),
                            )
                            .on_press(move |_: &()| open_create()),
                    ),
            )
            .child(
                view().test_id("sg-table").flex_1().child(
                    data_table()
                        .columns(columns)
                        .data(data_of(active_rows, false))
                        .render_cell(cell_of(active_rows, false))
                        .loading(move || loading.get())
                        .loading_view(text("正在加载…").text_color(muted))
                        .empty(
                            text(EMPTY_GUIDE)
                                .text_size(px(font_size::SECONDARY))
                                .text_color(muted)
                                .test_id("sg-empty"),
                        )
                        .classes(table_classes())
                        .flex_1(),
                ),
            )
    };
    let archived_card = move || {
        view()
            .flex_1()
            .flex_col()
            .gap_2()
            .app_class("rounded-(--radius-card) border border-border p-3")
            .child(
                view().flex_1().child(
                    text(ARCHIVED_TITLE)
                        .text_size(px(font_size::SECONDARY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg),
                ),
            )
            .child(
                view().test_id("sg-archived-table").flex_1().child(
                    data_table()
                        .columns(archived_columns)
                        .data(data_of(archived_rows, true))
                        .render_cell(cell_of(archived_rows, true))
                        .loading(move || loading.get())
                        .loading_view(text("正在加载…").text_color(muted))
                        .classes(table_classes())
                        .flex_1(),
                ),
            )
    };

    // 表单弹层内容（视图树内联函数式插槽）：新建与编辑共用；计划月存结构性
    // 只在编辑模式出现；保存错误条位贴域错误。
    let form_content = move || {
        view()
            .flex_col()
            .gap_2()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label("名称", muted))
                    .child(
                        input()
                            .test_id("sg-form-name")
                            .value(move || form_name.get())
                            .placeholder(PLACEHOLDER_NAME)
                            .optional_label("目标名称")
                            .text_color(fg)
                            .w(px(240.0))
                            .app_class(
                                "bg-background rounded-(--radius) border border-border px-2 py-1 \
                                 placeholder:text-muted-foreground",
                            )
                            .on_change(move |text: &str| form_name.set(text.to_owned())),
                    ),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label("目标金额", muted))
                    .child(
                        input()
                            .test_id("sg-form-amount")
                            .value(move || form_target_text.get())
                            .placeholder(PLACEHOLDER_TARGET)
                            .optional_label("目标金额")
                            .text_color(fg)
                            .w(px(160.0))
                            .app_class(
                                "bg-background rounded-(--radius) border border-border px-2 py-1 \
                                 placeholder:text-muted-foreground",
                            )
                            .on_change(move |text: &str| {
                                form_target_text.set(text.to_owned())
                            }),
                    ),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label("截止日期", muted))
                    .child(date_select::date_option_select(
                        "sg-form-deadline",
                        form_deadline,
                    )),
            )
            .child(
                // 手填计划月存：结构性只在编辑模式出现（新建走 create 入参，
                // 不携带该字段——旧壳 v-if="editing" 同口径）。
                when(is_edit, move || {
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(field_label("计划月存", muted))
                        .child(
                            input()
                                .test_id("sg-form-planned")
                                .value(move || form_planned_text.get())
                                .placeholder(PLACEHOLDER_PLANNED)
                                .optional_label("计划月存")
                                .text_color(fg)
                                .w(px(160.0))
                                .app_class(
                                    "bg-background rounded-(--radius) border border-border px-2 py-1 \
                                     placeholder:text-muted-foreground",
                                )
                                .on_change(move |text: &str| {
                                    form_planned_text.set(text.to_owned())
                                }),
                        )
                })
                .otherwise(view),
            )
            .child(
                // 保存错误条位（域码化错误 zh 原样；下次尝试开始时清空）。
                when(
                    move || form_error.get().is_some(),
                    move || {
                        // 拦截提示与域码化错误同位原样呈现（旧壳 message 展示
                        // 同口径，不加二级前缀）。
                        text(move || form_error.get().unwrap_or_default())
                            .text_size(px(font_size::CAPTION))
                            .text_color(danger_fg)
                            .test_id("sg-form-error")
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
                        button()
                            .app_class(
                                "px-3 py-1.5 rounded-(--radius) border border-border text-foreground hover:bg-accent",
                            )
                            .child(
                                text("取消")
                                    .text_size(px(font_size::SECONDARY))
                                    .text_color(fg)
                                    .test_id("sg-form-cancel"),
                            )
                            .on_press(move |_: &()| close_form()),
                    )
                    .child(
                        button()
                            .app_class("px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90")
                            .child(
                                text("保存")
                                    .text_size(px(font_size::SECONDARY))
                                    .text_color(crate::theme::color("primary-foreground"))
                                    .test_id("sg-form-save"),
                            )
                            .on_press(move |_: &()| perform_save()),
                    ),
            )
    };

    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(
            view().px_4().py_2().child(
                text(ViewName::SavingsGoals.label())
                    .text_size(px(font_size::TITLE))
                    .text_color(fg)
                    .test_id("view-title"),
            ),
        )
        .child(
            // 装载失败条位（码化错误 zh-CN 原样，不静默——进度口径缺失不静默
            // 回退空清单）。
            when(
                move || error.get().is_some(),
                move || {
                    text(move || format!("加载失败：{}", error.get().unwrap_or_default()))
                        .text_size(px(font_size::CAPTION))
                        .text_color(danger_fg)
                        .test_id("sg-error")
                        .app_class("px-4 py-1")
                },
            )
            .otherwise(view),
        )
        .child(
            // 内容区（账户视图同形态的字段卡片列；表格虚拟化按自身高度裁行
            //——根内容列必须 flex_1，卡片弹性高才有确定高来源，#2041 先例）。
            view()
                .flex_1()
                .flex_col()
                .gap_3()
                .p_4()
                .child(active_card())
                // 归档卡（旧壳 v-if 归档非零才渲染；折叠零行不呈现）。
                .child(when(move || !archived_rows.get().is_empty(), archived_card).otherwise(view))
                .child(
                    // 操作反馈条位（成功/失败共用一条；新动作开始清回空）。
                    when(
                        move || status.get().is_some(),
                        move || {
                            text(move || status.get().unwrap_or_default())
                                .text_size(px(font_size::CAPTION))
                                .text_color(fg)
                                .test_id("sg-status")
                        },
                    )
                    .otherwise(view),
                )
                .child(
                    // 新建 / 编辑表单弹层（标题随模式；显示由意图非空派生）。
                    modal::app_modal(
                        move || form_open.get(),
                        move || {
                            if edit_goal_id.get_untracked().is_some() {
                                FORM_TITLE_EDIT.to_owned()
                            } else {
                                FORM_TITLE_CREATE.to_owned()
                            }
                        },
                        close_form,
                        form_content,
                    ),
                )
                .child(
                    // 删除确认（error 级：目标与专属账户一并收起不可恢复——
                    // 旧壳二确认文案在框；余额非零时写入口码化拒绝）。
                    when(
                        move || delete_target.get().is_some(),
                        move || {
                            danger::danger_confirm(
                                move || delete_target.get().is_some(),
                                delete_spec(),
                                move |outcome: DangerOutcome| match outcome {
                                    DangerOutcome::Confirmed => {
                                        if let Some(id) = delete_target.get_untracked() {
                                            perform_delete(id);
                                        }
                                        close_delete();
                                    }
                                    DangerOutcome::Dismissed => close_delete(),
                                },
                            )
                        },
                    )
                    .otherwise(view),
                ),
        )
}

// ---------------------------------------------------------------------------
// 视图局部辅助（列映射与快照装配单点：UI 层只做映射不产列）。
// ---------------------------------------------------------------------------

/// 装载批产物（读接缝单点的具名元组）：蓄水进度（域读路径单点口径）+ 币种
/// 字典——`blocking` 读闭包返回值，复杂度聚合单点。
type GoalLoads = InfraResult<(
    Vec<ledger_savings_goal::SavingsGoalProgress>,
    Vec<ledger_currencies::Currency>,
)>;

/// 快照 → 装配单点（计算的入口收窄到一处，便于 headless 断言对准装配面）。
fn assemble_cells(
    rows: &[ledger_savings_goal::SavingsGoalProgress],
    currencies: &CurrencyTable,
    privacy: bool,
) -> Vec<SavingsGoalRowCells> {
    assembly::assemble_savings_goal_rows(rows, currencies, privacy)
}

/// 表格外围定宽扣减（储蓄列宽计算输入）：内容列 p-4（两侧 16pt）+ 分区卡
/// p-3（两侧 12pt）+ 分区卡描边两侧 1pt——与账户视图同形。窄窗自适应随系列
/// 尾部横切票。
const SAVINGS_TABLE_WIDTH_PAD: f32 = 16.0 * 2.0 + 12.0 * 2.0 + 2.0;

/// 装配列 → 宿主列（UI 层只做映射不产列；弹性宽度分配同账户视图先例——列
/// 组件无弹性列机制，富余宽度全给名称列；总宽不得超出表格实测可用宽，尾列
/// 缺席即操作钮灭失）。
fn map_columns(defs: &[assembly::ColumnDef], width: f32) -> Vec<DataTableColumn> {
    let mut columns: Vec<DataTableColumn> = defs
        .iter()
        .map(|def| {
            let mut column = DataTableColumn::default();
            column.key = def.key.into();
            column.title = Some(def.title.into());
            column.width = Some(Points(def.width));
            column.align = match def.align {
                assembly::ColumnAlign::Start => DataTableAlign::Start,
                assembly::ColumnAlign::End => DataTableAlign::End,
            };
            column
        })
        .collect();
    let suggested: f32 = defs.iter().map(|def| def.width).sum();
    let extra = (width - SAVINGS_TABLE_WIDTH_PAD - suggested).max(0.0);
    if extra > 0.0
        && let Some(name) = columns.iter_mut().find(|col| col.key == "name")
        && let Some(w) = name.width
    {
        name.width = Some(Points(w.0 + extra));
    }
    columns
}

/// 行动作钮（文本钮同位旧壳操作列 NButton；test_id 是按压唯一锚点）。
fn action_press(
    id: &'static str,
    label: &'static str,
    color: AdaptiveColor,
    action: impl Fn() + 'static,
) -> nsh_core::element::View {
    view()
        .test_id(id)
        .app_class("rounded-sm hover:bg-accent px-1")
        .on_press(move |_: &PressEvent| action())
        .child(
            text(label)
                .text_size(px(font_size::CAPTION))
                .text_color(color),
        )
}

/// 删除确认 spec（error 级，旧壳 dialog.warning 二确认文案逐字同位）：后果句
/// 携「余额非零被拒」说明与目标名点名。
fn delete_spec_for(
    goal_id: &Option<String>,
    raw_of: &impl Fn(&str) -> Option<SavingsGoalRowCells>,
) -> danger::DangerConfirmSpec {
    let name = goal_id
        .as_deref()
        .and_then(raw_of)
        .map(|raw| raw.name.clone())
        .unwrap_or_default();
    danger::DangerConfirmSpec {
        level: danger::DangerLevel::Error,
        title: "删除攒钱目标".to_owned(),
        lead: Some(format!("确认删除「{name}」？")),
        strong_warning: Some(
            "删除后目标与专属账户一并收起，历史交易保留可查；账户仍有余额时将被拒绝。".to_owned(),
        ),
        detail: None,
        confirm_label: "删除".to_owned(),
        cancel_label: "取消".to_owned(),
    }
}
