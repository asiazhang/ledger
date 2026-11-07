// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块与外挂 tests.rs）经 crate 根 cfg(test) 整体
// 放行，生产构建零放宽。

//! 物品视图（issue #2044 NSH 迁移⑬；旧壳 `ItemsView.vue` + `src/item/`
//! 组件树的重生）。
//!
//! **总题解释（先例裁决）**：票面以「购买项」冠名并把 ADR-0138 订单形态的
//! 呈现词表（每项一行、订单级列纵向合并、分类列逐行、计数按交易计）挂在
//! 迁移目标上；旧壳 `ItemsView.vue` 承载的是**物品域**（Item，CONTEXT.md
//! `Item` / ADR-0014 的独立领域实体），清单是每物品一行的平表（名称 / 购买
//! 日期 / 状态 / 总成本 / 已用天数 / 每天成本），与 ADR-0138 的购买项
//! （`transaction_purchases` 交易子表——呈现随交易列表 / 搜索票已迁、纠错靠
//! 按幂等键重导、界面只读是决策 15 的定案）不同源。本票按 spec #2031「旧壳
//! 适用行为等价判据：零改动、全套既有测试保持绿」的平价纪律执行：清单平表 +
//! 编辑 / 详情 / 处置 / 软删 + 自选参考日重算 + 来源跳转行高亮逐条重生，不
//! 引入旧壳不存在的新呈现；偏差条目在交付报告对表。
//!
//! 实现明细见各区块注释；整体职责边界（按旧壳核实平价）：清单卡片（装配
//! 产物渲染、零新增列）+ 创建唯一入口提示条（ADR-0025：物品只能经交易右键
//! 「加入物品」创建；NSH 的该入口随本票挂交易行菜单，本文件提供确认弹窗）+
//! 编辑弹窗（换关带出覆盖、币种不可改）+ 详情弹窗（成本分解、自选参考日
//! 重算、本位币折算读账本本位币）+ 处置弹窗（日期必填、残值可选，已处置 =
//! 修正处置信息）+ 软删轻量确认气泡（ADR-0078 轻量档平价：旧壳
//! AppPopconfirm 同形态，气泡挂行内删除按钮）。写路径直调域公开写入口
//! （不经 IPC，ADR-0145 决策 4），成功后广播交易失效信号（交易列表来源列
//! 展示物品名 / 处置态——旧壳 `ledger:changed` 粗粒度语义的壳内对应物；
//! 广播接线删除即红钉在 headless 名字改后来源列重读旅程）。
//!
//! 界面文案仅 zh-CN（旧壳 items.* zh-CN 词条原样字面承载）；码化错误 zh 原样
//! 呈现；视图本地状态、会话内保留不做（离开再回 = 全新装载，旧壳重挂同口径）。
//! 窄窗横向滚动下限（旧壳 issue #849 / ADR-0088 决策 11）归系列尾部窄窗横切
//! 票（Out of Scope，spec #2031）；行聚焦的滚动定位 NSH 无宿主面（虚拟化
//! 滚动需组件句柄，封装层现无该窗口），本壳落点 = 高亮呈现，滚动位随窄窗
//! 横切票。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use ledger_infra::error::AppError;
use nsh_base::{
    DataTableAlign, DataTableColumn, DataTableContextMenu, DataTableRows, button, data_table, input,
};
use nsh_core::element::AnyElement;
use nsh_core::element::desktop::MenuItem;
use nsh_core::element::{FontWeight, SharedString};
use nsh_core::prelude::*;

use crate::app::{db_slot, field_label, table_classes};
use crate::assembly::{self, ColumnAlign, CurrencyTable, ITEM_COLUMNS, ItemListContext};
use crate::boot;
use crate::context_menu;
use crate::date_select;
use crate::modal;
use crate::nav_model::{NavRequest, ViewName};
use crate::pinyin_select;
use crate::popconfirm;
use crate::{nav, signals};
use ledger_item::{ItemDailyCost, ItemDisposeInput, ItemInput, ItemWithDailyCost};

// ---------------------------------------------------------------------------
// 词表（旧壳 items.* zh-CN 词条原样；i18n 框架级方案不在本票，字面承载）。
// ---------------------------------------------------------------------------
const CREATE_HINT: &str =
    "物品不支持直接新增：请在「交易」页右键一笔支出交易，选择“加入物品”创建。";
const GO_TRANSACTIONS: &str = "去交易页";
const LIST_CARD_TITLE: &str = "物品列表";
const STATUS_IN_USE: &str = "在用";
const EMPTY_GUIDE: &str = "暂无物品：请在「交易」页右键一笔支出交易，选择“加入物品”创建。";
const ROW_DETAIL: &str = "详情";
const ROW_EDIT: &str = "编辑";
const ROW_DISPOSE: &str = "处置";
const ROW_DISPOSE_INFO: &str = "处置信息";
const ROW_DELETE: &str = "删除";
const ROW_CANCEL: &str = "取消";
const ROW_SAVE: &str = "保存";
const DELETE_CONFIRM: &str = "不再跟踪该物品，从列表移除？";
const EDIT_TITLE: &str = "编辑物品";
const EDIT_LABEL_NAME: &str = "名称";
const EDIT_LABEL_DATE: &str = "购买日期";
const EDIT_LABEL_COST: &str = "总成本";
const EDIT_PLACEHOLDER_COST: &str = "总成本（元）";
const EDIT_LABEL_LINKED_TX: &str = "关联购买交易";
const EDIT_LABEL_CURRENCY: &str = "币种";
const EDIT_LABEL_NOTE: &str = "备注";
const EDIT_PLACEHOLDER_NOTE: &str = "品牌 / 型号 / 渠道（可选）";
const EDIT_CURRENCY_FIXED: &str = "（不可修改）";
const DETAIL_TITLE: &str = "物品详情";
const DETAIL_LABEL_NAME: &str = "名称";
const DETAIL_LABEL_STATUS: &str = "状态";
const DETAIL_LABEL_PURCHASE_DATE: &str = "购买日期";
const DETAIL_LABEL_DISPOSAL_DATE: &str = "处置日期";
const DETAIL_LABEL_RESIDUAL: &str = "残值";
const DETAIL_LABEL_TOTAL_COST: &str = "总成本";
const DETAIL_LABEL_NATIVE: &str = "本位币折算";
const DETAIL_LABEL_NOTE: &str = "备注";
const DETAIL_LABEL_LINKED_TX: &str = "关联购买交易";
const DETAIL_LABEL_REF_DATE: &str = "参考日";
const DETAIL_LABEL_USED_DAYS: &str = "已用天数";
const DETAIL_LABEL_PER_DAY: &str = "每天成本分解";
const DETAIL_LINKED_YES: &str = "已关联（溯源）";
const DETAIL_REF_CLEAR: &str = "清空";
const DISPOSE_TITLE_IN_USE: &str = "处置物品";
const DISPOSE_TITLE_INFO: &str = "处置信息";
const DISPOSE_LABEL_ITEM: &str = "物品";
const DISPOSE_LABEL_DATE: &str = "处置日期";
const DISPOSE_LABEL_RESIDUAL: &str = "残值";
const DISPOSE_RESIDUAL_PLACEHOLDER: &str = "残值（元，可选）";
const DISPOSE_CONFIRM: &str = "确认处置";
const ADD_TITLE: &str = "加入物品";
const ADD_LABEL_PURCHASE_DATE: &str = "购买日期";
const ADD_LABEL_BASE_COST: &str = "基础成本";
const ADD_LABEL_NAME: &str = "物品名称";
const ADD_NAME_PLACEHOLDER: &str = "默认取交易备注，可微调";
const ADD_CONFIRM: &str = "确认创建";
const EDIT_PLACEHOLDER_NAME: &str = "物品名称";
const MSG_NAME_REQUIRED: &str = "请输入物品名称";
const MSG_COST_INVALID: &str = "请输入大于 0 的总成本";
const MSG_RESIDUAL_INVALID: &str = "残值需为不小于 0 的金额";
const MSG_SAVED: &str = "已保存";
const MSG_DISPOSAL_OK: &str = "已处置";
const MSG_DISPOSE_UPDATED: &str = "已更新处置信息";
const MSG_DELETED: &str = "已删除";
const MSG_ADDED: &str = "已加入物品";
const MSG_SAVE_FAILED: &str = "保存失败";
const MSG_DISPOSE_FAILED: &str = "处置失败";
const MSG_DELETE_FAILED: &str = "删除失败";
const MSG_RECALC_FAILED: &str = "重算失败";
const MSG_PLAIN: &str = "—";

/// 本位币基准读取失败的软回退（旧壳缺省 CNY 同口径：折算后缀不阻断列表）。
const DEFAULT_BASE_CURRENCY: &str = "CNY";

/// 装载占位与任务缺省错误文案（旧壳 loading / 异常分支同口径）。
const LOADING_PLACEHOLDER: &str = "正在加载…";
const LOAD_TASK_FAILED: &str = "内部错误：读任务异常退出";
const WRITE_TASK_FAILED: &str = "内部错误：写入任务异常退出";

fn dispose_label_by_flag(in_use: bool) -> &'static str {
    if in_use {
        ROW_DISPOSE
    } else {
        ROW_DISPOSE_INFO
    }
}

// ---------------------------------------------------------------------------
// 快照与流形状
// ---------------------------------------------------------------------------

/// 装载批产物（读接缝单点的具名元组）。
type ItemLoads = ledger_infra::error::Result<(
    Vec<ItemWithDailyCost>,
    Vec<ledger_currencies::Currency>,
    String,
    Vec<ledger_transaction::Transaction>,
)>;

/// 编辑 / 处置 / 详情目标的行反查单点（同一快照单点，不另立第二映射）。
fn snap_of<'a>(snapshot: &'a [ItemWithDailyCost], id: &str) -> Option<&'a ItemWithDailyCost> {
    snapshot.iter().find(|row| row.item.id == id)
}

/// **交易视图消费面**（「加入物品」行菜单项的溯源唯一准入，旧壳
/// AddItemForm 的 `hasItem` 判据同源，items store 按溯源指针比对不新增查询）：
/// 快照内存在指向该交易的未删除物品 → 菜单项置灰。禁令事实源在域创建守卫。
pub(crate) fn has_item_for_tx(snapshot: &[ItemWithDailyCost], transaction_id: &str) -> bool {
    snapshot
        .iter()
        .any(|row| row.item.purchase_transaction_id.as_deref() == Some(transaction_id))
}

/// 物品行集快照容器（视图信号持有用）：域行 `ItemWithDailyCost` 无 `PartialEq`
/// 语义，信号载荷等值以「id + updated_at + version + 天数快照」呈现等值面
/// （账号行快照容器同形，呈现等值即反复重写不通知）。
#[derive(Debug, Clone, Default)]
pub(crate) struct ItemsSnap(pub Vec<ItemWithDailyCost>);
impl PartialEq for ItemsSnap {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(other.0.iter()).all(|(a, b)| {
                a.item.id == b.item.id
                    && a.item.updated_at == b.item.updated_at
                    && a.item.version == b.item.version
                    && a.used_days == b.used_days
            })
    }
}

/// 候选交易行集快照容器（视图信号持有用）：等值面 = id + updated_at（同上）。
#[derive(Debug, Clone, Default)]
pub(crate) struct CandidatesSnap(pub Vec<ledger_transaction::Transaction>);
impl PartialEq for CandidatesSnap {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self
                .0
                .iter()
                .zip(other.0.iter())
                .all(|(a, b)| a.id == b.id && a.updated_at == b.updated_at)
    }
}

/// 详情重算结果快照容器（视图信号持有用）：Option 的呈现等值面（同上）。
#[derive(Debug, Clone, Default)]
pub(crate) struct RecalcSnap(pub Option<ItemDailyCost>);
impl PartialEq for RecalcSnap {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => {
                a.used_days == b.used_days
                    && a.per_day_cents == b.per_day_cents
                    && a.numerator_cents == b.numerator_cents
            }
            (None, None) => true,
            _ => false,
        }
    }
}

/// 加入物品弹窗目标容器（信号持有用；`Transaction` 无 `PartialEq`，等值面 =
/// 目标 id——同 id 重开不重建弹窗内容）。
#[derive(Debug, Clone, Default)]
pub(crate) struct AddItemTarget(pub Option<ledger_transaction::Transaction>);
impl PartialEq for AddItemTarget {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_ref().map(|tx| tx.id.clone()) == other.0.as_ref().map(|tx| tx.id.clone())
    }
}

/// 「今天」（本地时区日历日）：处置日期缺省与参考日缺省的同一时间基点。
fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

// ---------------------------------------------------------------------------
// 物品视图（导航骨架物品分支的真实组件，#2044）。
// ---------------------------------------------------------------------------
#[component]
pub(crate) fn ItemsView() -> impl IntoElement {
    let view_owner = Owner::current();

    // ============================ 快照态 ============================
    let items = signal(ItemsSnap::default());
    let currencies = signal(CurrencyTable::default());
    let base_currency = signal(DEFAULT_BASE_CURRENCY.to_owned());
    let candidates = signal(CandidatesSnap::default());
    let loading = signal(true);
    let error = signal(None::<String>);
    let load_seq = signal(0u64);

    let reload = move || {
        loading.set(true);
        error.set(None);
        load_seq.update(|seq| *seq += 1);
        let seq = load_seq.get();
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let Some(dbc) = db.clone() else {
                    error.set(Some("内部错误：账本库会话缺失".to_owned()));
                    loading.set(false);
                    return;
                };
                let outcome = blocking(move || -> ItemLoads {
                    let dbc = &dbc;
                    let rows = boot::item_rows(dbc)?;
                    let currency_rows = boot::currency_rows(dbc).unwrap_or_default();
                    let base = boot::base_currency_code(dbc)
                        .unwrap_or_else(|_| DEFAULT_BASE_CURRENCY.to_owned());
                    let candidate_rows = boot::item_candidate_transactions(dbc)
                        .ok()
                        .unwrap_or_default();
                    Ok((rows, currency_rows, base, candidate_rows))
                })
                .await;
                let loaded = outcome.ok().and_then(|inner| inner.ok());
                match loaded {
                    Some((rows, currency_rows, base, candidate_rows)) => {
                        items.set(ItemsSnap(rows));
                        currencies.set(CurrencyTable::from_currencies(&currency_rows));
                        base_currency.set(base);
                        candidates.set(CandidatesSnap(candidate_rows));
                    }
                    None => error.set(Some(LOAD_TASK_FAILED.to_owned())),
                }
                loading.set(false);
            });
        });
    };

    // ============================ 表格装配（渲染期零格式化） ============================
    let rows = computed(move || {
        let currency_table = currencies.get();
        assembly::assemble_item_rows(
            &items.get().0,
            &ItemListContext {
                currencies: &currency_table,
            },
        )
    });

    // ============================ 来源跳转落点（行高亮） ============================
    let highlight = signal(None::<String>);
    // 落位 + 意图消费（读一次封闸：「先拿 id 后等数据」——只暂存 id，行渲染
    // 后生效，旧壳 useFocusParam 同语义；已处置物品仍在列表（带处置标注）
    // 高亮不落空，软删物品不在列表、来源列不反查——无落空态）。高亮保持到
    // 实例消亡：刷新/重进 = 新实例重定位（NSH 重新导航重新投递 = 旧壳
    // 「URL 在场即复现」的进程内对应形态）。
    effect(move || {
        let _landed = nav::store().current.get();
        let intent = nav::take_intent();
        highlight.set(intent.and_then(|intent| intent.focus));
    });

    // ============================ 落位装载与失效信号 ============================
    effect(move || {
        let _landed = nav::store().current.get();
        reload();
    });
    // 失效信号订阅（旧壳 itemsStore `ledger:changed` 失效重拉的壳内对应物）：
    // 交易数据变化（含本壳各写入口广播）→ 清单重读。订阅句柄随组件实例存亡
    // （app.rs reload_subscription 同形：直放 let 会在组件体结束时 Drop 撤销）。
    let reload_subscription = signals::subscribe_transactions_changed(reload);
    effect(move || {
        let _held_for_view_lifetime: &signals::TransactionsChangedSubscription =
            &reload_subscription;
    });

    // ============================ 主题令牌 ============================
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

    // 操作反馈条位（NSH 无 toast——#2038 裁决；旧壳 message.success/error 的
    // 条位对应物：新动作开始时清回空）。
    let status = signal(None::<String>);

    // ============================ 编辑弹窗态 ============================
    // 目标按物品 id 持有（弹窗内容自快照按 id 反查）；换关（与既有关联不同）
    // 即锁定日期/成本手改并回显带出预览——后端提交时以交易值覆盖（域
    // guard::apply_purchase_link），前端预填只是口径预览不是第二事实源（旧壳
    // applyLinkedTxToEdit 同语义注释）。
    let edit_target: Signal<Option<String>> = signal(None);
    let edit_name = signal(String::new());
    let edit_date = signal(today());
    let edit_cost = signal(String::new());
    let edit_link_id = signal(None::<String>);
    let edit_note = signal(String::new());
    let edit_error = signal(None::<String>);

    let edit_snap = move || {
        edit_target
            .get_untracked()
            .and_then(|id| snap_of(&items.get_untracked().0, &id).cloned())
    };

    // 换关判定单点（锁定 / 预览的判据）：弹窗行既有溯源 vs 当前选中。判据
    // 读 edit_link_id 是**响应式**读——锁定面（when 分支）随选中变化重建。
    let is_relinking = move || {
        edit_link_id.get() != edit_snap().and_then(|snap| snap.item.purchase_transaction_id)
    };
    // 换关锁定回显（带出预览）：候选项即时可读（后端提交时以交易值覆盖，
    // 旧壳 applyLinkedTxToEdit 同语义）。
    let edit_relink_preview = move || {
        candidates
            .get_untracked()
            .0
            .iter()
            .find(|tx| Some(&tx.id) == edit_link_id.get_untracked().as_ref())
            .map(|tx| {
                let fmt = currencies.get_untracked().format_of(&tx.currency_code);
                format!(
                    "{} · {}",
                    tx.date,
                    assembly::format_amount_basic(tx.amount_cents, &fmt)
                )
            })
            .unwrap_or_default()
    };

    let open_edit = move |row_id: String| {
        let Some(snap) = snap_of(&items.get_untracked().0, &row_id).cloned() else {
            return;
        };
        let existing = snap.item;
        edit_name.set(existing.name);
        edit_date.set(date_select::parse_date_text(&existing.purchase_date).unwrap_or_else(today));
        edit_cost.set(assembly::cents_to_amount_text(
            existing.total_cost_cents,
            currencies
                .get_untracked()
                .format_of(&existing.currency_code)
                .decimal_places,
        ));
        edit_link_id.set(existing.purchase_transaction_id);
        edit_note.set(existing.note.unwrap_or_default());
        edit_error.set(None);
        edit_target.set(Some(existing.id));
    };

    let close_edit = move || edit_target.set(None);

    let perform_edit = move || {
        edit_error.set(None);
        status.set(None);
        let Some(target) = edit_snap() else {
            return;
        };
        let name = edit_name.get_untracked().trim().to_owned();
        if name.is_empty() {
            edit_error.set(Some(MSG_NAME_REQUIRED.to_owned()));
            return;
        }
        // 金额换算接缝非法（空 / 非正）→ 前置拦截不写入（旧壳 costInvalid
        // 同语义；域内同名守卫兜底）。
        let Some(cost_cents) =
            assembly::budget_yuan_to_cents(&edit_cost.get_untracked()).filter(|cents| *cents > 0)
        else {
            edit_error.set(Some(MSG_COST_INVALID.to_owned()));
            return;
        };
        // 入参语义（域 ItemInput 全量替换）：关联 Some + 与既有不同 = 新关联
        // 带出覆盖；None = 保留既有溯源（溯源只增不减，旧壳同口径）；币种按
        // 行内币种原样传（不可改——域全量替换语义的必填位）。
        let input = ItemInput {
            name,
            purchase_date: edit_date.get_untracked().to_string(),
            total_cost_cents: cost_cents,
            currency_code: target.item.currency_code.clone(),
            note: {
                let text = edit_note.get_untracked().trim().to_owned();
                (!text.is_empty()).then_some(text)
            },
            purchase_transaction_id: edit_link_id.get_untracked(),
        };
        let id = target.item.id.clone();
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::update_item(db, &id, input),
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
                        // 物品行变化（名称 / 溯源 / 成本）会改变交易列表来源列
                        // 展示——按旧壳 ledger:changed 粗粒度语义广播交易失效
                        //（广播接线删除即红：headless 名字改后来源列重读旅程）。
                        signals::notify_transactions_invalidated();
                    }
                    Ok(Err(e)) => {
                        edit_error.set(Some(format!("{MSG_SAVE_FAILED}: {e}")));
                    }
                    Err(_) => edit_error.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // ============================ 处置弹窗态 ============================
    // 处置日期必填（NSH date_select 受控件恒有值：缺省今天 / 已处置回填处置
    // 日——旧壳「为空提示」分支在受控形态不可达，录入错误由域码化错误显式
    // 拒绝）。已处置物品再开 = 修正处置信息（旧壳同语义，确认钮文案沿用
    // 「保存」）。
    let dispose_target: Signal<Option<String>> = signal(None);
    let dispose_date = signal(today());
    let dispose_residual = signal(String::new());
    let dispose_error = signal(None::<String>);

    let dispose_snap = move || {
        dispose_target
            .get_untracked()
            .and_then(|id| snap_of(&items.get_untracked().0, &id).cloned())
    };
    let disposing_in_use = move || {
        dispose_snap().is_some_and(|snap| snap.item.status == ledger_item::ItemStatus::InUse)
    };

    let open_dispose = move |row_id: String| {
        let Some(snap) = snap_of(&items.get_untracked().0, &row_id).cloned() else {
            return;
        };
        dispose_date.set(
            snap.item
                .disposal_date
                .as_ref()
                .and_then(|date| date_select::parse_date_text(date))
                .unwrap_or_else(today),
        );
        dispose_residual.set(
            snap.item
                .residual_value_cents
                .map(|cents| {
                    assembly::cents_to_amount_text(
                        cents,
                        currencies
                            .get_untracked()
                            .format_of(&snap.item.currency_code)
                            .decimal_places,
                    )
                })
                .unwrap_or_default(),
        );
        dispose_error.set(None);
        dispose_target.set(Some(snap.item.id));
    };

    let close_dispose = move || dispose_target.set(None);

    let perform_dispose = move || {
        dispose_error.set(None);
        status.set(None);
        let Some(target) = dispose_snap() else {
            close_dispose();
            return;
        };
        // 残值换算（金额换算接缝单点）：空 = 无残值；非空必须落到不小于 0 的
        // 整数分（旧壳 residualInvalid 同语义，域内同名守卫兜底）。
        let residual_text = dispose_residual.get_untracked().trim().to_owned();
        let residual_cents = if residual_text.is_empty() {
            None
        } else {
            match assembly::budget_yuan_to_cents(&residual_text) {
                Some(cents) if cents >= 0 => Some(cents),
                _ => {
                    dispose_error.set(Some(MSG_RESIDUAL_INVALID.to_owned()));
                    return;
                }
            }
        };
        let input = ItemDisposeInput {
            disposal_date: dispose_date.get_untracked().to_string(),
            residual_value_cents: residual_cents,
        };
        let id = target.item.id.clone();
        let was_in_use = target.item.status == ledger_item::ItemStatus::InUse;
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::dispose_item(db, &id, input),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        close_dispose();
                        status.set(Some(if was_in_use {
                            MSG_DISPOSAL_OK.to_owned()
                        } else {
                            MSG_DISPOSE_UPDATED.to_owned()
                        }));
                        signals::notify_transactions_invalidated();
                    }
                    Ok(Err(e)) => dispose_error.set(Some(format!("{MSG_DISPOSE_FAILED}: {e}"))),
                    Err(_) => dispose_error.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // ============================ 软删（轻量确认气泡锚定行内删除按钮） ============================
    let perform_delete = move |id: String| {
        status.set(None);
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::delete_item(db, &id),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        status.set(Some(MSG_DELETED.to_owned()));
                        signals::notify_transactions_invalidated();
                    }
                    Ok(Err(e)) => status.set(Some(format!("{MSG_DELETE_FAILED}: {e}"))),
                    Err(_) => status.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };
    // 软删确认气泡的开合（单实例受控：同一时刻只允许一个气泡在屏）。ESC /
    // 点外由弹层宿主关闭（封装 on_open_change 回写）。
    let popconfirm_open = signal(None::<String>);

    // ============================ 详情弹窗态 ============================
    let detail_target = signal(None::<String>);
    // 自选参考日重算（旧壳 issue #121）：打开弹窗不自动重算（列表快照展示）；
    // 选择参考日 → 域单件成本命令重算三元组覆盖展示；清空 → 回缺省口径
    //（列表快照值，重算结果一并撤回）。NSH date_select 受控件恒有值，缺省
    // 基点 = 今天；重算触发经「开窗抑制一轮」的 effect 追踪（程序化置值不
    // 触发重算、用户改值即触发），旧壳 update-formatted-value 事件的等价形。
    let detail_ref_date = signal(today());
    let detail_dirty = signal(false);
    let detail_cost = signal(RecalcSnap::default());
    let detail_error = signal(None::<String>);
    let detail_suppress = signal(true);

    let detail_snap = move || {
        detail_target
            .get_untracked()
            .and_then(|id| snap_of(&items.get_untracked().0, &id).cloned())
    };

    let close_detail = move || detail_target.set(None);

    let perform_recalc = move |date: chrono::NaiveDate| {
        detail_ref_date.set(date);
        detail_dirty.set(true);
        detail_error.set(None);
        if detail_target.get_untracked().is_none() {
            return;
        }
        let Some(target) = detail_snap() else {
            return;
        };
        let id = target.item.id.clone();
        let date_text = date.to_string();
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::item_cost(db, &id, Some(&date_text)),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(cost)) => detail_cost.set(RecalcSnap(Some(cost))),
                    Ok(Err(e)) => detail_error.set(Some(format!("{MSG_RECALC_FAILED}: {e}"))),
                    Err(_) => detail_error.set(Some(LOAD_TASK_FAILED.to_owned())),
                }
            });
        });
    };
    // 清空参考日（旧壳 AppDatePicker clearable）：回缺省口径（列表快照值，
    // 重算结果一并撤回）。
    let clear_recalc = move || {
        detail_dirty.set(false);
        detail_cost.set(RecalcSnap(None));
        detail_ref_date.set(today());
        detail_suppress.set(true);
    };

    // 详情参考日选择的重算追踪：弹窗开启后的信号变化 → 非抑制变更即重算。
    let recalc_watch = perform_recalc;
    effect(move || {
        if detail_target.get().is_none() {
            return;
        }
        let date = detail_ref_date.get();
        if detail_suppress.get_untracked() {
            detail_suppress.set(false);
            return;
        }

        recalc_watch(date);
    });

    // ============================ 创建唯一入口提示 ============================
    let go_transactions = move || nav::open(NavRequest::bare(ViewName::Transactions));

    // ============================ 行操作（行内按钮组 + 右键菜单） ============================
    // 旧壳行操作 = 行内小按钮组（详情/编辑/处置|处置信息/删除 + Popconfirm）。
    // NSH 同位承载：行内按钮组照旧呈现（旧壳重算无右键菜单面，本壳右键是
    // 账户视图先例的行菜单形态——承载详情/编辑/处置；删除的轻量确认气泡以
    // 行内按钮为锚点，右键菜单不下delete动作，差异入交付报告对表）。
    let open_row_menu = move |row_index: usize, at: Point<Points>| {
        let snapshot = rows.get_untracked();
        let Some(assembled) = snapshot.get(row_index).cloned() else {
            return;
        };
        let id = assembled.id.clone();
        let mut items_menu = Vec::new();
        items_menu.push(MenuItem::new(ROW_DETAIL).on_select({
            let id = id.clone();
            move || detail_target.set(Some(id.clone()))
        }));
        items_menu.push(MenuItem::new(ROW_EDIT).on_select({
            let id = id.clone();
            move || open_edit(id.clone())
        }));
        items_menu.push(
            MenuItem::new(dispose_label_by_flag(
                assembled.status_label == STATUS_IN_USE,
            ))
            .on_select({
                let id = id.clone();
                move || open_dispose(id.clone())
            }),
        );
        context_menu::open_context_menu(items_menu, at);
    };

    // 列定义映射（装配列 → 宿主列；富余宽全给名称列——旧壳同弹性）。
    let columns = computed(move || {
        let width = window().width.0 - crate::sidebar::SIDEBAR_WIDTH;
        item_columns(width)
    });

    // 行高亮类（rowClasses 段）：highlight id 命中行加高亮底色（旧壳
    // `.item-row-focus` 琥珀底的 NSH 对应形态——主题 accent 透明底，双态自适应；
    // 空串段被行渲染器跳过）。
    let row_classes = move || {
        let focused = highlight.get();
        rows.get()
            .iter()
            .map(|row| {
                if focused.as_deref() == Some(row.id.as_str()) {
                    ITEM_ROW_FOCUS_CLASS.to_owned()
                } else {
                    String::default()
                }
            })
            .map(SharedString::from)
            .collect::<Vec<SharedString>>()
    };

    // 表格数据（渲染期零格式化：格文本已装配）。
    let data_of = move || {
        let snapshot = rows.get();
        DataTableRows::new(snapshot.len())
            .texts("name", snapshot.iter().map(|r| r.name.as_str()))
            .texts(
                "purchase_date",
                snapshot.iter().map(|r| r.purchase_date.as_str()),
            )
            .texts("status", snapshot.iter().map(|r| r.status_label.as_str()))
            .texts("total_cost", snapshot.iter().map(|r| r.total_cost.as_str()))
            .texts(
                "used_days",
                snapshot.iter().map(|r| r.used_days_text.as_str()),
            )
            .texts(
                "per_day_cost",
                snapshot.iter().map(|r| r.per_day_cost.as_str()),
            )
            .texts("actions", snapshot.iter().map(|_| ""))
    };

    // 自绘格：名称列（聚焦行 test_id 锚点）、其余文本列、操作列（行内按钮组
    // = 详情 / 编辑 / 处置|处置信息 / 删除气泡）。
    let cell_of = move |cell: &nsh_base::DataTableCell| -> nsh_core::element::AnyElement {
        let index = cell.row as usize;
        let assembled = rows.get_untracked().get(index).cloned();
        match cell.key.as_ref() {
            "name" => {
                let focused = assembled.as_ref().is_some_and(|row| {
                    highlight.get_untracked().as_deref() == Some(row.id.as_str())
                });
                let mut name_view = view();
                if focused {
                    name_view = name_view.test_id("item-row-focus");
                }
                name_view
                    .child(
                        text(cell.text.clone())
                            .text_size(px(13.0))
                            .text_color(fg)
                            .number_of_lines(1),
                    )
                    .into_element()
            }
            "actions" => {
                let Some(row) = assembled else {
                    return view().into_element();
                };
                let is_in_use = row.status_label == STATUS_IN_USE;
                let id = row.id.clone();
                view()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(row_action_button(format!("item-detail-{id}"), ROW_DETAIL, fg, {
                        let id = id.clone();
                        move || detail_target.set(Some(id.clone()))
                    }))
                    .child(row_action_button(format!("item-edit-{id}"), ROW_EDIT, fg, {
                        let id = id.clone();
                        move || open_edit(id.clone())
                    }))
                    .child(row_action_button(
                        format!("item-dispose-{id}"),
                        dispose_label_by_flag(is_in_use),
                        fg,
                        {
                            let id = id.clone();
                            move || open_dispose(id.clone())
                        },
                    ))
                    .child(
                        // 轻量确认气泡（ADR-0078 轻量档：旧壳 AppPopconfirm 同
                        // 形态——软删无恢复入口，确认文案把去向说清楚；error
                        // 色确认键归封装）。
                        popconfirm::popconfirm(
                            "item-delete",
                            {
                                let id = id.clone();
                                move || popconfirm_open.get().as_deref() == Some(id.as_str())
                            },
                            {
                                let id = id.clone();
                                move |shown: bool| popconfirm_open.set(shown.then(|| id.clone()))
                            },
                            popconfirm::PopconfirmSpec {
                                message: DELETE_CONFIRM.to_owned(),
                                confirm_label: ROW_DELETE.to_owned(),
                                cancel_label: ROW_CANCEL.to_owned(),
                            },
                            // 触发件用纯视图（nsh-base Button 自含按压语义
                            // 会吞掉外层按压——封装的按压源是外层包裹视图，
                            // 预算删除先例同注）。
                            view()
                                .app_class(
                                    "px-2 py-0.5 rounded-md border border-border text-foreground hover:bg-accent",
                                )
                                .child(
                                    text(ROW_DELETE)
                                        .text_size(px(12.0))
                                        .text_color(fg)
                                        .test_id(format!("item-delete-{id}").as_str()),
                                ),
                            move || perform_delete(id.clone()),
                        ),
                    )
                    .into_element()
            }
            _ => text(cell.text.clone())
                .text_size(px(13.0))
                .text_color(fg)
                .number_of_lines(1)
                .into_element(),
        }
    };

    // ============================ 编辑弹窗内容 ============================
    let edit_modal_content = move || {
        let relinking = is_relinking();
        // 币种只读位（行内币种，旧壳 currencyFixed 同语义）。
        let currency_text = format!(
            "{}{}",
            edit_snap()
                .map(|snap| snap.item.currency_code)
                .unwrap_or_default(),
            EDIT_CURRENCY_FIXED
        );
        view()
            .flex_col()
            .gap_2()
            .child(row_field(
                EDIT_LABEL_NAME,
                muted,
                input()
                    .test_id("item-edit-name")
                    .value(move || edit_name.get())
                    .placeholder(EDIT_PLACEHOLDER_NAME)
                    .optional_label("物品名称")
                    .text_color(fg)
                    .w(px(220.0))
                    .app_class(
                        "bg-background rounded-md border border-border px-2 py-1 \
                         placeholder:text-muted-foreground",
                    )
                    .on_change(move |text: &str| edit_name.set(text.to_owned())),
            ))
            .child(
                // 购买日期：换关锁定回显（带出预览），未换关走日期选择器
                //（手改仍有义：域守卫只在换关时带出覆盖——旧壳 relinking 禁用
                // 同语义的锁定面差异入交付报告对表：本壳锁定 = 只换关期间
                // 回显，提交即带出覆盖）。
                when(is_relinking, move || {
                    locked_text_field(EDIT_LABEL_DATE, muted, edit_relink_preview())
                })
                .otherwise(move || {
                    row_field(
                        EDIT_LABEL_DATE,
                        muted,
                        date_select::date_select("item-edit-date", edit_date),
                    )
                }),
            )
            .child(
                // 总成本：锁定面语义同上（换关后由后端带出覆盖，旧壳手改
                // 排除；本壳锁定期间不呈现手改入口，提交即覆盖）。
                when(is_relinking, move || {
                    locked_text_field(EDIT_LABEL_COST, muted, edit_relink_preview())
                })
                .otherwise(move || {
                    row_field(
                        EDIT_LABEL_COST,
                        muted,
                        input()
                            .test_id("item-edit-cost")
                            .value(move || edit_cost.get())
                            .placeholder(EDIT_PLACEHOLDER_COST)
                            .optional_label("总成本")
                            .text_color(fg)
                            .w(px(140.0))
                            .app_class(
                                "bg-background rounded-md border border-border px-2 py-1 \
                                 placeholder:text-muted-foreground",
                            )
                            .on_change(move |text: &str| edit_cost.set(text.to_owned())),
                    )
                }),
            )
            .child(row_field(
                EDIT_LABEL_LINKED_TX,
                muted,
                pinyin_select::pinyin_select(
                    "item-edit-link",
                    move || {
                        candidate_options(candidates.get_untracked().0, &currencies.get_untracked())
                    },
                    edit_link_id,
                    EDIT_LABEL_LINKED_TX,
                    false,
                ),
            ))
            .child(row_field(
                EDIT_LABEL_CURRENCY,
                muted,
                text(currency_text)
                    .text_size(px(13.0))
                    .text_color(muted)
                    .test_id("item-edit-currency"),
            ))
            .child(row_field(
                EDIT_LABEL_NOTE,
                muted,
                input()
                    .test_id("item-edit-note")
                    .value(move || edit_note.get())
                    .placeholder(EDIT_PLACEHOLDER_NOTE)
                    .optional_label("备注")
                    .text_color(fg)
                    .w(px(240.0))
                    .app_class(
                        "bg-background rounded-md border border-border px-2 py-1 \
                         placeholder:text-muted-foreground",
                    )
                    .on_change(move |text: &str| edit_note.set(text.to_owned())),
            ))
            .child(error_line(
                move || edit_error.get().is_some(),
                move || {
                    format!(
                        "{}: {}",
                        MSG_SAVE_FAILED,
                        edit_error.get().unwrap_or_default()
                    )
                },
                danger_fg,
                "item-edit-error",
            ))
            .child(action_buttons(
                close_edit,
                perform_edit,
                ROW_SAVE,
                fg,
                "item-edit",
            ))
    };

    // ============================ 处置弹窗内容 ============================
    let dispose_modal_content = move || {
        let in_use = disposing_in_use();
        view()
            .flex_col()
            .gap_2()
            .child(row_field(
                DISPOSE_LABEL_ITEM,
                muted,
                text(
                    dispose_snap()
                        .map(|snap| snap.item.name)
                        .unwrap_or_default(),
                )
                .text_size(px(13.0))
                .text_color(fg)
                .test_id("item-dispose-name"),
            ))
            .child(row_field(
                DISPOSE_LABEL_DATE,
                muted,
                date_select::date_select("item-dispose-date", dispose_date),
            ))
            .child(row_field(
                DISPOSE_LABEL_RESIDUAL,
                muted,
                input()
                    .test_id("item-dispose-residual")
                    .value(move || dispose_residual.get())
                    .placeholder(DISPOSE_RESIDUAL_PLACEHOLDER)
                    .optional_label("残值")
                    .text_color(fg)
                    .w(px(140.0))
                    .app_class(
                        "bg-background rounded-md border border-border px-2 py-1 \
                         placeholder:text-muted-foreground",
                    )
                    .on_change(move |text: &str| dispose_residual.set(text.to_owned())),
            ))
            .child(error_line(
                move || dispose_error.get().is_some(),
                move || {
                    format!(
                        "{}: {}",
                        MSG_DISPOSE_FAILED,
                        dispose_error.get().unwrap_or_default()
                    )
                },
                danger_fg,
                "item-dispose-error",
            ))
            .child(action_buttons(
                close_dispose,
                perform_dispose,
                if in_use { DISPOSE_CONFIRM } else { ROW_SAVE },
                fg,
                "item-dispose",
            ))
    };

    // ============================ 详情弹窗内容 ============================
    let detail_modal_content = move || {
        let snapshot_fmt = move || {
            let currency_code = detail_target
                .get_untracked()
                .and_then(|id| snap_of(&items.get_untracked().0, &id).cloned())
                .map(|row| row.item.currency_code)
                .unwrap_or_default();
            currencies.get_untracked().format_of(&currency_code)
        };
        let snapshot = detail_snap();
        // 金额呈现面（行内币种 + 本位币折算两个基准；旧壳 detailAmount 同语义）。
        let currency_code = snapshot
            .as_ref()
            .map(|row| row.item.currency_code.clone())
            .unwrap_or_default();
        let row_fmt = currencies.get_untracked().format_of(&currency_code);
        let base_fmt = currencies
            .get_untracked()
            .format_of(&base_currency.get_untracked());
        let disposed = snapshot
            .as_ref()
            .is_some_and(|row| row.item.status == ledger_item::ItemStatus::Disposed);

        let mut body = view().flex_col().gap_1();
        body = body
            .child(row_field(
                DETAIL_LABEL_NAME,
                muted,
                text(
                    snapshot
                        .as_ref()
                        .map(|row| row.item.name.clone())
                        .unwrap_or_default(),
                )
                .text_size(px(13.0))
                .text_color(fg)
                .test_id("item-detail-name"),
            ))
            .child(row_field(
                DETAIL_LABEL_STATUS,
                muted,
                text(if disposed {
                    "已处置".to_owned()
                } else {
                    STATUS_IN_USE.to_owned()
                })
                .text_size(px(13.0))
                .text_color(fg),
            ))
            .child(row_field(
                DETAIL_LABEL_PURCHASE_DATE,
                muted,
                text(
                    snapshot
                        .as_ref()
                        .map(|row| row.item.purchase_date.clone())
                        .unwrap_or_default(),
                )
                .text_size(px(13.0))
                .text_color(fg),
            ));
        if disposed {
            // 已处置行追加处置日期与残值两位（旧壳 v-if 分支同语义）。
            body = body
                .child(row_field(
                    DETAIL_LABEL_DISPOSAL_DATE,
                    muted,
                    text(
                        snapshot
                            .as_ref()
                            .and_then(|row| row.item.disposal_date.clone())
                            .unwrap_or_default(),
                    )
                    .text_size(px(13.0))
                    .text_color(fg)
                    .test_id("item-detail-disposal"),
                ))
                .child(row_field(
                    DETAIL_LABEL_RESIDUAL,
                    muted,
                    text(
                        snapshot
                            .as_ref()
                            .and_then(|row| row.item.residual_value_cents)
                            .map(|cents| assembly::format_amount_basic(cents, &row_fmt))
                            .unwrap_or_else(|| MSG_PLAIN.to_owned()),
                    )
                    .text_size(px(13.0))
                    .text_color(fg)
                    .test_id("item-detail-residual"),
                ));
        }
        body = body
            .child(row_field(
                DETAIL_LABEL_TOTAL_COST,
                muted,
                text(assembly::format_amount_basic(
                    snapshot
                        .as_ref()
                        .map(|row| row.item.total_cost_cents)
                        .unwrap_or(0),
                    &row_fmt,
                ))
                .text_size(px(13.0))
                .text_color(fg)
                .test_id("item-detail-total"),
            ))
            .child(row_field(
                DETAIL_LABEL_NATIVE,
                muted,
                text(format!(
                    "{}（{}）",
                    assembly::format_amount_basic(
                        snapshot
                            .as_ref()
                            .map(|row| row.item.cost_native_cents)
                            .unwrap_or(0),
                        &base_fmt
                    ),
                    base_currency.get_untracked()
                ))
                .text_size(px(13.0))
                .text_color(fg)
                .test_id("item-detail-native"),
            ))
            .child(row_field(
                DETAIL_LABEL_NOTE,
                muted,
                text(
                    snapshot
                        .as_ref()
                        .and_then(|row| row.item.note.clone())
                        .unwrap_or_else(|| MSG_PLAIN.to_owned()),
                )
                .text_size(px(13.0))
                .text_color(fg),
            ))
            .child(row_field(
                DETAIL_LABEL_LINKED_TX,
                muted,
                text(if snapshot
                    .as_ref()
                    .and_then(|row| row.item.purchase_transaction_id.as_ref())
                    .is_some()
                {
                    DETAIL_LINKED_YES.to_owned()
                } else {
                    MSG_PLAIN.to_owned()
                })
                .text_size(px(13.0))
                .text_color(fg)
                .test_id("item-detail-linked"),
            ))
            .child(
                // 参考日重算行（date_select + 清空钮——清空回缺省口径，重算
                // 结果一并撤回）。
                row_field(
                    DETAIL_LABEL_REF_DATE,
                    muted,
                    view()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(date_select::date_select("item-detail-ref", detail_ref_date))
                        .child(
                            button()
                                .app_class(
                                    "px-2 py-0.5 rounded-md border border-border text-foreground hover:bg-accent",
                                )
                                .on_press(move |_: &()| clear_recalc())
                                .child(
                                    text(DETAIL_REF_CLEAR)
                                        .text_size(px(12.0))
                                        .text_color(fg)
                                        .test_id("item-detail-ref-clear"),
                                ),
                        ),
                ),
            )
            .child(row_field(
                DETAIL_LABEL_USED_DAYS,
                muted,
                text(move || {
                    // 三元组展示值：重算结果优先，未重算回落列表快照（旧壳
                    // detailCostView 响应式节点同义——重算落位即时呈现）。
                    let used = detail_cost
                        .get()
                        .0
                        .as_ref()
                        .map(|cost| cost.used_days)
                        .or_else(|| {
                            detail_target
                                .get_untracked()
                                .and_then(|id| snap_of(&items.get_untracked().0, &id).cloned())
                                .map(|row| row.used_days)
                        })
                        .unwrap_or(0);
                    format!("{used} 天（含购买当日）")
                })
                .text_size(px(13.0))
                .text_color(fg)
                .test_id("item-detail-used-days"),
            ))
            .child(row_field(
                DETAIL_LABEL_PER_DAY,
                muted,
                text(move || {
                    let cost = detail_cost.get().0;
                    let (used, numerator, per_day) = match &cost {
                        Some(item_cost) => (
                            item_cost.used_days,
                            item_cost.numerator_cents,
                            item_cost.per_day_cents,
                        ),
                        None => {
                            let snapshot = detail_target
                                .get_untracked()
                                .and_then(|id| snap_of(&items.get_untracked().0, &id).cloned());
                            (
                                snapshot.as_ref().map(|row| row.used_days).unwrap_or(0),
                                snapshot.as_ref().map(|row| row.numerator_cents).unwrap_or(0),
                                snapshot.as_ref().map(|row| row.per_day_cents).unwrap_or(0.0),
                            )
                        }
                    };
                    let fmt = snapshot_fmt();
                    format!(
                        "{} ÷ {used} 天 = {}/天",
                        assembly::format_amount_f64(numerator as f64, &fmt),
                        assembly::format_amount_f64(per_day, &fmt)
                    )
                })
                .text_size(px(13.0))
                .text_color(fg)
                .test_id("item-detail-breakdown"),
            ))
            .child(error_line(
                move || detail_error.get().is_some(),
                {
                    let detail_error = detail_error;
                    move || {
                        format!(
                            "{}: {}",
                            MSG_RECALC_FAILED,
                            detail_error.get().unwrap_or_default()
                        )
                    }
                },
                danger_fg,
                "item-detail-error",
            ))
            .child(
                view()
                    .flex_row()
                    .justify_end()
                    .child(
                        button()
                            .app_class(
                                "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent",
                            )
                            .on_press(move |_: &()| close_detail())
                            .child(
                                text(ROW_CANCEL)
                                    .text_size(px(13.0))
                                    .text_color(fg)
                                    .test_id("item-detail-cancel"),
                            ),
                    ),
            );
        body
    };

    // 评论（视图树）
    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(
            view().px_4().py_2().child(
                text(ViewName::Items.label())
                    .text_size(px(16.0))
                    .text_color(fg)
                    .test_id("view-title"),
            ),
        )
        .child(
            // 装载失败条位（码化错误 zh-CN 原样，不静默——清单读数缺失不静默
            // 回退空态）。
            when(
                move || error.get().is_some(),
                move || {
                    text(move || error.get().unwrap_or_default())
                        .text_size(px(12.0))
                        .text_color(danger_fg)
                        .test_id("item-error")
                        .app_class("px-4 py-1")
                },
            )
            .otherwise(view),
        )
        .child(
            view()
                .flex_1()
                .flex_col()
                .gap_3()
                .p_4()
                .child(
                    // 创建唯一入口提示（issue #207 / ADR-0025）：本页不提供
                    // 手动新增，物品只能经交易右键「加入物品」创建（NSH 的
                    // 该入口随本票挂交易行菜单）；提示条常驻顶部并一键跳转
                    // 交易页。
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .app_class("rounded-md border border-border bg-accent/10 p-3")
                        .child(
                            text(CREATE_HINT)
                                .text_size(px(13.0))
                                .text_color(fg)
                                .flex_1()
                                .test_id("item-create-hint"),
                        )
                        .child(
                            button()
                                .app_class(
                                    "px-2 py-1 rounded-md border border-border text-foreground hover:bg-accent",
                                )
                                .on_press(move |_: &()| go_transactions())
                                .child(
                                    text(GO_TRANSACTIONS)
                                        .text_size(px(13.0))
                                        .text_color(fg)
                                        .test_id("item-go-transactions"),
                                ),
                        ),
                )
                .child(
                    // 清单卡片（卡片自身 flex_1——虚拟化表格取弹性链高度，
                    // 卡片不定高则虚拟化量出零高，先例账户视图注释同形）。
                    view()
                        .flex_1()
                        .flex_col()
                        .gap_2()
                        .app_class("rounded-md border border-border p-3")
                        .child(
                            view().flex_1().child(
                                text(LIST_CARD_TITLE)
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(fg),
                            ),
                        )
                        .child(
                            view().test_id("item-table").flex_1().child(
                                data_table()
                                    .columns(columns)
                                    .data(data_of)
                                    .row_classes(row_classes)
                                    .render_cell(cell_of)
                                    .on_context_menu(move |e: &DataTableContextMenu| {
                                        if let Some(row_index) = e.row {
                                            open_row_menu(
                                                row_index as usize,
                                                Point::new(e.x, e.y),
                                            );
                                        }
                                    })
                                    .loading(move || loading.get())
                                    .loading_view(text(LOADING_PLACEHOLDER).text_color(muted))
                                    .empty(
                                        text(EMPTY_GUIDE)
                                            .text_color(muted)
                                            .test_id("item-empty-guide"),
                                    )
                                    .classes(table_classes())
                                    .flex_1(),
                            ),
                        ),
                )
                .child(
                    // 操作反馈条位（成功/失败共用一条，新动作开始清回空）。
                    when(
                        move || status.get().is_some(),
                        move || {
                            text(move || status.get().unwrap_or_default())
                                .text_size(px(12.0))
                                .text_color(fg)
                                .test_id("item-status")
                        },
                    )
                    .otherwise(view),
                )
                .child(modal::app_modal(
                    move || edit_target.get().is_some(),
                    || EDIT_TITLE.to_owned(),
                    close_edit,
                    edit_modal_content,
                ))
                .child(modal::app_modal(
                    move || dispose_target.get().is_some(),
                    move || {
                        if disposing_in_use() {
                            DISPOSE_TITLE_IN_USE.to_owned()
                        } else {
                            DISPOSE_TITLE_INFO.to_owned()
                        }
                    },
                    close_dispose,
                    dispose_modal_content,
                ))
                .child(modal::app_modal(
                    move || detail_target.get().is_some(),
                    || DETAIL_TITLE.to_owned(),
                    close_detail,
                    detail_modal_content,
                )),
        )
}

// ---------------------------------------------------------------------------
// 行辅助件（装配产物 → 呈现）
// ---------------------------------------------------------------------------

/// 行内小操作按钮（旧壳 NButton size=tiny 的对应形态）：次级外观、单行文本、
/// headless 锚点 id 由调用方给定。
fn row_action_button(
    id: String,
    label: &'static str,
    fg: AdaptiveColor,
    on_press: impl Fn() + 'static,
) -> AnyElement {
    button()
        .app_class("px-2 py-0.5 rounded-md border border-border text-foreground hover:bg-accent")
        .on_press(move |_: &()| on_press())
        .child(text(label).text_size(px(12.0)).text_color(fg).test_id(&id))
        .into_element()
}

/// 弹窗动作按钮行（取消殿后被最右主按钮：账户视图同款文字/进制）。
fn action_buttons(
    on_cancel: impl Fn() + Copy + 'static,
    on_confirm: impl Fn() + Copy + 'static,
    confirm_label: &'static str,
    fg: AdaptiveColor,
    id: &'static str,
) -> View {
    let cancel = format!("{id}-cancel");
    let confirm = format!("{id}-confirm");
    view()
        .flex_row()
        .justify_end()
        .gap_2()
        .child(
            button()
                .app_class(
                    "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent",
                )
                .on_press(move |_: &()| on_cancel())
                .child(
                    text(ROW_CANCEL)
                        .text_size(px(13.0))
                        .text_color(fg)
                        .test_id(cancel.as_str()),
                ),
        )
        .child(
            button()
                .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90")
                .on_press(move |_: &()| on_confirm())
                .child(
                    text(confirm_label)
                        .text_size(px(13.0))
                        .text_color(rgb(0xffffff))
                        .test_id(confirm.as_str()),
                ),
        )
}

/// 高亮行类（旧壳 `tr.item-row-focus` 琥珀底的 NSH 对应形态，rowClasses 段）：
/// 颜色取主题 accent 透明度底（亮暗两态同为可读底色——旧壳 rgba 固定色值
/// 的对应升级，人工对表复核项）。
const ITEM_ROW_FOCUS_CLASS: &str = "bg-accent/25";

/// 总成本常量后缀（详情描述尾币符，旧壳 currencySuffix 同词表）。
/// 表格外围定宽扣减（物品列宽计算输入，账户表同算）。
const ITEMS_TABLE_WIDTH_PAD: f32 = 16.0 * 2.0 + 12.0 * 2.0 + 2.0;

/// 物品列宿主映射（[`ITEM_COLUMNS`] → 宿主列；UI 层只做映射不产列）：弹性
/// 宽度分配同 [`crate::app::elastic_columns`] 先例——富余宽度全给名称列。
fn item_columns(width: f32) -> Vec<DataTableColumn> {
    let mut columns: Vec<DataTableColumn> = ITEM_COLUMNS
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
    let suggested: f32 = ITEM_COLUMNS.iter().map(|def| def.width).sum();
    let extra = (width - ITEMS_TABLE_WIDTH_PAD - suggested).max(0.0);
    if extra > 0.0
        && let Some(name) = columns.iter_mut().find(|col| col.key == "name")
        && let Some(w) = name.width
    {
        name.width = Some(Points(w.0 + extra));
    }
    columns
}

// ---------------------------------------------------------------------------
// 弹窗局部字段件
// ---------------------------------------------------------------------------

/// 「标签 + 值」行为件（弹窗字段与详情描述同形；旧壳 NFormItem label-placement
/// left 同布局——label 左、值跟随）。
fn row_field(label: &'static str, muted: AdaptiveColor, value: impl IntoElement) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(field_label_boxed(label, muted))
        .child(value)
}

/// 锁定字段（换关期间的带出预览，旧壳禁用输入 + 带出值回显的对应形态）。
fn locked_text_field(label: &'static str, muted: AdaptiveColor, preview: String) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(field_label_boxed(label, muted))
        .child(
            text(preview)
                .text_size(px(13.0))
                .text_color(muted)
                .test_id("item-edit-backfill"),
        )
}

/// 错误条位（弹窗内，域码化错误 zh 原样；下次尝试开始时清空）。
fn error_line(
    show: impl Fn() -> bool + 'static,
    text_of: impl Fn() -> String + 'static,
    danger_fg: AdaptiveColor,
    id: &'static str,
) -> impl IntoElement {
    when(show, move || {
        text(text_of())
            .text_size(px(12.0))
            .text_color(danger_fg)
            .test_id(id)
    })
    .otherwise(view)
}

fn field_label_boxed(label: &'static str, muted: AdaptiveColor) -> AnyElement {
    crate::app::field_label(label, muted).into_element()
}

// ---------------------------------------------------------------------------
// 「加入物品」确认弹窗（旧壳 src/item/AddItemForm.vue 的重生；挂交易视图树）
// ---------------------------------------------------------------------------

/// 「加入物品」确认弹窗（ADR-0025 创建唯一入口的 NSH 承载面——旧壳挂交易
/// 页行右键，本壳随本票把入口接上交易行菜单）。弹窗内容自目标交易自动带出：
/// 购买日期 / 基础成本 / 币种**只读展示**（提交时原样传给后端、后端仍以交易
/// 值覆盖——前端预填只是口径预览，不是第二事实源）；名称默认取交易备注可
/// 微调。域码化错误在弹窗内呈现、弹窗保持打开，用户可取消或改名重试（旧壳
/// 同语义）。
///
/// `target` 是调用方持有的弹窗目标交易信号（None = 不呈现）；`feedback` 是
/// 调用方持有的反馈条位信号（成功文案呈现面：NSH 无 toast 的条位对应物）。
/// 信号面值语义由调用方负责（成功后广播 [signals::notify_transactions_invalidated]
/// 交易失效、清单重读由订阅者承担）。
pub(crate) fn add_item_modal(
    target: Signal<AddItemTarget>,
    currencies: Signal<CurrencyTable>,
    feedback: Signal<Option<String>>,
) -> impl IntoElement {
    let view_owner = Owner::current();
    let name = signal(String::new());
    let submitting = signal(false);
    let error = signal(None::<String>);
    let open_fn = move || target.get().0.is_some();

    // 开弹窗重置：名称默认取交易备注（可微调）；提交中与错误一并复位
    //（旧壳 AddItemForm 每次打开即新例的同语义）。
    effect(move || {
        if !open_fn() {
            return;
        }
        let Some(tx) = target.get_untracked().0 else {
            return;
        };
        name.set(tx.note.clone().unwrap_or_default());
        error.set(None);
        submitting.set(false);
    });

    let close = move || target.set(AddItemTarget(None));

    let submit = move || {
        let Some(tx) = target.get_untracked().0 else {
            return;
        };
        let name_text = name.get_untracked().trim().to_owned();
        if name_text.is_empty() {
            error.set(Some(MSG_NAME_REQUIRED.to_owned()));
            return;
        }
        submitting.set(true);
        error.set(None);
        // 创建入参（旧壳 AddItemForm submit 同形）：购买日期 / 基础成本 /
        // 币种原样传给后端（后端以交易值覆盖）、备注 None。
        let input = ItemInput {
            name: name_text,
            purchase_date: tx.date.clone(),
            total_cost_cents: tx.amount_cents,
            currency_code: tx.currency_code.clone(),
            note: None,
            purchase_transaction_id: Some(tx.id.clone()),
        };
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::create_item(db, input),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(_new_id)) => {
                        close();
                        feedback.set(Some(MSG_ADDED.to_owned()));
                        // 创建成功广播交易失效（旧壳 ledger:changed 粗粒度
                        // 语义：交易列表重读时来源列即显物品名；物品清单由
                        // 订阅重读承担）。
                        signals::notify_transactions_invalidated();
                    }
                    Ok(Err(e)) => error.set(Some(format!("加入物品失败: {e}"))),
                    Err(_) => error.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    let content = move || {
        let snapshot = target.get_untracked().0;
        // 自动带出的只读展示（购买日期 / 基础成本；币种在成本后缀，旧壳
        // addForm 只读位同形态）。
        let date_text = snapshot
            .as_ref()
            .map(|tx| tx.date.clone())
            .unwrap_or_default();
        let cost_text = snapshot
            .as_ref()
            .map(|tx| {
                let fmt = currencies.get_untracked().format_of(&tx.currency_code);
                assembly::format_amount_basic(tx.amount_cents, &fmt)
            })
            .unwrap_or_default();
        view()
            .flex_col()
            .gap_2()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label(ADD_LABEL_PURCHASE_DATE, muted_of()))
                    .child(
                        text(date_text)
                            .text_size(px(13.0))
                            .test_id("item-add-date"),
                    ),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label(ADD_LABEL_BASE_COST, muted_of()))
                    .child(
                        text(cost_text)
                            .text_size(px(13.0))
                            .test_id("item-add-cost"),
                    ),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label(ADD_LABEL_NAME, muted_of()))
                    .child(
                        input()
                            .test_id("item-add-name")
                            .value(move || name.get())
                            .placeholder(ADD_NAME_PLACEHOLDER)
                            .optional_label("物品名称")
                            .text_color(fg_of())
                            .w(px(260.0))
                            .app_class(
                                "bg-background rounded-md border border-border px-2 py-1 \
                                 placeholder:text-muted-foreground",
                            )
                            .on_change(move |text: &str| name.set(text.to_owned())),
                    ),
            )
            .child(error_line_owned(error, "item-add-error"))
            .child(
                view()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        button()
                            .app_class(
                                "px-3 py-1.5 rounded-md border border-border text-foreground hover:bg-accent",
                            )
                            .disabled(move || submitting.get())
                            .on_press(move |_: &()| close())
                            .child(text(ROW_CANCEL).text_size(px(13.0))),
                    )
                    .child(
                        button()
                            .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90")
                            .disabled(move || submitting.get())
                            .on_press(move |_: &()| submit())
                            .child(
                                text(ADD_CONFIRM)
                                    .text_size(px(13.0))
                                    .test_id("item-add-confirm"),
                            ),
                    ),
            )
    };

    modal::app_modal(open_fn, || ADD_TITLE.to_owned(), close, content)
}

/// 加入物品弹窗内的错误条位（独立自有信号；词表同电商弹窗语义）。
fn error_line_owned(error: Signal<Option<String>>, id: &'static str) -> impl IntoElement {
    when(
        move || error.get().is_some(),
        move || {
            text(move || error.get().unwrap_or_default())
                .text_size(px(12.0))
                .test_id(id)
        },
    )
    .otherwise(view)
}

fn muted_of() -> AdaptiveColor {
    let tokens = nsh_core::element::tokens();
    tokens
        .color("muted-foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.45, 1.0).into())
}

fn fg_of() -> AdaptiveColor {
    let tokens = nsh_core::element::tokens();
    tokens
        .color("foreground")
        .unwrap_or_else(|| hsla(0.0, 0.0, 0.98, 1.0).into())
}

/// 关联购买交易候选下拉选项（编辑弹窗换关数据源）：label 镜像旧壳
/// linkTxOptions「{date} · {formatAmount} · {备注}」形态。
fn candidate_options(
    transactions: Vec<ledger_transaction::Transaction>,
    currencies: &CurrencyTable,
) -> Vec<assembly::CategoryOption> {
    transactions
        .iter()
        .map(|transaction| {
            let fmt = currencies.format_of(&transaction.currency_code);
            assembly::CategoryOption {
                id: transaction.id.clone(),
                label: {
                    let mut label = format!(
                        "{} · {}",
                        transaction.date,
                        assembly::format_amount_basic(transaction.amount_cents, &fmt),
                    );
                    if let Some(text) = &transaction.note {
                        label.push_str(&format!(" · {text}"));
                    }
                    label
                },
            }
        })
        .collect()
}
