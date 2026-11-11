//! 账户视图（issue #2041 NSH 迁移⑩；旧壳 `AccountsView.vue` 的重生）。
//!
//! 实现明细见各区块注释；整体职责边界（按旧壳核实平价）：账户余额清单
//! （三分区：攒钱 / 主表 / 页底已归档）+ 新建表单卡 + 编辑账户弹窗 +
//! 余额调整弹窗 + 行操作（编辑 / 调整余额 / 归档 / 恢复 / 删除，右键与
//! 「⋯」两入口同 options）；余额读 V017 持久化缓存（ADR-0067）只消费不另立
//! 算术；写路径直调域公开写入口（不经 IPC，ADR-0145 决策 4），与旧壳账户族
//! 命令同一执行协议（含删除 / 归档先经目标域守卫的命令壳同位）。零新封装件
//! （spec #2031 just-in-time 策略，组件全量复用 Modal / Danger / Select /
//! DatePicker 已建套件）。
//!
//! 界面文案仅 zh-CN；码化错误 zh 原样呈现；视图本地状态、会话内保留不做
//! （离开再回 = 全新装载，旧壳 onMounted 同口径）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::{
    DataTableAlign, DataTableColumn, DataTableContextMenu, DataTableRows, button, data_table, input,
};
use nsh_core::element::FontWeight;
use nsh_core::element::desktop::MenuItem;
use nsh_core::prelude::*;

use crate::adaptive;
use crate::app::db_slot;
use crate::assembly::{
    self, ACCOUNT_COLUMNS, AccountListContext, AccountRowCells, ColumnAlign, CurrencyTable,
};
use crate::boot;
use crate::context_menu;
use crate::danger::{self, DangerOutcome};
use crate::date_select;
use crate::design::font_size;
use crate::modal;
use crate::nav;
use crate::nav_model::{NavRequest, ViewName};
use crate::select;
use crate::signals;
use crate::transactions::{amount_color, amount_privacy_enabled, table_classes};
use crate::tx_filter;
use crate::views::field_label;
use ledger_infra::error::AppError;

// ---------------------------------------------------------------------------
// 词表（旧壳 accounts.* zh-CN 词条原样；i18n 框架级方案不在本票，字面承载）。
// ---------------------------------------------------------------------------
const CREATE_CARD_TITLE: &str = "新增账户";
const GOAL_CARD_TITLE: &str = "攒钱";
const LIST_CARD_TITLE: &str = "账户列表";
const ARCHIVED_TITLE: &str = "已归档";
const MSG_NAME_REQUIRED: &str = "请输入账户名称";
const MSG_CREATED: &str = "已创建账户";
const MSG_DELETED: &str = "已删除";
const MSG_SAVED: &str = "已保存";
const MSG_ADJUSTED: &str = "余额已调整";
const MSG_ARCHIVED: &str = "账户已归档，历史与流水保留可查";
const MSG_RESTORED: &str = "账户已恢复";
const MSG_TYPE_REQUIRED: &str = "请选择账户类型";
const MSG_CURRENCY_REQUIRED: &str = "请选择币种";

/// 账户视图（导航骨架账户分支的真实组件，#2041）。
#[component]
pub(crate) fn AccountsView() -> impl IntoElement {
    let view_owner = Owner::current();

    // ============================ 快照态 ============================
    // 视图本地快照（离开即回占位态，旧壳重挂同口径）。
    let balances = signal(BalancesSnap::default());
    let currencies = signal(CurrencyTable::default());
    let goals = signal(std::collections::BTreeSet::<String>::new());
    let plans = signal(PlansSnap::default());
    let privacy = signal(false);
    let loading = signal(true);
    let error = signal(None::<String>);
    let load_seq = signal(0u64);

    // 装载批：余额（V017 缓存口径）+ 币种字典 + 目标绑定集 + 计划引用集，
    // 同批读取。计划读失败按空集处理（计划数提示是佐证信息不是守卫，读失败
    // 不挡收纳动作，旧壳 plan-usage 消费同口径）；其余读任一失败升级为装载
    // 错误条位（分组口径缺失不静默回退）。竞态 latest-wins（搜索视图先例）。
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
                let outcome = blocking(move || -> AccountLoads {
                    let balance_rows = boot::account_balance_rows(&db)?;
                    let currency_rows = boot::currency_rows(&db)?;
                    let goal_rows = boot::savings_goal_progress_rows(&db)?;
                    let plan_rows = boot::plan_rows(&db).ok().unwrap_or_default();
                    Ok((balance_rows, currency_rows, goal_rows, plan_rows))
                })
                .await;
                if load_seq.get() != seq {
                    return;
                }
                match outcome {
                    Ok(Ok((balance_rows, currency_rows, goal_rows, plan_rows))) => {
                        balances.set(BalancesSnap(
                            balance_rows.iter().map(account_row_snap_of).collect(),
                        ));
                        currencies.set(CurrencyTable::from_currencies(&currency_rows));
                        goals.set(
                            goal_rows
                                .iter()
                                .map(|p| p.goal.account_id.clone())
                                .collect::<std::collections::BTreeSet<String>>(),
                        );
                        // 计划集按引用计数的判据面投影（域行无 Clone，语义不
                        // 变：软删滤除在域读，status/is_deleted 判据在谓词）。
                        plans.set(PlansSnap(
                            plan_rows
                                .iter()
                                .map(|p| assembly::PlanLeadFace {
                                    is_deleted: p.core.is_deleted,
                                    status: p.core.status.clone(),
                                    account_id: p.core.account_id.clone(),
                                    to_account_id: p.to_account_id.clone(),
                                })
                                .collect(),
                        ));
                        privacy.set(amount_privacy_enabled());
                    }
                    Ok(Err(e)) => error.set(Some(e.to_string())),
                    Err(_) => error.set(Some("内部错误：读任务异常退出".to_owned())),
                }
                loading.set(false);
            });
        });
    };

    // 落位：首次装载 + 下钻意图消费（读一次封闸：账户路由无自身定位形态，
    // 意图投递即弃——旧壳 accounts 路由不读 focus 同语义）。
    effect(move || {
        let _landed = nav::store().current.get();
        let _ = nav::take_intent();
        reload();
    });

    // 主题令牌（双态自适应；缺失兜底统一归 crate::theme 单点）。
    let fg = crate::theme::color("foreground");
    let muted = crate::theme::color("muted-foreground");
    let accent = crate::theme::color("primary");
    let danger_fg = crate::theme::color("destructive");

    // 操作反馈条位（NSH 无 toast——#2038 裁决；旧壳 message.success/error
    // 的条位对应物：新动作开始时清回空）。状态语义: None = 无 / 空串 = 进
    // 行中占位 / 正文 = 结果。启用后接各动作（见 perform_* 收编注释）。
    let status = signal(None::<String>);

    // 新建卡数据源（表单选项）：类型闭集 + 币种字典（字典 label 镜像旧壳
    // currencyOptions「{name} ({code})」同形）。空字典（引导未就绪）也照常
    // 呈现空选项，装载错误归加载错误条位（不静默回退缺省币种）。

    // 表单选项（类型闭集 + 币种字典）：label 镜像旧壳 currencyOptions
    // 「{name} ({code})」同形。
    let type_options = assembly::account_type_options;
    let currency_options = move || currencies.get().currency_options();
    // ============================ 行装配与分区 ============================
    let rows = computed(move || {
        let currency_table = currencies.get();
        let goal_ids = goals.get();
        let snap = balances.get();
        let faces: Vec<assembly::AccountRowFace> = snap
            .0
            .iter()
            .map(|row| assembly::AccountRowFace {
                id: &row.id,
                name: &row.name,
                kind: row.kind,
                currency_code: &row.currency_code,
                archived: row.is_archived,
                balance_cents: row.balance_cents,
                credit_limit_cents: row.credit_limit_cents,
                statement_day: row.statement_day,
                due_day: row.due_day,
            })
            .collect();
        assembly::assemble_account_rows(
            &faces,
            &AccountListContext {
                currencies: &currency_table,
                goal_account_ids: &goal_ids,
                privacy_enabled: privacy.get(),
            },
        )
    });
    // 三分区（旧壳 goalBalances / normalBalances / archivedBalances 同口径纯派
    // 生）：同一 rows 快照的种纯派生，见 [`assembly::partition_account_rows]。
    let partitions = computed(move || assembly::partition_account_rows(&rows.get()));
    let goal_rows = computed(move || partitions.get().0);
    let normal_rows = computed(move || partitions.get().1);
    let archived_rows = computed(move || partitions.get().2);
    let archived_summary = computed(move || {
        let currency_table = currencies.get();
        let privacy_enabled = privacy.get();
        let snap = balances.get();
        let faces: Vec<assembly::AccountRowFace> = snap
            .0
            .iter()
            .map(|row| assembly::AccountRowFace {
                id: &row.id,
                name: &row.name,
                kind: row.kind,
                currency_code: &row.currency_code,
                archived: row.is_archived,
                balance_cents: row.balance_cents,
                credit_limit_cents: row.credit_limit_cents,
                statement_day: row.statement_day,
                due_day: row.due_day,
            })
            .collect();
        assembly::assemble_archived_summary(
            &archived_rows.get(),
            &faces,
            &currency_table,
            privacy_enabled,
        )
    });

    // ============================ 新建账户卡 ============================
    let create_name = signal(String::new());
    let create_type = signal(Some("cash".to_owned()));
    let create_currency = signal(Some("CNY".to_owned()));
    let create_initial = signal("0".to_owned());
    let create_limit = signal(String::new());
    let create_statement = signal(String::new());
    let create_due = signal(String::new());
    let create_error = signal(None::<String>);
    let is_credit_create = move || create_type.get().as_deref() == Some("credit");

    let perform_create = move || {
        create_error.set(None);
        status.set(None);
        let name = create_name.get().trim().to_owned();
        if name.is_empty() {
            create_error.set(Some(MSG_NAME_REQUIRED.to_owned()));
            return;
        }
        let Some(type_value) = create_type.get() else {
            create_error.set(Some(MSG_TYPE_REQUIRED.to_owned()));
            return;
        };
        let Ok(kind) = type_value.parse::<ledger_accounts::AccountType>() else {
            create_error.set(Some(format!("未知账户类型: {type_value}")));
            return;
        };
        let Some(currency_code) = create_currency.get() else {
            create_error.set(Some(MSG_CURRENCY_REQUIRED.to_owned()));
            return;
        };
        let is_credit = kind == ledger_accounts::AccountType::Credit;
        let (credit_limit_cents, statement_day, due_day) = if is_credit {
            (
                assembly::budget_yuan_to_cents(&create_limit.get()),
                day_text_to_i64(&create_statement.get()),
                day_text_to_i64(&create_due.get()),
            )
        } else {
            // 非信用卡不携带档案三键（缺省 = 未设置，旧壳不携带同口径）。
            (None, None, None)
        };
        let input = ledger_accounts::AccountInput {
            name,
            kind,
            currency_code,
            initial_balance_cents: Some(
                assembly::budget_yuan_to_cents(&create_initial.get()).unwrap_or(0),
            ),
            credit_limit_cents,
            statement_day,
            due_day,
        };
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                // 会话缺失码化兑底（写路径在会话就位前执行的不可达兑底，
                // #2033 预算写路径同形）。
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::create_account(db, input),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(_new_id)) => {
                        // 重置创建表单（旧壳 create 成功同动作）。
                        create_name.set(String::new());
                        create_initial.set("0".to_owned());
                        create_limit.set(String::new());
                        create_statement.set(String::new());
                        create_due.set(String::new());
                        status.set(Some(MSG_CREATED.to_owned()));
                        reload();
                    }
                    Ok(Err(e)) => create_error.set(Some(format!("创建失败: {e}"))),
                    Err(_) => create_error.set(Some("内部错误：写入任务异常退出".to_owned())),
                }
            });
        });
    };

    // ============================ 编辑弹窗态 ============================
    // 目标按账户 id 持有（弹窗内容自 balances 快照按 id 反查原始行；信号
    // 载荷等值语义以 id 判，不要求域结构比较）。
    let edit_target: Signal<Option<String>> = signal(None);
    let edit_name = signal(String::new());
    let edit_currency = signal(None::<String>);
    let edit_limit = signal(String::new());
    let edit_statement = signal(String::new());
    let edit_due = signal(String::new());
    let edit_error = signal(None::<String>);

    // 行数据消费面：编辑 / 调整目标行反查（快照内单点）；行缺席（并发删除）
    // 直接弹窗关闭，不呈现半态。快照容器已可克隆，反查按 id 找同一行克隆。
    let snap_of = move |account_id: &str| {
        balances
            .get_untracked()
            .0
            .iter()
            .find(|row| row.id == account_id)
            .cloned()
    };
    let raw_row_of = move |account_id: Option<String>| account_id.and_then(|id| snap_of(&id));

    let open_edit = move |row: AccountRowSnap| {
        edit_name.set(row.name.clone());
        edit_currency.set(Some(row.currency_code.clone()));
        // 档案字段回填：额度按币种小数位从分转元展示，日子为声明值原样。
        let dp = currencies
            .get_untracked()
            .format_of(&row.currency_code)
            .decimal_places;
        edit_limit.set(
            row.credit_limit_cents
                .map(|cents| assembly::cents_to_amount_text(cents, dp))
                .unwrap_or_default(),
        );
        edit_statement.set(row.statement_day.map(|d| d.to_string()).unwrap_or_default());
        edit_due.set(row.due_day.map(|d| d.to_string()).unwrap_or_default());
        edit_error.set(None);
        edit_target.set(Some(row.id.clone()));
    };

    let is_goal_bound_edit = move || {
        raw_row_of(edit_target.get_untracked())
            .map(|row| goals.get_untracked().contains(&row.id))
            .unwrap_or(false)
    };
    let is_credit_edit = move || {
        raw_row_of(edit_target.get_untracked())
            .map(|row| row.kind == ledger_accounts::AccountType::Credit)
            .unwrap_or(false)
    };

    let close_edit = move || edit_target.set(None);

    let perform_edit = move || {
        edit_error.set(None);
        status.set(None);
        let Some(target) = raw_row_of(edit_target.get_untracked()) else {
            close_edit();
            return;
        };
        let Some(currency_code) = edit_currency.get_untracked() else {
            edit_error.set(Some(MSG_CURRENCY_REQUIRED.to_owned()));
            return;
        };
        let name = edit_name.get_untracked().trim().to_owned();
        if name.is_empty() {
            edit_error.set(Some(MSG_NAME_REQUIRED.to_owned()));
            return;
        }
        // 目标绑定账户：不发 name 键（账户名随目标联动，账户侧任何路径都不
        // 回写名称，issue #1752）。
        let goal_bound = goals.get_untracked().contains(&target.id);
        let is_credit = target.kind == ledger_accounts::AccountType::Credit;
        let input = ledger_accounts::AccountUpdateInput {
            name: (!goal_bound).then_some(name),
            currency_code: Some(currency_code),
            credit_limit_cents: if is_credit {
                // 三态：给值 = 落定、空/非法 = 清空（null 决策面归旧壳
                // NInputNumber 的对应形态）。
                Some(assembly::budget_yuan_to_cents(&edit_limit.get_untracked()))
            } else {
                // 非信用卡根本不发三键（不发 = 不改）。
                None
            },
            statement_day: is_credit.then(|| day_text_to_i64(&edit_statement.get_untracked())),
            due_day: is_credit.then(|| day_text_to_i64(&edit_due.get_untracked())),
            is_archived: None,
        };
        let id = target.id.clone();
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::update_account(db, &id, input),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        close_edit();
                        status.set(Some(MSG_SAVED.to_owned()));
                        reload();
                    }
                    Ok(Err(e)) => edit_error.set(Some(format!("保存失败: {e}"))),
                    Err(_) => edit_error.set(Some("内部错误：写入任务异常退出".to_owned())),
                }
            });
        });
    };

    // ============================ 调整余额弹窗态 ============================
    let adjust_target: Signal<Option<String>> = signal(None);
    let adjust_target_text = signal(String::new());
    let adjust_date = signal(chrono::Local::now().date_naive());
    let adjust_error = signal(None::<String>);

    let open_adjust = move |row: AccountRowSnap| {
        adjust_target_text.set(String::new());
        adjust_date.set(chrono::Local::now().date_naive());
        adjust_error.set(None);
        adjust_target.set(Some(row.id.clone()));
    };
    let close_adjust = move || adjust_target.set(None);

    // 差额 Δ = 目标 − 当前（>0 从黑洞转入，<0 转出至黑洞，=0 无需调整）。
    // 目标非法（未填写 / 超界）= None（无差额，确认钮禁用）。
    let adjust_delta = move |target: Option<&AccountRowSnap>| {
        // 差额 Δ = 目标 − 当前（金额换算接缝非法输入 → None：无差额面）。
        let target_cents = assembly::budget_yuan_to_cents(&adjust_target_text.get())?;
        target.map(|row| target_cents - row.balance_cents)
    };

    let perform_adjust = move || {
        adjust_error.set(None);
        status.set(None);
        let Some(target) = raw_row_of(adjust_target.get_untracked()) else {
            close_adjust();
            return;
        };
        let Some(delta) = adjust_delta(Some(&target)) else {
            return;
        };
        if delta == 0 {
            return;
        }
        let input = ledger_accounts::AccountBalanceAdjustInput {
            target_balance_cents: target.balance_cents + delta,
            date: adjust_date.get().to_string(),
            note: None,
        };
        let id = target.id.clone();
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::adjust_account_balance(db, &id, &input),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok((_tx_id, _created))) => {
                        close_adjust();
                        // 余额调整生成了转账交易：广播交易失效信号（交易
                        // 列表重读；广播接线删除即红钉在 tests/app_headless.rs）。
                        signals::notify_transactions_invalidated();
                        status.set(Some(MSG_ADJUSTED.to_owned()));
                        reload();
                    }
                    Ok(Err(e)) => adjust_error.set(Some(format!("调整失败: {e}"))),
                    Err(_) => adjust_error.set(Some("内部错误：写入任务异常退出".to_owned())),
                }
            });
        });
    };

    // ============================ 归档 / 恢复与删除 ============================
    // 归档 / 恢复（spec #1952 / ADR-0142）：收纳态不改任何金额口径，幂等；
    // 确认框把真话说清楚（#1955）——余额非零显示当前余额并声明「仍计入净资产
    // 与余额合计」，被未结束计划引用时提示计划数量，两者都不阻断；删除走
    // error 级确认（软删不可恢复的话在对话框，ADR-0078 分级判据）；取消 / ESC
    // 不写。目标域守卫在写入口先行（boot 同位），错误落反馈条位。
    let delete_target: Signal<Option<String>> = signal(None);
    let archive_target: Signal<Option<String>> = signal(None);

    let perform_set_archived = move |row: AccountRowSnap, archived: bool| {
        status.set(None);
        let id = row.id.clone();
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => {
                        if archived {
                            boot::archive_account(db, &id)
                        } else {
                            boot::unarchive_account(db, &id)
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
                            if archived { MSG_ARCHIVED } else { MSG_RESTORED }.to_owned(),
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

    let perform_delete = move |row: AccountRowSnap| {
        status.set(None);
        let id = row.id.clone();
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::delete_account(db, &id),
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
    let close_archive = move || archive_target.set(None);

    // 归档确认的未结束计划数（plans 快照在装载批读取，会话内同数据；归档
    // 确认框正文只陈述数量与「不停用」；旧壳确认按下时异步读一次，本壳
    // 读失败（空集）零阻断同口径）。
    let plan_count_for = move |account_id: &str| {
        let snapshot = plans.get_untracked().0;
        assembly::count_unfinished_plans(&snapshot, account_id) as i64
    };

    let open_row_menu = move |partition: Memo<Vec<AccountRowCells>>,
                              row_index: usize,
                              at: Point<Points>| {
        let snapshot = partition.get_untracked();
        let Some(assembled) = snapshot.get(row_index) else {
            return;
        };
        let account_id = assembled.id.clone();
        let Some(raw) = snap_of(&account_id) else {
            return;
        };
        let mut items = Vec::new();
        if raw.is_archived {
            // 归档行的动作集收窄为恢复（ADR-0142 决策 2：编辑/调整/删除
            // 都要先恢复，避免在一张看不见全貌的卡上改钱）。
            items.push(
                MenuItem::new("恢复").on_select(move || perform_set_archived(raw.clone(), false)),
            );
        } else {
            items.push(MenuItem::new("编辑").on_select({
                let raw = raw.clone();
                move || open_edit(raw.clone())
            }));
            items.push(MenuItem::new("调整余额").on_select({
                let raw = raw.clone();
                move || open_adjust(raw.clone())
            }));
            items.push(MenuItem::new("归档").on_select({
                let archive_id = raw.id.clone();
                move || archive_target.set(Some(archive_id.clone()))
            }));
            items.push(MenuItem::new("删除").on_select({
                let delete_id = raw.id.clone();
                move || delete_target.set(Some(delete_id.clone()))
            }));
        }
        context_menu::open_context_menu(items, at);
    };

    // 账户名下钻（旧壳 AccountLink / issue #96/#97）：跳交易页并按涉及账户
    // 过滤（交易筛选下钻载荷全量替换，消费导航单点接缝，消费不自建）。
    let drill_transactions = move |account_id: String| {
        nav::open(NavRequest {
            target: ViewName::Transactions,
            container_tab: None,
            form_tab: None,
            focus: None,
            tx_filters: Some(tx_filter::DrillFilters {
                account: Some(account_id),
                ..tx_filter::DrillFilters::default()
            }),
            investments_drill: None,
        });
    };

    // 列定义映射（装配列 → 宿主列；富余宽全给名称列）。
    let columns = computed(move || {
        let width = adaptive::content_width();
        account_columns(width)
    });

    // 已归档分区折叠展开（默认折叠且不持久化——折叠是查看口味，不入设备
    // 偏好，ADR-0142 决策 1 同口径）。
    let archived_expanded = signal(false);

    // 分区表数据 → 宿主表格数据（按列装配，格文本已是装配产物）。
    let data_of = move |partition: Memo<Vec<AccountRowCells>>| {
        move || {
            let snapshot = partition.get();
            DataTableRows::new(snapshot.len())
                .texts("name", snapshot.iter().map(|r| r.name.as_str()))
                .texts("type", snapshot.iter().map(|r| r.type_label.as_str()))
                .texts(
                    "currency",
                    snapshot.iter().map(|r| r.currency_code.as_str()),
                )
                .texts("balance", snapshot.iter().map(|r| r.balance.as_str()))
                .texts("actions", snapshot.iter().map(|_| ""))
        }
    };

    // 分区表自绘格：名称列 = 可点击账户名（主题强调色，下钻交易页按账户
    // 过滤）；余额列 = 金额（+ 信用卡使用率小字行，警示阈值染色归装配）；
    // 操作列 = 「⋯」（右键同位）。渲染期零格式化（格文本已装配）。
    let cell_of = move |partition: Memo<Vec<AccountRowCells>>| {
        move |cell: &nsh_base::DataTableCell| -> nsh_core::element::AnyElement {
            let index = cell.row as usize;
            let assembled = partition.get_untracked().get(index).cloned();
            match cell.key.as_ref() {
                "name" => {
                    let id = assembled.map(|r| r.id.clone()).unwrap_or_default();
                    let label = cell.text.clone();
                    view()
                        .test_id(format!("acct-name-{id}").as_str())
                        .on_press({
                            let id = id.clone();
                            move |_: &PressEvent| {
                                if !id.is_empty() {
                                    drill_transactions(id.clone());
                                }
                            }
                        })
                        .child(
                            text(label)
                                .text_size(px(font_size::SECONDARY))
                                .text_color(accent)
                                .number_of_lines(1),
                        )
                        .into_element()
                }
                "balance" => {
                    let amount = text(cell.text.clone())
                        .text_size(px(font_size::SECONDARY))
                        .text_color(fg)
                        .number_of_lines(1);
                    match assembled.as_ref().and_then(|r| r.credit_percent) {
                        Some(percent) => {
                            let warning = assembled
                                .as_ref()
                                .map(|r| r.credit_warning)
                                .unwrap_or(false);
                            view()
                                .flex_col()
                                .gap_1()
                                .child(amount)
                                .child(
                                    text(format!("已用 {percent}%"))
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(if warning {
                                            amount_color(assembly::AmountSemantic::Expense)
                                        } else {
                                            muted
                                        })
                                        .test_id("acct-credit-percent"),
                                )
                                .into_element()
                        }
                        None => amount.into_element(),
                    }
                }
                "actions" => {
                    let Some(row) = assembled else {
                        return view().into_element();
                    };
                    let id = row.id.clone();
                    view()
                        .test_id(format!("acct-more-{id}").as_str())
                        .on_press({
                            move |e: &PressEvent| {
                                open_row_menu(partition, index, e.page_position);
                            }
                        })
                        .child(text("⋯").text_size(px(font_size::SECONDARY)).text_color(fg))
                        .into_element()
                }
                _ => text(cell.text.clone()).number_of_lines(1).into_element(),
            }
        }
    };

    // 分区表构造（三分区同构：同列集同渲染器，数据各分区行集）。
    let table_of = move |partition: Memo<Vec<AccountRowCells>>, table_id: &'static str| {
        view().test_id(table_id).flex_1().child(
            data_table()
                .columns(columns)
                .data(data_of(partition))
                .render_cell(cell_of(partition))
                // 行右键菜单（「⋯」同 options）：目标行由右键所在行固定
                // （fixed-target，旧壳同位）。
                .on_context_menu({
                    move |e: &DataTableContextMenu| {
                        let Some(row_index) = e.row else {
                            return;
                        };
                        open_row_menu(partition, row_index as usize, Point::new(e.x, e.y));
                    }
                })
                .loading(move || loading.get())
                .loading_view(text("正在加载…").text_color(muted))
                // 无 .empty 槽：行集已在数据门控（折叠零行、无归档账户整卡缺席），
                // 空态文案归 outcome 面（折叠不呈现「暂无数据」——旧壳 v-if 同果）。
                .classes(table_classes())
                .flex_1(),
        )
    };

    // 并区卡片体（标题 + 数据表）：卡片自身 flex_1——分区是根内容列的弹性
    // 子项，撑出确定高给内嵌的虚拟化表格（表格组件在弹性链内取自身高度，
    // 卡片不定高则虚拟化量出零高、行不呈现——#2041 面重排即发现的先例）。
    let section_of = move |title: &'static str,
                           partition: Memo<Vec<AccountRowCells>>,
                           table_id: &'static str| {
        view()
            .flex_1()
            .flex_col()
            .gap_2()
            .app_class("rounded-(--radius-card) border border-border p-3")
            .child(
                view().flex_1().child(
                    text(title)
                        .text_size(px(font_size::SECONDARY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg),
                ),
            )
            .child(table_of(partition, table_id))
    };

    // ============================ 弹窗内容（视图树内联函数式插槽） ============================
    // 槽内容在分支内现建（组件同体；字段与只读摘要随分支重建——信号在同
    // 组件作用域，与旧壳 useModalIntent + 表单 key 重建的对应形态）。
    let edit_modal_content = move || {
        let is_goal = is_goal_bound_edit();
        let is_credit = is_credit_edit();
        let type_text = if is_goal {
            "攒钱".to_owned()
        } else {
            raw_row_of(edit_target.get_untracked())
                .map(|row| assembly::account_type_label(row.kind).to_owned())
                .unwrap_or_default()
        };
        // 信用卡只读摘要：取已保存的行值（不随上方未提交的输入实时变化——
        // 摘要回答「这张卡现在用了多少额度」，旧壳 editCredit 同语义）。
        let credit = raw_row_of(edit_target.get_untracked()).and_then(|row| {
            assembly::credit_profile_fields(
                row.kind,
                row.credit_limit_cents,
                row.statement_day,
                row.due_day,
                row.balance_cents,
                today_local(),
            )
        });
        let fmt = raw_row_of(edit_target.get_untracked())
            .map(|row| currencies.get_untracked().format_of(&row.currency_code))
            .unwrap_or_default();
        let usage_text = match &credit {
            Some(profile) => {
                if profile.available_cents.is_none() {
                    "尚未填写信用额度".to_owned()
                } else {
                    format!(
                        "已用 {} · 可用 {} · 使用率 {}%",
                        assembly::format_amount_basic(profile.used_cents, &fmt),
                        profile
                            .available_cents
                            .map(|a| assembly::format_amount_basic(a, &fmt))
                            .unwrap_or_default(),
                        profile.utilization_percent.unwrap_or_default(),
                    )
                }
            }
            None => String::new(),
        };
        let next_nodes_text = match &credit {
            Some(profile) => match (&profile.next_statement_date, &profile.next_due_date) {
                (Some(statement), Some(due)) => {
                    format!("账单日 {statement} · 还款日 {due}")
                }
                _ => "尚未填写账单日与还款日".to_owned(),
            },
            None => String::new(),
        };
        // 信用卡档案字段与只读摘要（仅信用卡出现；摘要不随输入实时变化）。
        let credit_section = when(
            is_credit,
            move || {
                view()
                    .flex_col()
                    .gap_2()
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("信用额度", muted))
                            .child(
                                input()
                                    .test_id("acct-edit-limit")
                                    .value(move || edit_limit.get())
                                    .placeholder("如 50000")
                                    .optional_label("信用额度")
                                    .text_color(fg)
                                    .w(px(140.0))
                                    .app_class(
                                        "bg-background rounded-(--radius) border border-border px-2 py-1                                          placeholder:text-muted-foreground",
                                    )
                                    .on_change(move |text: &str| {
                                        edit_limit.set(text.to_owned())
                                    }),
                            ),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("账单日", muted))
                            .child(
                                input()
                                    .test_id("acct-edit-statement")
                                    .value(move || edit_statement.get())
                                    .placeholder("1–31")
                                    .optional_label("账单日")
                                    .text_color(fg)
                                    .w(px(100.0))
                                    .app_class(
                                        "bg-background rounded-(--radius) border border-border px-2 py-1                                          placeholder:text-muted-foreground",
                                    )
                                    .on_change(move |text: &str| {
                                        edit_statement.set(text.to_owned())
                                    }),
                            ),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(field_label("还款日", muted))
                            .child(
                                input()
                                    .test_id("acct-edit-due")
                                    .value(move || edit_due.get())
                                    .placeholder("1–31")
                                    .optional_label("还款日")
                                    .text_color(fg)
                                    .w(px(100.0))
                                    .app_class(
                                        "bg-background rounded-(--radius) border border-border px-2 py-1                                          placeholder:text-muted-foreground",
                                    )
                                    .on_change(
                                        move |text: &str| edit_due.set(text.to_owned()),
                                    ),
                            ),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_start()
                            .gap_2()
                            .child(field_label("额度使用", muted))
                            .child(
                                text(usage_text.clone())
                                    .text_size(px(font_size::CAPTION))
                                    .text_color(muted)
                                    .test_id("acct-edit-usage"),
                            ),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_start()
                            .gap_2()
                            .child(field_label("下次账单节点", muted))
                            .child(
                                text(next_nodes_text.clone())
                                    .text_size(px(font_size::CAPTION))
                                    .text_color(muted)
                                    .test_id("acct-edit-next"),
                            ),
                    )
            },
        )
        .otherwise(view);
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
                            .test_id("acct-edit-name")
                            .value(move || edit_name.get())
                            .placeholder("账户名称")
                            .optional_label("账户名称")
                            .disabled(is_goal)
                            .text_color(fg)
                            .w(px(240.0))
                            .app_class(
                                "bg-background rounded-(--radius) border border-border px-2 py-1 \
                                 placeholder:text-muted-foreground",
                            )
                            .on_change(move |text: &str| edit_name.set(text.to_owned())),
                    ),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label("类型", muted))
                    .child(
                        text(type_text)
                            .text_size(px(font_size::SECONDARY))
                            .text_color(muted)
                            .test_id("acct-edit-type"),
                    ),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label("币种", muted))
                    .child(select::app_select(
                        "acct-edit-currency",
                        currency_options,
                        edit_currency,
                        "选择币种",
                        false,
                    )),
            )
            .child(credit_section)
            .child(
                // 保存错误条位（域码化错误 zh 原样；下次尝试开始时清空）。
                when(
                    move || edit_error.get().is_some(),
                    move || {
                        text(move || {
                            format!("保存失败：{}", edit_error.get().unwrap_or_default())
                        })
                        .text_size(px(font_size::CAPTION))
                        .text_color(danger_fg)
                        .test_id("acct-edit-error")
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
                                    .test_id("acct-edit-cancel"),
                            )
                            .on_press(move |_: &()| close_edit()),
                    )
                    .child(
                        button()
                            .app_class("px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90")
                            .child(
                                text("保存")
                                    .text_size(px(font_size::SECONDARY))
                                    .text_color(crate::theme::color("primary-foreground"))
                                    .test_id("acct-edit-save"),
                            )
                            .on_press(move |_: &()| perform_edit()),
                    ),
            )
    };
    let adjust_modal_content = move || {
        let current_text = raw_row_of(adjust_target.get_untracked())
            .map(|row| {
                let fmt = currencies.get_untracked().format_of(&row.currency_code);
                assembly::format_amount_basic(row.balance_cents, &fmt)
            })
            .unwrap_or_default();
        // 差额与确认置灰是响应式节点（捕获信号的闭包随输入逐字量化），
        // 不随弹窗内容重建——弹窗分支只走一次，静态文本会陈旧在首次快照。
        view()
            .flex_col()
            .gap_2()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label("当前余额", muted))
                    .child(
                        text(current_text)
                            .text_size(px(font_size::SECONDARY))
                            .text_color(fg)
                            .test_id("acct-adjust-current"),
                    ),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label("目标余额", muted))
                    .child(
                        input()
                            .test_id("acct-adjust-target")
                            .value(move || adjust_target_text.get())
                            .placeholder("对账后应有的余额")
                            .optional_label("目标余额")
                            .text_color(fg)
                            .w(px(240.0))
                            .app_class("bg-background rounded-(--radius) border border-border px-2 py-1 placeholder:text-muted-foreground")
                            .on_change(move |text: &str| {
                                adjust_target_text.set(text.to_owned())
                            }),
                    ),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label("调整日期", muted))
                    .child(date_select::date_select("acct-adjust-date", adjust_date)),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label("差额", muted))
                    .child(
                        // 差额实时呈现（旧壳 adjustDelta computed 同口径）。
                        text(move || {
                            let delta = adjust_delta(
                                raw_row_of(adjust_target.get_untracked()).as_ref(),
                            );
                            match delta {
                                Some(delta) if delta != 0 => {
                                    let target = raw_row_of(adjust_target.get_untracked());
                                    let fmt = target
                                        .map(|row| {
                                            currencies
                                                .get_untracked()
                                                .format_of(&row.currency_code)
                                        })
                                        .unwrap_or_default();
                                    let abs = assembly::format_amount_basic(delta.abs(), &fmt);
                                    let direction = if delta > 0 {
                                        format!("将从「无」转入 {abs}")
                                    } else {
                                        format!("将转出 {abs} 至「无」")
                                    };
                                    format!(
                                        "{direction}（生成一笔与「无」的转账，删除该转账即撤销调整）"
                                    )
                                }
                                Some(_) => "余额已等于目标值，无需调整".to_owned(),
                                None => "对账后应有的余额未填写".to_owned(),
                            }
                        })
                            .text_size(px(font_size::CAPTION))
                            .text_color(move || {
                                let positive = adjust_delta(
                                    raw_row_of(adjust_target.get_untracked()).as_ref(),
                                )
                                .map(|d| d > 0)
                                .unwrap_or(false);
                                if positive {
                                    amount_color(assembly::AmountSemantic::Income)
                                } else {
                                    muted
                                }
                            })
                            .test_id("acct-adjust-delta"),
                    ),
            )
            .child(
                // 调整错误条位（域码化错误 zh 原样；下次尝试开始时清空）。
                when(
                    move || adjust_error.get().is_some(),
                    move || {
                        text(move || {
                            format!("调整失败：{}", adjust_error.get().unwrap_or_default())
                        })
                        .text_size(px(font_size::CAPTION))
                        .text_color(danger_fg)
                        .test_id("acct-adjust-error")
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
                                    .test_id("acct-adjust-cancel"),
                            )
                            .on_press(move |_: &()| close_adjust()),
                    )
                    .child(
                        button()
                            .app_class("px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90")
                            .disabled(move || {
                                let delta = adjust_delta(
                                    raw_row_of(adjust_target.get_untracked()).as_ref(),
                                );
                                delta.is_none() || delta == Some(0)
                            })
                            .child(
                                text("确认调整")
                                    .text_size(px(font_size::SECONDARY))
                                    .text_color(crate::theme::color("primary-foreground"))
                                    .test_id("acct-adjust-confirm"),
                            )
                            .on_press(move |_: &()| perform_adjust()),
                    ),
            )
    };

    // ============================ 视图树 ============================
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
                .child(field_label("名称", muted))
                .child(
                    input()
                        .test_id("acct-create-name")
                        .value(move || create_name.get())
                        .placeholder("账户名称")
                        .optional_label("账户名称")
                        .text_color(fg)
                        .w(px(160.0))
                        .app_class("bg-background rounded-(--radius) border border-border px-2 py-1 placeholder:text-muted-foreground")
                        .on_change(move |text: &str| create_name.set(text.to_owned())),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(field_label("类型", muted))
                .child(select::app_select(
                    "acct-type",
                    type_options,
                    create_type,
                    "选择类型",
                    false,
                )),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(field_label("币种", muted))
                .child(select::app_select(
                    "acct-currency",
                    currency_options,
                    create_currency,
                    "选择币种",
                    false,
                )),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(field_label("初始余额", muted))
                .child(
                    input()
                        .test_id("acct-create-initial")
                        .value(move || create_initial.get())
                        .placeholder(move || {
                            if is_credit_create() {
                                "已有欠款请填负数"
                            } else {
                                "0.00"
                            }
                        })
                        .optional_label("初始余额")
                        .text_color(fg)
                        .w(px(140.0))
                        .app_class("bg-background rounded-(--radius) border border-border px-2 py-1 placeholder:text-muted-foreground")
                        .on_change(move |text: &str| create_initial.set(text.to_owned())),
                ),
        )
        .child(
            // 信用卡档案三字段（仅类型选信用卡时出现，spec #1327）。
            when(
                is_credit_create,
                move || {
                    view()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .flex_wrap()
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(field_label("信用额度", muted))
                                .child(
                                    input()
                                        .test_id("acct-create-limit")
                                        .value(move || create_limit.get())
                                        .placeholder("如 50000")
                                        .optional_label("信用额度")
                                        .text_color(fg)
                                        .w(px(140.0))
                                        .app_class("bg-background rounded-(--radius) border border-border px-2 py-1 placeholder:text-muted-foreground")
                                        .on_change(move |text: &str| {
                                            create_limit.set(text.to_owned())
                                        }),
                                ),
                        )
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(field_label("账单日", muted))
                                .child(
                                    input()
                                        .test_id("acct-create-statement")
                                        .value(move || create_statement.get())
                                        .placeholder("1–31")
                                        .optional_label("账单日")
                                        .text_color(fg)
                                        .w(px(100.0))
                                        .app_class("bg-background rounded-(--radius) border border-border px-2 py-1 placeholder:text-muted-foreground")
                                        .on_change(move |text: &str| {
                                            create_statement.set(text.to_owned())
                                        }),
                                ),
                        )
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(field_label("还款日", muted))
                                .child(
                                    input()
                                        .test_id("acct-create-due")
                                        .value(move || create_due.get())
                                        .placeholder("1–31")
                                        .optional_label("还款日")
                                        .text_color(fg)
                                        .w(px(100.0))
                                        .app_class("bg-background rounded-(--radius) border border-border px-2 py-1 placeholder:text-muted-foreground")
                                        .on_change(move |text: &str| {
                                            create_due.set(text.to_owned())
                                        }),
                                ),
                        )
                },
            )
            .otherwise(view),
        )
        .child(
            button()
                .app_class("px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90")
                .child(
                    text("添加")
                        .text_size(px(font_size::SECONDARY))
                        .text_color(crate::theme::color("primary-foreground"))
                        .test_id("acct-add"),
                )
                .on_press(move |_: &()| perform_create()),
        );

    let create_card = view()
        .flex_col()
        .gap_2()
        .app_class("rounded-(--radius-card) border border-border p-3")
        .child(
            text(CREATE_CARD_TITLE)
                .text_size(px(font_size::SECONDARY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg),
        )
        .child(create_form)
        .child(
            // 创建错误/提示条位（前置拦截与域错误同位；下次尝试开始时清空）。
            when(
                move || create_error.get().is_some(),
                move || {
                    text(move || create_error.get().unwrap_or_default())
                        .text_size(px(font_size::CAPTION))
                        .text_color(danger_fg)
                        .test_id("acct-create-error")
                },
            )
            .otherwise(view),
        );

    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(
            view().px_4().py_2().child(
                text(ViewName::Accounts.label())
                    .text_size(px(font_size::TITLE))
                    .text_color(fg)
                    .test_id("view-title"),
            ),
        )
        .child(
            // 装载失败条位（码化错误 zh-CN 原样，不静默——分组口径缺失不静默
            // 回退，债务环 ADR-0067 消费纪律）。
            when(
                move || error.get().is_some(),
                move || {
                    text(move || format!("加载失败：{}", error.get().unwrap_or_default()))
                        .text_size(px(font_size::CAPTION))
                        .text_color(danger_fg)
                        .test_id("acct-error")
                        .app_class("px-4 py-1")
                },
            )
            .otherwise(view),
        )
        .child(
            // 内容区（预算视图同形态的字段卡片列；表格虚拟化按自身高度裁行）
            view()
                .flex_1()
                .flex_col()
                .gap_3()
                .p_4()
                .child(create_card)
                .child(
                    when(
                        move || !goal_rows.get().is_empty(),
                        move || section_of(GOAL_CARD_TITLE, goal_rows, "acct-goal-table"),
                    )
                    .otherwise(view),
                )
                .child(
                    when(
                        move || !normal_rows.get().is_empty(),
                        move || section_of(LIST_CARD_TITLE, normal_rows, "acct-table"),
                    )
                    .otherwise(view),
                )
                .child(
                    // 已归档分区（页底、默认折叠、标题行带数量与余额合计；
                    // 展开后才渲染表格——折叠即不挂载行）。
                    when(
                        move || !archived_rows.get().is_empty(),
                        move || {
                            view()
                                .flex_1()
                                .flex_col()
                                .gap_2()
                                .app_class("rounded-(--radius-card) border border-border p-3")
                                .child(
                                    view().flex_row().items_center().gap_2().child(
                                        button()
                                            .accessibility_label(if archived_expanded.get() {
                                                "收起已归档账户"
                                            } else {
                                                "展开已归档账户"
                                            })
                                            .app_class(
                                                "px-2 py-1 rounded-(--radius) text-foreground hover:bg-accent",
                                            )
                                            .on_press(move |_: &()| {
                                                archived_expanded.update(|v| *v = !*v);
                                            })
                                            .child(
                                                // 折叠箭头随开合翻转（响应式节点：折叠
                                                // 是信号态，静态 format 只呈现首帧；
                                                // test_id 锚在文本上，headless 物理命
                                                // 中到内层按钮——tx-filter-clear 先例）。
                                                text(move || {
                                                    format!(
                                                        "{} {}",
                                                        if archived_expanded.get() {
                                                            "▾"
                                                        } else {
                                                            "▸"
                                                        },
                                                        ARCHIVED_TITLE
                                                    )
                                                })
                                                .text_size(px(font_size::SECONDARY))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(fg)
                                                .test_id("acct-archived-toggle"),
                                            )
                                            .child(
                                                text(move || {
                                                    format!(" {}", archived_summary.get())
                                                })
                                                .text_size(px(font_size::CAPTION))
                                                .text_color(muted)
                                                .test_id("acct-archived-summary"),
                                            ),
                                    ),
                                )
                                .child(
                                    // 归档表：容器 flex_1 常在给虚拟化定高；行集由
                                    // 展开态门控（折叠零行、无 .empty 槽——空状态归
                                    // 旧壳 v-if 不挂载行的同果；展开才挂行）。
                                    view()
                                        .test_id("acct-archived-table")
                                        .flex_1()
                                        .child({
                                            let partition = archived_rows;
                                            data_table()
                                                .columns(columns)
                                                .data(move || {
                                                    if !archived_expanded.get() {
                                                        return DataTableRows::new(0);
                                                    }
                                                    let snapshot = partition.get_untracked();
                                                    DataTableRows::new(snapshot.len())
                                                        .texts(
                                                            "name",
                                                            snapshot.iter().map(|r| r.name.as_str()),
                                                        )
                                                        .texts(
                                                            "type",
                                                            snapshot
                                                                .iter()
                                                                .map(|r| r.type_label.as_str()),
                                                        )
                                                        .texts(
                                                            "currency",
                                                            snapshot
                                                                .iter()
                                                                .map(|r| r.currency_code.as_str()),
                                                        )
                                                        .texts(
                                                            "balance",
                                                            snapshot.iter().map(|r| r.balance.as_str()),
                                                        )
                                                        .texts("actions", snapshot.iter().map(|_| ""))
                                                })
                                                .render_cell(cell_of(archived_rows))
                                                .on_context_menu({
                                                    let partition = archived_rows;
                                                    move |e: &DataTableContextMenu| {
                                                        let Some(row_index) = e.row else {
                                                            return;
                                                        };
                                                        open_row_menu(
                                                            partition,
                                                            row_index as usize,
                                                            Point::new(e.x, e.y),
                                                        );
                                                    }
                                                })
                                                .loading(move || loading.get())
                                                .loading_view(
                                                    text("正在加载…").text_color(muted),
                                                )
                                                .classes(table_classes())
                                                .flex_1()
                                        })
                                )
                        },
                    )
                    .otherwise(view),
                )
                .child(
                    // 操作反馈条位（成功/失败共用一条，新动作开始清回空）。
                    when(
                        move || status.get().is_some(),
                        move || {
                            text(move || status.get().unwrap_or_default())
                                .text_size(px(font_size::CAPTION))
                                .text_color(fg)
                                .test_id("acct-status")
                        },
                    )
                    .otherwise(view),
                )
                .child(
                    // 编辑账户弹窗（Modal 封装：标题 + 内容槽；显示由目标
                    // 非空派生；内容随分支重建，字段值归各自信号）。
                    modal::app_modal(
                        move || edit_target.get().is_some(),
                        || "编辑账户".to_owned(),
                        close_edit,
                        edit_modal_content,
                    ),
                )
                .child(
                    // 调整余额弹窗（日期默认今天可改，对账常补记过去日期）。
                    modal::app_modal(
                        move || adjust_target.get().is_some(),
                        || "调整余额".to_owned(),
                        close_adjust,
                        adjust_modal_content,
                    ),
                )
                .child(
                    // 删除确认（error 级：软删不可恢复「相关交易仍保留」明示）。
                    when(
                        move || delete_target.get().is_some(),
                        move || {
                            danger::danger_confirm(
                                move || delete_target.get().is_some(),
                                danger::DangerConfirmSpec {
                                    level: danger::DangerLevel::Error,
                                    title: "删除账户".to_owned(),
                                    lead: Some(format!(
                                        "确认删除账户「{}？」",
                                        raw_row_of(delete_target.get_untracked())
                                            .map(|row| row.name.clone())
                                            .unwrap_or_default()
                                    )),
                                    strong_warning: Some("删除后相关交易仍保留。".to_owned()),
                                    detail: None,
                                    confirm_label: "删除".to_owned(),
                                    cancel_label: "取消".to_owned(),
                                },
                                move |outcome: DangerOutcome| match outcome {
                                    DangerOutcome::Confirmed => {
                                        if let Some(row) =
                                            raw_row_of(delete_target.get_untracked())
                                        {
                                            perform_delete(row);
                                        }
                                        close_delete();
                                    }
                                    DangerOutcome::Dismissed => close_delete(),
                                },
                            )
                        },
                    )
                    .otherwise(view),
                )
                .child(
                    // 归档确认（warning 级：不阻断——余额 / 计划句子把真说清楚，
                    // 确认按钮不置灰）。
                    when(
                        move || archive_target.get().is_some(),
                        move || {
                            danger::danger_confirm(
                                move || archive_target.get().is_some(),
                                archive_spec_for(&archive_target.get_untracked().clone().unwrap_or_default(), plan_count_for, balances, currencies, privacy),
                                move |outcome: DangerOutcome| match outcome {
                                    DangerOutcome::Confirmed => {
                                        if let Some(row) =
                                            raw_row_of(archive_target.get_untracked())
                                        {
                                            perform_set_archived(row, true);
                                        }
                                        close_archive();
                                    }
                                    DangerOutcome::Dismissed => close_archive(),
                                },
                            )
                        },
                    )
                    .otherwise(view),
                ),
        )
}
/// 「今天」（本地时区日历日）：信用卡「下次账单节点」派生的时间基点。
fn today_local() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

/// 归档确认 spec（warning 级，#1955 真话矩阵）：一句通用正文 + 「余额非零」
/// 句与「未结束计划数」句按在场与否追加，两者都不阻断；「无句可说」时独立
/// 呈现（仅通用正文）。余额句经与列表同一金额格式化接缝（含隐私掩码）。
fn archive_spec_for(
    account_id: &str,
    plan_count_for: impl Fn(&str) -> i64 + Copy + 'static,
    balances: Signal<BalancesSnap>,
    currencies: Signal<CurrencyTable>,
    privacy: Signal<bool>,
) -> danger::DangerConfirmSpec {
    let snap = balances.get_untracked();
    let Some(row) = snap.0.iter().find(|row| row.id == account_id) else {
        // 行缺席（并发删除等的比率面）：退化通用正文句，确认落地仍走写入
        // （后续写入错误归反馈条位）。
        return danger::DangerConfirmSpec {
            level: danger::DangerLevel::Warning,
            title: "归档账户".to_owned(),
            lead: Some("确认归档该账户？".to_owned()),
            strong_warning: None,
            detail: None,
            confirm_label: "归档".to_owned(),
            cancel_label: "取消".to_owned(),
        };
    };
    let name = row.name.clone();
    let mut lead = Some(format!(
        "确认归档账户「{name}」？归档后它从主表移入下方的「已归档」分区，历史与流水仍可查。"
    ));
    let mut strong = None;
    let mut detail = None;
    if row.balance_cents != 0 {
        let fmt = currencies.get_untracked().format_of(&row.currency_code);
        let mask = privacy.get_untracked();
        let amount = if mask {
            assembly::AMOUNT_PRIVACY_MASK.to_owned()
        } else {
            assembly::format_amount_basic(row.balance_cents, &fmt)
        };
        strong = Some(format!(
            "该账户当前余额 {amount}，归档后仍计入净资产与余额合计——金额不会消失；要让这笔钱退出净资产，请用「调整余额」把余额归零。"
        ));
    }
    let plan_count = plan_count_for(account_id);
    if plan_count > 0 {
        detail = Some(format!(
            "归档不会停用计划: 仍有 {plan_count} 个未结束的定时计划指向该账户，它们照常执行。"
        ));
    }
    if strong.is_none() && detail.is_none() {
        // 「无句可说」：单句正文的 #1953 渲染形态逐字节同旧壳（不回退）。
        let _ = &mut lead;
    }
    danger::DangerConfirmSpec {
        level: danger::DangerLevel::Warning,
        title: "归档账户".to_owned(),
        lead,
        strong_warning: strong,
        detail,
        confirm_label: "归档".to_owned(),
        cancel_label: "取消".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// 视图局部快照容器与表单输入换算（#2041）：快照与文本输入换算归本视图；
// 渲染期零格式化纪律下归本视图。
// ---------------------------------------------------------------------------

/// 装载批产物（读接缝单点的具名元组）：余额（V017 缓存口径）+ 币种字典 +
/// 目标绑定进度 + 计划引用集——`blocking` 读闭包返回值，复杂度聚合单点。
type AccountLoads = ledger_infra::error::Result<(
    Vec<ledger_accounts::AccountBalance>,
    Vec<ledger_currencies::Currency>,
    Vec<ledger_savings_goal::SavingsGoalProgress>,
    Vec<ledger_scheduled::ScheduledTransactionWithExt>,
)>;

/// 账户行可克隆快照（视图局部）：域行 `AccountBalance` 无 Clone 语义，清单与
/// 表单消费的字段全集在装载批一次性投影持有（清单呈现/表单回填/写目标：名称
/// 随动只读、信用卡档案三键与余额）；不参与金额算术（余额读 V017 缓存口径，
/// 只消费）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AccountRowSnap {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) kind: ledger_accounts::AccountType,
    pub(crate) currency_code: String,
    pub(crate) is_archived: bool,
    pub(crate) balance_cents: i64,
    pub(crate) credit_limit_cents: Option<i64>,
    pub(crate) statement_day: Option<i64>,
    pub(crate) due_day: Option<i64>,
}

/// 行集投影 [`AccountRowSnap`] 的集合容器（域行无 Clone 语义，装载批一次性投影）。
/// 载荷等值语义按信号判（derive Vec 逐行比较，非成员字段不计入呈现等值）。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct BalancesSnap(pub Vec<AccountRowSnap>);

/// 域行 → 行快照投影（装载批单点）：清单/表单消费的字段全集转换唯一（装载
/// 批投影、后续所有消费面读同一份快照，不另立第二映射）。
fn account_row_snap_of(row: &ledger_accounts::AccountBalance) -> AccountRowSnap {
    AccountRowSnap {
        id: row.account.id.clone(),
        name: row.account.name.clone(),
        kind: row.account.kind,
        currency_code: row.account.currency_code.clone(),
        is_archived: row.account.is_archived,
        balance_cents: row.balance_cents,
        credit_limit_cents: row.account.credit_limit_cents,
        statement_day: row.account.statement_day,
        due_day: row.account.due_day,
    }
}

/// 定时计划行集快照容器（视图信号持有用）：域读路径 [`boot::plan_rows`] 行集
/// 投影 [`assembly::PlanLeadFace`] 的集合容器（域行无 Clone 语义，装载批一次
/// 性投影——本视图只消费引用计数的判据字段）。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PlansSnap(pub Vec<assembly::PlanLeadFace>);

/// 信用卡档案日输入（账单日/还款日）文本 → 声明日值：空/非法回 `None`
/// （携带 `None` = 清空，旧壳 NInputNumber 空值同语义）；可解析即原值透传，
/// 1–31 越界值不在此拦截——旧壳不拦、后端以码化错误显式拒绝（旧壳
/// 「越界不在此拦截」注释同口径）。
fn day_text_to_i64(text: &str) -> Option<i64> {
    text.trim().parse::<i64>().ok()
}

/// 表格外围定宽扣减（账户列宽计算输入）：内容列 p-4（两侧 16pt）+ 分区卡
/// p-3（两侧 12pt）+ 分区卡描边两侧 1pt——表格可用宽比视口内容窄出的固定
/// 外围（窄窗自适应随系列尾部横切票）。
const ACCOUNT_TABLE_WIDTH_PAD: f32 = 16.0 * 2.0 + 12.0 * 2.0 + 2.0;

/// 账户列宿主映射（[`ACCOUNT_COLUMNS`] → 宿主列；UI 层只做映射不产列）：
/// 弹性宽度分配同 [`transactions::elastic_columns`] 先例——列组件无弹性列机制，富余
/// 宽度全给名称列（旧壳名称列弹性），其余列按装配建议宽度；总宽不得超出
/// 表格实测可用宽（列总和超出视口时尾列不挂载——DataTable 虚拟化按可视窗
/// 裁列，操作列缺席即右键/「⋯」双入口灭失）。
fn account_columns(width: f32) -> Vec<DataTableColumn> {
    let mut columns: Vec<DataTableColumn> = ACCOUNT_COLUMNS
        .iter()
        .map(|def| {
            let mut column = DataTableColumn::default();
            column.key = def.key.into();
            column.title = Some(def.title.into());
            column.width = Some(Points(def.width));
            column.align = match def.align {
                ColumnAlign::Start => DataTableAlign::Start,
                ColumnAlign::End => DataTableAlign::End,
            };
            column
        })
        .collect();
    let suggested: f32 = ACCOUNT_COLUMNS.iter().map(|def| def.width).sum();
    let extra = (width - ACCOUNT_TABLE_WIDTH_PAD - suggested).max(0.0);
    if extra > 0.0
        && let Some(name) = columns.iter_mut().find(|col| col.key == "name")
        && let Some(w) = name.width
    {
        name.width = Some(Points(w.0 + extra));
    }
    columns
}
