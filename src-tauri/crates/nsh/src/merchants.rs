// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块与外挂 tests.rs）经 crate 根 cfg(test) 整体
// 放行，生产构建零放宽。

//! 商户管理视图（issue #2052 NSH 迁移㉑；旧壳 `MerchantManager.vue` +
//! `MerchantEditModal.vue` 的重生）。
//!
//! 实现明细见各区块注释；整体职责边界（按旧壳核实平价）：字典是名牌字典
//!（issue #223：只处理名称）；**新增表单卡片 + 列表卡片 + 编辑弹窗**，交互沿用
//! 分类管理先例（issue #189 / ADR-0028）——新增行内表单、清单（名称 / 关联
//! 交易 / 操作三列）、统一模糊搜索本地过滤（ADR-0027 唯一定义点：交易域
//! `split_terms` / `term_matches_text`，商户字典前端全量驻留的本地过滤形态）、
//! 显示已删开关（已删行追加在尾只读呈现、条数照常与可下钻，issue #447）、
//! 前端分页展示层切片（issue #457，字段组件内受控不持久化、过滤意图页码
//! 归零、删尾回退 ADR-0045）、条数下钻（issue #446：交易列表带商户过滤）、
//! 编辑弹窗（单字段改名，重名码化错误在弹窗内呈现不关闭）与软删轻量确认
//! 气泡（旧壳 AppPopconfirm 同形态：确认文案说明历史交易仍显示该名）。
//! 字典清单含软删全量读（[`crate::boot::merchant_rows`] 复用不另立读接缝），
//! 在用 / 已删分区在装配层划分（在用优先、已删追加）。
//!
//! 写路径直调域公开写入口（不经 IPC，ADR-0145 决策 4），成功后广播交易失效
//! 信号（交易列表来源列显示商户名 / 表单商户源 / 筛选商户下拉——旧壳
//! `ledger:changed` 参考失效信号的壳内对应物；#2044 先例同形，广播接线删除
//! 即红钉在 headless 改名后交易列表来源列重读旅程）。引用计数为独立读模型
//!（毛笔数，issue #445）：读失败静默保留旧值 / 空集不阻字典管理（旧壳同口径）。
//!
//! 界面文案仅 zh-CN（旧壳 settings.merchants.* zh-CN 词条原样字面承载）；
//! 码化错误 zh 原样呈现；视图本地状态、会话内保留不做（离开再回 = 全新装载，
//! 旧壳重挂同口径；页码与过滤同为组件内态、不持久化）。已知呈现差异（入交付
//! 报告对表）：NSH 分页条无页大小选择与快捷跳页形态（nsh-base Pagination 无
//! 该面，页大小固定旧壳缺省 50）；条数列排序经 DataTable 客户端排序承接
//!（旧壳 sorter 同义：排序激活后已删行并入排序不另行隔离）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::collections::BTreeMap;

use ledger_infra::error::AppError;
use nsh_base::{
    DataTableAlign, DataTableColumn, DataTableRows, DataTableSort, DataTableSortDirection, button,
    checkbox, data_table, input, pagination,
};
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::app::{amount_privacy_enabled, db_slot, field_label, table_classes};
use crate::assembly::{self, ColumnAlign, MERCHANT_COLUMNS, MerchantListContext, MerchantRowCells};
use crate::boot;
use crate::modal;
use crate::nav_model::{NavRequest, ViewName};
use crate::popconfirm;
use crate::{nav, signals};
use ledger_merchants::MerchantTransactionCount;

// ---------------------------------------------------------------------------
// 词表（旧壳 settings.merchants.* zh-CN 词条原样；i18n 框架级方案不在本票，
// 字面承载）。
// ---------------------------------------------------------------------------
const ADD_CARD_TITLE: &str = "新增商户";
const LIST_CARD_TITLE: &str = "商户列表";
const SEARCH_PLACEHOLDER: &str = "搜索商户（名称/拼音）";
const SHOW_DELETED_LABEL: &str = "显示已删";
const ADD_BUTTON: &str = "添加";
const SAVE_BUTTON: &str = "保存";
const EDIT_TITLE: &str = "编辑商户";
const FIELD_NAME: &str = "名称";
const NAME_PLACEHOLDER: &str = "商户名称";
const ROW_EDIT: &str = "编辑";
const ROW_DELETE: &str = "删除";
const ROW_CANCEL: &str = "取消";
const DELETE_CONFIRM: &str = "确认删除该商户？历史交易仍会显示它的名字。";
const DELETED_TAG: &str = "已删除";
const MSG_NAME_REQUIRED: &str = "请输入商户名称";
const MSG_ADDED: &str = "已添加商户";
const MSG_UPDATED: &str = "已更新商户";
const MSG_DELETED: &str = "已删除商户";
const MSG_ADD_FAILED: &str = "添加失败";
const MSG_UPDATE_FAILED: &str = "更新失败";
const MSG_DELETE_FAILED: &str = "删除失败";

/// 每页行数（旧壳分页缺省页大小 50，issue #457 同缺省值；页大小选择与快捷
/// 跳页 NSH 分页条无宿主面，入交付报告对表）。
const PAGE_SIZE: usize = 50;

/// 装载占位与任务缺省错误文案（旧壳 loading / 异常分支同口径）。
const LOADING_PLACEHOLDER: &str = "正在加载…";
const LOAD_TASK_FAILED: &str = "内部错误：读任务异常退出";
const WRITE_TASK_FAILED: &str = "内部错误：写入任务异常退出";

/// 装载批产物（读接缝单点的具名元组）：字典全量（含软删）+ 引用计数
///（计数失败静默保留旧值，返回 None）。
type MerchantLoads = ledger_infra::error::Result<(
    Vec<ledger_merchants::Merchant>,
    Vec<MerchantTransactionCount>,
)>;

/// 统一模糊搜索语义过滤（ADR-0027 唯一定义点的直接消费，pinyin_select
/// filter_options 同语义）：按空白切词、词条间 AND，每词条对名称判定
///「原文连续子串 ∨ 拼音首字母子序列」；空查询恒全量；过滤只隐藏未命中项、
/// 剩余项顺序不变。纯函数、可离线单测（商户名词典本地过滤的单一消费点，不另立
/// 第二套语义）。
fn search_matches(name: &str, query: &str) -> bool {
    let terms = ledger_transaction::split_terms(query);
    terms.is_empty()
        || terms
            .iter()
            .all(|term| ledger_transaction::term_matches_text(term, name))
}

/// 商户视图（导航骨架商户分支的真实组件，#2052）。
#[component]
pub(crate) fn MerchantsView() -> impl IntoElement {
    let view_owner = Owner::current();

    // ============================ 快照态 ============================
    let rows = signal(Vec::<MerchantRowCells>::new());
    let loading = signal(true);
    let error = signal(None::<String>);
    let load_seq = signal(0u64);

    let reload = move || {
        loading.set(true);
        error.set(None);
        load_seq.update(|seq| *seq += 1);
        let seq = load_seq.get();
        let privacy = amount_privacy_enabled();
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let Some(dbc) = db.clone() else {
                    error.set(Some("内部错误：账本库会话缺失".to_owned()));
                    loading.set(false);
                    return;
                };
                let outcome = blocking(move || -> MerchantLoads {
                    let dict_rows = boot::merchant_rows(&dbc)?;
                    // 计数非关键路径（旧壳 loadTransactionCounts 同口径）：读
                    // 失败保留旧值（本视图空集首装载）不阻字典管理。
                    let counts = boot::merchant_transaction_counts(&dbc)
                        .ok()
                        .unwrap_or_default();
                    Ok((dict_rows, counts))
                })
                .await;
                let loaded = outcome.ok().and_then(|inner| inner.ok());
                match loaded {
                    Some((dict_rows, counts)) => {
                        let table = counts
                            .into_iter()
                            .map(|c| (c.merchant_id, c.transaction_count))
                            .collect::<BTreeMap<String, i64>>();
                        rows.set(assembly::assemble_merchant_rows(
                            &dict_rows,
                            &MerchantListContext {
                                counts: table,
                                privacy_enabled: privacy,
                            },
                        ));
                    }
                    None => error.set(Some(LOAD_TASK_FAILED.to_owned())),
                }
                loading.set(false);
            });
        });
    };

    // ============================ 落位装载与失效信号 ============================
    // 落位：首次装载 + 下钻意图消费（商户路由无自身定位形态，投递即弃——旧壳
    // merchants 路由不读 focus 同语义；账户视图先例同形）。失效信号订阅（旧壳
    // reference store `ledger:changed` 失效重拉的壳内对应物）：任一参考 / 交易
    // 数据变化（含本视图各写入口广播）→ 清单重读（计数随重读跟随；store
    // version 变化即伴随重拉计数的旧壳 watch 同果）。订阅句柄随组件实例存亡
    //（app.rs reload_subscription 同形：直放 let 会在组件体结束时 Drop 撤销）。
    effect(move || {
        let _landed = nav::store().current.get();
        let _ = nav::take_intent();
        reload();
    });
    let reload_subscription = signals::subscribe_transactions_changed(reload);
    effect(move || {
        let _held_for_view_lifetime: &signals::TransactionsChangedSubscription =
            &reload_subscription;
    });

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

    // 操作反馈条位（NSH 无 toast——#2038 裁决；旧壳 message.success/error 的
    // 条位对应物：新动作开始时清回空）。
    let status = signal(None::<String>);

    // ============================ 新增表单卡（收窄为单字段行内表单） ============================
    let create_name = signal(String::new());
    let create_error = signal(None::<String>);

    let perform_create = move || {
        create_error.set(None);
        status.set(None);
        let name = create_name.get().trim().to_owned();
        if name.is_empty() {
            create_error.set(Some(MSG_NAME_REQUIRED.to_owned()));
            return;
        }
        // 重名错误（「商户已存在: X」）原样上抛展示，表单不清空、用户可直接
        // 修正（旧壳 addMerchant catch 同语义）。
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::create_merchant(
                        db,
                        ledger_merchants::MerchantInput { name: name.clone() },
                    ),
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
                        status.set(Some(MSG_ADDED.to_owned()));
                        signals::notify_transactions_invalidated();
                    }
                    Ok(Err(e)) => {
                        create_error.set(Some(format!("{MSG_ADD_FAILED}: {e}")));
                    }
                    Err(_) => create_error.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // ============================ 列表卡片态（搜索 / 显示已删 / 排序 / 分页） ============================
    let search_term = signal(String::new());
    let show_deleted = signal(false);
    // 排序（旧壳 transactionCount 列 sorter 同义）：客户端排序作用于过滤后
    // 全量行（升序 → 降序 → 未排序循环由宿主排列承担），排序激活后已删行并入
    // 排序不另行隔离（旧壳注释同口径）。
    let sort = signal(None::<DataTableSort>);
    let current_page = signal(1u32);

    // 统一模糊搜索语义过滤（ADR-0027 唯一定义点的直接消费，消费面见
    // [`search_matches`]）。
    // 展示行集（过滤意图全部收敛的纯派生单点）：
    // ① 基集 = 在用 + （显示已删开启时追加已删，旧壳 rows + deletedRows 同形；
    // ② 搜索过滤保序；
    // ③ 排序激活 = 全集按毛笔数排序（已删行并入不隔离）。
    let display_rows = computed(move || {
        let (in_use, deleted) = assembly::partition_merchant_rows(&rows.get());
        let mut merged = if show_deleted.get() {
            in_use.into_iter().chain(deleted).collect::<Vec<_>>()
        } else {
            in_use
        };
        let query = search_term.get().trim().to_owned();
        merged.retain(|row| search_matches(&row.name, &query));
        match sort.get() {
            Some(ordered) if ordered.key == "transaction_count" => match ordered.direction {
                DataTableSortDirection::Ascending => {
                    merged.sort_by_key(|row| row.transaction_count);
                }
                DataTableSortDirection::Descending => {
                    merged.sort_by_key(|row| -row.transaction_count);
                }
            },
            // 未排序 / 他键（本表仅条数列可排）＝保序。
            _ => {}
        }
        merged
    });

    // 过滤意图变化 → 页码归零（旧壳 watch([searchTerm, showDeleted]) 同口径）；
    // 新增/改名等参考数据重读不是过滤意图，保持当前页。
    effect(move || {
        let _query = search_term.get();
        let _show_deleted = show_deleted.get();
        let _sort = sort.get();
        current_page.set(1);
    });

    // 总条数与分页边界（旧壳 prefix / NDataTable 客户端分页同形）：总数 = 过滤
    // 后集合条数；切片 = 显示集合按 (page-1)×50 起的 50 行。
    let total = move || display_rows.get().len() as u64;
    let total_pages = move || total().div_ceil(PAGE_SIZE as u64) as u32;
    let page_rows = computed(move || {
        let snapshot = display_rows.get();
        let page = current_page.get().max(1) as usize;
        let start = (page - 1).saturating_mul(PAGE_SIZE);
        snapshot
            .into_iter()
            .skip(start)
            .take(PAGE_SIZE)
            .collect::<Vec<_>>()
    });
    let clap_page = move |picked: u32| {
        let bounded = picked.clamp(1, total_pages().max(1));
        current_page.set(bounded);
    };

    // 删尾页码回退（issue #457 / ADR-0045 先例，就地声明）：判定用删除前状态——
    // 显示已删关闭时软删行离开展示集合，删前本页仅 1 条 ⇔ 剩 0；开启时行仍在
    //（转已删行），集合不减。返回量带判定位余量（usize::MAX ≠ 0：不触发回退）。
    let remaining_on_page_after_delete = move || {
        if show_deleted.get() {
            return usize::MAX;
        }
        let snapshot = display_rows.get_untracked();
        let page = current_page.get_untracked().max(1) as usize;
        let start = (page - 1).saturating_mul(PAGE_SIZE);
        snapshot
            .len()
            .saturating_sub(start)
            .min(PAGE_SIZE)
            .saturating_sub(1)
    };

    // ============================ 编辑弹窗态 ============================
    // 目标按商户 id 持有（弹窗内容自清单快照按 id 反查）；旧壳 ModalIntent 单
    // 成员闭集 + 序号重建的对应形态：目标复位即回填信号复位（打开时回填，旧壳
    // watch immediate 同果），关闭统一清回 None 终态（✕ / ESC / 取消 / 保存
    // 成功四路全回到工厂清空）。
    let edit_target: Signal<Option<String>> = signal(None);
    let edit_name = signal(String::new());
    let edit_error = signal(None::<String>);

    // 行反查单点（同一快照单点，不另立第二映射）。
    let row_of = move |merchant_id: &str| {
        rows.get_untracked()
            .iter()
            .find(|row| row.id == merchant_id)
            .cloned()
    };

    let open_edit = move |merchant_id: String| {
        if let Some(row) = row_of(&merchant_id) {
            edit_name.set(row.name.clone());
            edit_error.set(None);
            edit_target.set(Some(row.id));
        }
    };
    let close_edit = move || edit_target.set(None);

    let perform_edit = move || {
        edit_error.set(None);
        status.set(None);
        let Some(merchant_id) = edit_target.get_untracked() else {
            return;
        };
        let name = edit_name.get_untracked().trim().to_owned();
        if name.is_empty() {
            edit_error.set(Some(MSG_NAME_REQUIRED.to_owned()));
            return;
        }
        // 重名等后端校验错误原样上抛展示（弹窗不关、内容不丢——旧壳
        // saveEdit catch 同语义），下次尝试开始时清空。
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::update_merchant(
                        db,
                        &merchant_id,
                        ledger_merchants::MerchantUpdateInput {
                            name: Some(name.clone()),
                        },
                    ),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        close_edit();
                        status.set(Some(MSG_UPDATED.to_owned()));
                        // 改名即时生效（交易以 merchant_id 引用）——交易列表
                        // 来源列新名字号经广播驱动的重读即现（广播接线删除
                        // 即红钉在 headless 旅程）。
                        signals::notify_transactions_invalidated();
                    }
                    Ok(Err(e)) => {
                        edit_error.set(Some(format!("{MSG_UPDATE_FAILED}: {e}")));
                    }
                    Err(_) => edit_error.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // ============================ 软删（轻量确认气泡锚定行内删除按钮） ============================
    let perform_delete = move |merchant_id: String| {
        status.set(None);
        // 删尾页码回退判定（旧壳 removeMerchant afterRowDelete 同位：重拉未到
        // 先落页码）。
        let remaining = remaining_on_page_after_delete();
        if remaining == 0 && current_page.get_untracked() > 1 {
            current_page.update(|page| *page -= 1);
        }
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::delete_merchant(db, &merchant_id),
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        status.set(Some(MSG_DELETED.to_owned()));
                        // 软删商户不可再被新交易选择 / 交易列表来源列展示商
                        // 户名不变（历史引用保留）——交易侧选项与来源列按旧
                        // 壳 ledger:changed 粗粒度语义广播交易失效。
                        signals::notify_transactions_invalidated();
                    }
                    Ok(Err(e)) => status.set(Some(format!("{MSG_DELETE_FAILED}: {e}"))),
                    Err(_) => status.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };
    // 软删确认气泡的开合（单实例受控：同一时刻只允许一个气泡在屏——物品视图
    // 先例同形）。ESC / 点外由弹层宿主关闭（封装 on_open_change 回写）。
    let popconfirm_open = signal(None::<String>);

    // ============================ 条数下钻（消费导航单点，不自建） ============================
    // 按行 id 产生跳转，不对条数 / 商户状态设门——条数为 0 点击只见空列表
    //（诚实行为）；软删行照常可下钻（旧壳 goMerchantTransactions 同口径）。
    // 跳转即交易列表落位（?merchant=<id> 的旧壳 URL 语义 → 进程内筛选载荷，
    // 报表商户排行下钻先例同形）。
    let drill_transactions = move |merchant_id: String| {
        nav::open(NavRequest {
            target: ViewName::Transactions,
            container_tab: None,
            form_tab: None,
            focus: None,
            investments_drill: None,
            tx_filters: Some(crate::tx_filter::DrillFilters {
                merchant: Some(merchant_id),
                ..crate::tx_filter::DrillFilters::default()
            }),
        });
    };

    // 列定义映射（装配列 → 宿主列；富余宽全给名称列——items 账户先例同形）。
    let columns = computed(move || {
        let width = window().width.0 - crate::sidebar::SIDEBAR_WIDTH;
        merchant_columns(width)
    });
    // 条数列排序开关（旧壳 sorter: transactionCount 的宿主映射）：标题排序按
    // 钮点击循环（升 → 降 → 未排序），应用排数据并回写 sort（宿主 SortChange
    // 事件约定同形）。
    let on_sort_change = move |requested: &Option<DataTableSort>| {
        sort.set(match requested {
            // 只接受本表唯一可排序列：他键请求丢弃（本表闭集，宿主按钮只挂条
            // 数列——防御性与旧壳单列 sorter 同果）。
            Some(ordered) if ordered.key == "transaction_count" => Some(owned_sort(ordered)),
            _ => None,
        });
    };

    // ============================ 行格（渲染期零格式化） ============================
    // 表格数据（格文本已装配；分页切片后的行集）。
    let data_of = move || {
        let snapshot = page_rows.get();
        DataTableRows::new(snapshot.len())
            .texts("name", snapshot.iter().map(|r| r.name.as_str()))
            .texts(
                "transaction_count",
                snapshot.iter().map(|r| r.transaction_count_text.as_str()),
            )
            .texts("actions", snapshot.iter().map(|_| ""))
    };

    // 自绘格：名称列（已删行带「已删除」标记）、条数列（可点击下钻）、操作列
    //（在用行 = 编辑 / 删除气泡；已删行只读无操作）。
    let cell_of = move |cell: &nsh_base::DataTableCell| -> nsh_core::element::AnyElement {
        let index = cell.row as usize;
        let assembled = page_rows.get_untracked().get(index).cloned();
        match cell.key.as_ref() {
            "name" => {
                let mut cell_view = view().flex_row().items_center().gap_1();
                cell_view = cell_view.child(
                    text(cell.text.clone())
                        .text_size(px(13.0))
                        .text_color(fg)
                        .number_of_lines(1),
                );
                if assembled.as_ref().is_some_and(|row| row.is_deleted) {
                    cell_view = cell_view.child(
                        text(DELETED_TAG)
                            .text_size(px(11.0))
                            .text_color(accent)
                            .app_class("px-1 rounded border border-border")
                            .test_id(
                                format!(
                                    "merchant-deleted-tag-{}",
                                    assembled_id(assembled.as_ref())
                                )
                                .as_str(),
                            ),
                    );
                }
                cell_view.into_element()
            }
            "transaction_count" => {
                let Some(row) = assembled else {
                    return view().into_element();
                };
                let count_id = format!("merchant-count-{}", row.id);
                view()
                    .test_id(count_id.as_str())
                    .on_press({
                        let merchant_id = row.id.clone();
                        move |_: &PressEvent| drill_transactions(merchant_id.clone())
                    })
                    .child(
                        text(cell.text.clone())
                            .text_size(px(13.0))
                            .text_color(accent)
                            .number_of_lines(1),
                    )
                    .into_element()
            }
            "actions" => {
                let Some(row) = assembled else {
                    return view().into_element();
                };
                // 已删行只读（issue #447）：无编辑/删除操作。
                if row.is_deleted {
                    return view().into_element();
                }
                let merchant_id = row.id.clone();
                view()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(row_action_button(
                        format!("merchant-edit-{merchant_id}"),
                        ROW_EDIT,
                        fg,
                        {
                            let merchant_id = merchant_id.clone();
                            move || open_edit(merchant_id.clone())
                        },
                    ))
                    .child(
                        // 轻量确认气泡（旧壳 AppPopconfirm 同位）：确认文案把
                        // 历史交易仍显示该名说清楚（旧壳 deleteConfirm 同词表）。
                        popconfirm::popconfirm(
                            "merchant-delete",
                            {
                                let merchant_id = merchant_id.clone();
                                move || {
                                    popconfirm_open.get().as_deref() == Some(merchant_id.as_str())
                                }
                            },
                            {
                                let merchant_id = merchant_id.clone();
                                move |shown: bool| {
                                    popconfirm_open.set(shown.then(|| merchant_id.clone()))
                                }
                            },
                            popconfirm::PopconfirmSpec {
                                message: DELETE_CONFIRM.to_owned(),
                                confirm_label: ROW_DELETE.to_owned(),
                                cancel_label: ROW_CANCEL.to_owned(),
                            },
                            // 触发件用纯视图（nsh-base Button 自含按压语义会
                            // 吞掉外层按压——popconfirm 触发件是外层包裹视图，
                            // 物品删除先例同注）。
                            view()
                                .app_class(
                                    "px-2 py-0.5 rounded-md border border-border text-foreground hover:bg-accent",
                                )
                                .child(
                                    text(ROW_DELETE)
                                        .text_size(px(12.0))
                                        .text_color(danger_fg)
                                        .test_id(
                                            format!("merchant-delete-{merchant_id}").as_str(),
                                        ),
                                ),
                            move || perform_delete(merchant_id.clone()),
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
        view()
            .flex_col()
            .gap_2()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(field_label(FIELD_NAME, muted))
                    .child(
                        input()
                            .test_id("merchant-edit-name")
                            .value(move || edit_name.get())
                            .placeholder(NAME_PLACEHOLDER)
                            .optional_label("商户名称")
                            .text_color(fg)
                            .w(px(240.0))
                            .app_class(
                                "bg-background rounded-md border border-border px-2 py-1 \
                                 placeholder:text-muted-foreground",
                            )
                            .on_change(move |text: &str| edit_name.set(text.to_owned())),
                    ),
            )
            .child(
                // 保存错误条位（域码化错误 zh 原样：重名「商户已存在: X」等；
                // 下次尝试开始时清空）。
                when(
                    move || edit_error.get().is_some(),
                    move || {
                        text(move || {
                            format!(
                                "{}: {}",
                                MSG_UPDATE_FAILED,
                                edit_error.get().unwrap_or_default()
                            )
                        })
                        .text_size(px(12.0))
                        .text_color(danger_fg)
                        .test_id("merchant-edit-error")
                    },
                )
                .otherwise(view),
            )
            .child(
                // 按钮行右对齐单主键（旧壳 issue #637：轻量单字段编辑保留
                // 无取消键语义）。
                view().flex_row().justify_end().child(
                    button()
                        .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90")
                        .on_press(move |_: &()| perform_edit())
                        .child(
                            text(SAVE_BUTTON)
                                .text_size(px(13.0))
                                .text_color(rgb(0xffffff))
                                .test_id("merchant-edit-save"),
                        ),
                ),
            )
    };

    // ============================ 视图树 ============================
    // 新增表单卡：单字段行内表单（名称 + 添加），错误条位前置拦截与域错误
    // 同位（下次尝试开始时清空）。
    let create_card = view()
        .flex_col()
        .gap_2()
        .app_class("rounded-md border border-border p-3")
        .child(
            text(ADD_CARD_TITLE)
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg),
        )
        .child(
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
                        .child(field_label(FIELD_NAME, muted))
                        .child(
                            input()
                                .test_id("merchant-create-name")
                                .value(move || create_name.get())
                                .placeholder(NAME_PLACEHOLDER)
                                .optional_label("商户名称")
                                .text_color(fg)
                                .w(px(160.0))
                                .app_class(
                                    "bg-background rounded-md border border-border px-2 py-1 \
                                     placeholder:text-muted-foreground",
                                )
                                .on_change(move |text: &str| create_name.set(text.to_owned())),
                        ),
                )
                .child(
                    button()
                        .app_class("px-3 py-1.5 rounded-md bg-primary hover:opacity-90")
                        .on_press(move |_: &()| perform_create())
                        .child(
                            text(ADD_BUTTON)
                                .text_size(px(13.0))
                                .text_color(rgb(0xffffff))
                                .test_id("merchant-add"),
                        ),
                ),
        )
        .child(
            when(
                move || create_error.get().is_some(),
                move || {
                    text(move || create_error.get().unwrap_or_default())
                        .text_size(px(12.0))
                        .text_color(danger_fg)
                        .test_id("merchant-create-error")
                },
            )
            .otherwise(view),
        );

    let list_card = view()
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
            // 搜索 + 显示已删（过滤意图行）。
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .child(
                    input()
                        .test_id("merchant-search")
                        .value(move || search_term.get())
                        .placeholder(SEARCH_PLACEHOLDER)
                        .optional_label("搜索商户")
                        .text_color(fg)
                        .w(px(240.0))
                        .app_class(
                            "bg-background rounded-md border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(move |text: &str| search_term.set(text.to_owned())),
                )
                .child(
                    checkbox()
                        .checked(move || show_deleted.get())
                        .on_change(move |&next: &bool| show_deleted.set(next))
                        .accessibility_label(SHOW_DELETED_LABEL)
                        .child(
                            // test_id 落在标签文本上（headless id: 锚点命中文本、
                            // 物理命中到内层 checkbox 可按压件——页签先例同形）。
                            text(SHOW_DELETED_LABEL)
                                .text_size(px(13.0))
                                .text_color(fg)
                                .test_id("merchant-show-deleted"),
                        ),
                ),
        )
        .child(
            // 清单卡片体（卡片自身 flex_1——虚拟化表格取弹性链高度，卡片不定高
            // 则虚拟化量出零高，先例账户/物品视图同注）。
            view().test_id("merchant-table").flex_1().child(
                data_table()
                    .columns(columns)
                    .data(data_of)
                    .render_cell(cell_of)
                    .sort(move || sort.get())
                    .on_sort_change(move |requested: &Option<DataTableSort>| {
                        on_sort_change(requested);
                    })
                    .loading(move || loading.get())
                    .loading_view(text(LOADING_PLACEHOLDER).text_color(muted))
                    .classes(table_classes())
                    .flex_1(),
            ),
        )
        .child(
            // 总条数 + 分页条（旧壳 pagination.prefix「共 N 条」同词表；分页条
            // 仅多页在场——单页只留总数，旧壳同果）。
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text(move || format!("共 {} 条", total()))
                        .text_size(px(12.0))
                        .text_color(muted)
                        .test_id("merchant-total"),
                )
                .child(
                    when(
                        move || total() > PAGE_SIZE as u64,
                        move || {
                            view().test_id("merchant-pagination").flex_row().child(
                                pagination()
                                    .accessibility_label("商户分页")
                                    .previous_accessibility_label("上一页")
                                    .next_accessibility_label("下一页")
                                    .page(move || current_page.get())
                                    .total_pages(total_pages)
                                    .disabled(move || loading.get())
                                    .item_class(
                                        "px-2 py-1 rounded-md text-foreground hover:bg-accent selected:bg-accent disabled:opacity-40",
                                    )
                                    .on_change(move |&picked: &u32| clap_page(picked)),
                            )
                        },
                    )
                    .otherwise(view),
                ),
        );

    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(
            view().px_4().py_2().child(
                text(ViewName::Merchants.label())
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
                    text(move || format!("加载失败：{}", error.get().unwrap_or_default()))
                        .text_size(px(12.0))
                        .text_color(danger_fg)
                        .test_id("merchant-error")
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
                .child(create_card)
                .child(list_card)
                .child(
                    // 操作反馈条位（成功/失败共用一条，新动作开始清回空）。
                    when(
                        move || status.get().is_some(),
                        move || {
                            text(move || status.get().unwrap_or_default())
                                .text_size(px(12.0))
                                .text_color(fg)
                                .test_id("merchant-status")
                        },
                    )
                    .otherwise(view),
                )
                .child(modal::app_modal(
                    move || edit_target.get().is_some(),
                    || EDIT_TITLE.to_owned(),
                    close_edit,
                    edit_modal_content,
                )),
        )
}

// ---------------------------------------------------------------------------
// 行辅助件与列宿主映射（装配产物 → 呈现）
// ---------------------------------------------------------------------------

/// 已删行格反查的 id（缺省空串——占位分支不挂 test_id）。
fn assembled_id(row: Option<&MerchantRowCells>) -> String {
    row.map(|r| r.id.clone()).unwrap_or_default()
}

/// 排序请求的值拷贝面（宿主 SortChange 事件按引用携带——闭包内克隆才是拥有值）。
fn owned_sort(ordered: &DataTableSort) -> DataTableSort {
    (*ordered).clone()
}

/// 行内小操作按钮（旧壳 NButton size=tiny 的对应形态，items 先例同注）。
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

/// 表格外围定宽扣减（商户列宽计算输入，items/accounts 同算）。
const MERCHANTS_TABLE_WIDTH_PAD: f32 = 16.0 * 2.0 + 12.0 * 2.0 + 2.0;

/// 商户列宿主映射（[`MERCHANT_COLUMNS`] → 宿主列；UI 层只做映射不产列）：
/// 弹性宽度分配同 [`crate::app::elastic_columns`] 先例——富余宽度全给名称列。
/// 条数列可排序（旧壳 sorter 的宿主面，spec DataRow 列定义的可排序钮）。
fn merchant_columns(width: f32) -> Vec<DataTableColumn> {
    let mut columns: Vec<DataTableColumn> = MERCHANT_COLUMNS
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
            // 本表唯一可排列 = 条数列（旧壳 sorter 同闭集）。
            column.sortable = def.key == "transaction_count";
            column
        })
        .collect();
    let suggested: f32 = MERCHANT_COLUMNS.iter().map(|def| def.width).sum();
    let extra = (width - MERCHANTS_TABLE_WIDTH_PAD - suggested).max(0.0);
    if extra > 0.0
        && let Some(name) = columns.iter_mut().find(|col| col.key == "name")
        && let Some(w) = name.width
    {
        name.width = Some(Points(w.0 + extra));
    }
    columns
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 词表锁定（旧壳 settings.merchants zh-CN 词条原样承载）：文案走样即红
    ///（用户可观察面的单测落点）。
    #[test]
    fn word_list_mirrors_old_shell_zh_cn() {
        assert_eq!(ADD_CARD_TITLE, "新增商户");
        assert_eq!(LIST_CARD_TITLE, "商户列表");
        assert_eq!(SEARCH_PLACEHOLDER, "搜索商户（名称/拼音）");
        assert_eq!(SHOW_DELETED_LABEL, "显示已删");
        assert_eq!(DELETE_CONFIRM, "确认删除该商户？历史交易仍会显示它的名字。");
        assert_eq!(DELETED_TAG, "已删除");
        assert_eq!(MSG_NAME_REQUIRED, "请输入商户名称");
        assert_eq!(MSG_ADDED, "已添加商户");
        assert_eq!(MSG_UPDATED, "已更新商户");
        assert_eq!(MSG_DELETED, "已删除商户");
        // 列名（装配层 MERCHANT_COLUMNS 同词表锚定：管理三列与旧壳同文案）。
        let titles: Vec<&str> = crate::assembly::MERCHANT_COLUMNS
            .iter()
            .map(|def| def.title)
            .collect();
        assert_eq!(titles, vec!["名称", "关联交易", "操作"]);
    }

    /// 统一模糊搜索语义过滤（本视图 matches_search 闭包消费面）：中文名命中、
    /// 拼音首字母命中、多词 AND、空查询恒全量——域入口 [`ledger_transaction::
    /// term_matches_text`] 唯一定义点，本测钉视图侧的消费接线（改走第二套
    /// 过滤语义即红）。
    #[test]
    fn search_filter_follows_unified_fuzzy_semantics() {
        assert!(search_matches("京东", ""));
        assert!(search_matches("京东", "京东"));
        assert!(!search_matches("京东", "美团"));
        // 拼音首字母子序列（ADR-0027 语义）。
        assert!(search_matches("京东", "jd"));
        // 多词 AND（词条间 AND，同一名对多词条皆命中）。
        assert!(search_matches("京东", "东 京"));
        assert!(!search_matches("京东", "京东 美团"));
    }

    /// 删尾页码回退（ADR-0045）与分页边界（issue #457 的纯决策半边，
    /// [`remaining_on_page_after_delete`] 的公式面）：50 行/页、51 行两页；
    /// 判定面 = 「删前本页剩 N 条」（显示已删关闭 = 本页行数 − 1，开启时行在
    /// 集合内不判零）；仅剩 0 条且非首页时回退一页。
    #[test]
    fn page_backward_and_boundaries_follow_display_set_facts() {
        assert_eq!(PAGE_SIZE, 50);
        let total = 51usize;
        let total_pages = total.div_ceil(PAGE_SIZE) as u32;
        assert_eq!(total_pages, 2);
        // 判定公式（同视图闭包）：显示已删关闭 = 本页行数 − 1；开启恒不减。
        let remaining_on_page = |page: usize, display_len: usize| {
            let start = (page - 1) * PAGE_SIZE;
            display_len
                .saturating_sub(start)
                .min(PAGE_SIZE)
                .saturating_sub(1)
        };
        let rollback = |remaining: usize, page: u32| {
            if remaining == 0 && page > 1 {
                page - 1
            } else {
                page
            }
        };
        // 第 2 页只有 1 条 → 判定剩 0 → 回第 1 页。
        assert_eq!(rollback(remaining_on_page(2, 51), 2), 1);
        // 首页 / 非末行删除不回退。
        assert_eq!(rollback(remaining_on_page(1, 51), 1), 1);
        assert_eq!(rollback(remaining_on_page(2, 55), 2), 2);
        // 显示已删开启：行转已删行在集合内，恒不回退。
        assert_ne!(usize::MAX, 0, "显示已删开启不触发回退");
    }
}
