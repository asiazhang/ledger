//! 分类管理卡（issue #2136 设置③；旧壳 `CategoryManager.vue` +
//! `CategoryKindPanel` + `CategoryTree` + `CategoryEditModal` 的重生，ADR-0022
//! 分类页签）。
//!
//! **结构**（旧壳同构）：支出 / 收入两类页签（CategoryKindPanel 两块）；每类 =
//! 新增表单卡（名称 + 父分类〔拼音可搜下拉，选项 = 同类根分类，默认「无（顶
//! 级）」〕 + 图标 + 添加）+ 分类清单卡（两级树行：根行 + 缩进子行，行级编辑 /
//! 删除 / 上移 / 下移）。写路径直调域公开写入口（[`crate::boot`] 接缝），成功
//! 后广播交易失效信号（交易列表分类列 / 表单分类下拉——旧壳 ledger:changed
//! 参考失效信号的壳内对应物，商户字典先例 #2052 同形）。
//!
//! **交互平价与 NSH 呈现差异**：
//! - 编辑弹窗（旧壳 CategoryEditModal）：名称 / 图标 / 父分类三字段 + 保存；
//!   空名前置拦截，域校验错误（两级分类 / 不存在）原样在弹窗内呈现不关闭。
//! - 软删轻量确认气泡（旧壳 AppPopconfirm 同形态，确认文案「确认删除？」）；
//!   预算删除守卫的码化错误落卡内状态条位。
//! - 拖拽排序的 NSH 对应形态：旧壳 NTree 拖拽改序（桌面专属交互）在 NSH 无
//!   拖拽宿主面，代之以行级「上移 / 下移」按钮写同一域入口
//!   （`reorder_categories` 全集覆盖语义，仅同父兄弟序列参与）——重排能力
//!   平价，交互形态差异留痕本注（呈现差异入交付报告对表）。
//! - 图标字段：旧壳 NInput 输入图标名（icon 组件集按名解析），NSH 同口径
//!   文本输入承载（图标名原样存储，树行暂不渲染图标面——旧壳 renderPrefix
//!   的图标视觉面为呈现层细节，能力面 = 图标名可编辑可存储）。
//!
//! 界面文案仅 zh-CN（旧壳 settings.categories.* zh-CN 词条原样承载）；码化
//! 错误 zh 原样呈现；视图本地快照，离开设置页再回 = 全新装载（旧壳重挂同
//! 口径）；阻塞 IO 经 `blocking` 后台执行。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_base::input;
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::app::db_slot;
use crate::assembly::CategoryOption;
use crate::boot;
use crate::design::font_size;
use crate::modal::app_modal;
use crate::pinyin_select;
use crate::popconfirm;
use crate::settings::status_line;
use crate::shell_msgs::{MSG_READ_TASK_FAILED, MSG_WRITE_TASK_FAILED};
use crate::write_load::session_missing_err;
use crate::{signals, views};

// ---------------------------------------------------------------------------
// 词表（旧壳 settings.categories.* zh-CN 词条原样；i18n 框架级方案不在本票，
// 字面承载）。
// ---------------------------------------------------------------------------
const KIND_EXPENSE: &str = "expense";
const KIND_INCOME: &str = "income";
const TAB_KIND_EXPENSE: &str = "支出";
const TAB_KIND_INCOME: &str = "收入";
const CARD_ADD: &str = "新增分类";
const CARD_LIST: &str = "分类列表";
const FIELD_NAME: &str = "名称";
const NAME_PLACEHOLDER: &str = "分类名称";
const FIELD_PARENT: &str = "父分类";
const PARENT_NONE: &str = "无（顶级）";
const FIELD_ICON: &str = "图标";
const ICON_PLACEHOLDER: &str = "图标名";
const BUTTON_ADD: &str = "添加";
const BUTTON_SAVE: &str = "保存";
const ROW_EDIT: &str = "编辑";
const ROW_DELETE: &str = "删除";
const ROW_UP: &str = "上移";
const ROW_DOWN: &str = "下移";
const EDIT_MODAL_TITLE: &str = "编辑分类";
const DELETE_CONFIRM: &str = "确认删除？";
const MSG_NAME_REQUIRED: &str = "请输入分类名称";
const MSG_ADDED: &str = "已添加分类";
const MSG_UPDATED: &str = "已更新分类";
const MSG_DELETED: &str = "已删除";
const MSG_ADD_FAILED_PREFIX: &str = "添加失败: ";
const MSG_UPDATE_FAILED_PREFIX: &str = "更新失败: ";
const MSG_DELETE_FAILED_PREFIX: &str = "删除失败: ";
const MSG_SORT_FAILED_PREFIX: &str = "排序失败: ";
const LOADING_PLACEHOLDER: &str = "正在加载…";
const LOAD_FAILED_PREFIX: &str = "加载失败：";

// ---------------------------------------------------------------------------
// 装配层（纯投影，无 UI 依赖；渲染期零格式化）。
// ---------------------------------------------------------------------------

/// 分类条目（域行的视图侧投影：信号集合要求项可比较，域实体无 PartialEq——
/// 装配层投影既定形态，商户 BackupRowView 同款先例）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CategoryItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub icon: Option<String>,
}

/// 域行集 → 条目投影（保序，字段逐一搬运）。
pub(crate) fn category_items(rows: Vec<ledger_categories::Category>) -> Vec<CategoryItem> {
    rows.into_iter()
        .map(|c| CategoryItem {
            id: c.id,
            name: c.name,
            kind: c.kind,
            parent_id: c.parent_id,
            icon: c.icon,
        })
        .collect()
}

/// 分类行视图（树行装配产物：缩进层级 + 同父兄弟定位一并在此算清，渲染期
/// 零派生）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CategoryRowView {
    /// 分类 id（行锚点 / 写目标）。
    pub id: String,
    /// 显示名（根行 = 自身名；子行 = 「父 > 子」路径名——旧壳路径口径
    /// [`crate::assembly::CategoryTable::path_of`] 同源）。
    pub label: String,
    /// 树层级（0 = 根行，1 = 子行；分类实体两级为设计形态）。
    pub depth: usize,
    /// 在同父兄弟序列中的位次（上移 / 下移可用性判定消费面）。
    pub sibling_index: usize,
    /// 同父兄弟总数。
    pub sibling_count: usize,
}

/// 在用分类行集 → 树行视图投影（域读序 = kind, sort_order, created_at；本
/// 投影保序切 kind、根行保序、子行随父）。同一行集一份投影，无第二份口径。
pub(crate) fn category_row_views(rows: &[CategoryItem], kind: &str) -> Vec<CategoryRowView> {
    let mut views = Vec::new();
    let roots: Vec<&CategoryItem> = rows
        .iter()
        .filter(|c| c.kind == kind && c.parent_id.is_none())
        .collect();
    let root_count = roots.len();
    for (root_index, root) in roots.iter().enumerate() {
        views.push(CategoryRowView {
            id: root.id.clone(),
            label: root.name.clone(),
            depth: 0,
            sibling_index: root_index,
            sibling_count: root_count,
        });
        let children: Vec<&CategoryItem> = rows
            .iter()
            .filter(|c| c.kind == kind && c.parent_id.as_deref() == Some(root.id.as_str()))
            .collect();
        let count = children.len();
        for (index, child) in children.iter().enumerate() {
            views.push(CategoryRowView {
                id: child.id.clone(),
                label: format!("{} > {}", root.name, child.name),
                depth: 1,
                sibling_index: index,
                sibling_count: count,
            });
        }
    }
    views
}

/// 同类根分类选项（新增 / 编辑弹窗的父分类下拉数据源；首项 = 「无（顶级）」
/// 哨兵空 id，旧壳 parentNone 同词表）。`exclude` = 编辑弹窗排除自身。
pub(crate) fn parent_options(
    rows: &[CategoryItem],
    kind: &str,
    exclude: Option<&str>,
) -> Vec<CategoryOption> {
    let mut options = vec![CategoryOption {
        id: String::new(),
        label: PARENT_NONE.to_owned(),
    }];
    options.extend(
        rows.iter()
            .filter(|c| c.kind == kind && c.parent_id.is_none() && Some(c.id.as_str()) != exclude)
            .map(|c| CategoryOption {
                id: c.id.clone(),
                label: c.name.clone(),
            }),
    );
    options
}

/// 同父兄弟重排的提交项（上移 / 下移按钮的纯决策半边）：把 `id` 在兄弟序列中
/// 移动 `delta` 位（−1 上移 / +1 下移），返回全集落定项（0..n 重编
/// `sort_order`）。越界（已到头）回 None 不发起写。矩阵化用例见 lib tests。
pub(crate) fn reorder_items_for_move(
    siblings: &[&CategoryItem],
    id: &str,
    delta: i32,
) -> Option<Vec<ledger_categories::ReorderItem>> {
    let from = siblings.iter().position(|c| c.id == id)?;
    let to = from as i64 + delta as i64;
    if to < 0 || to >= siblings.len() as i64 {
        return None;
    }
    let to = to as usize;
    let mut ordered: Vec<String> = siblings.iter().map(|c| c.id.clone()).collect();
    ordered.remove(from);
    ordered.insert(to, id.to_owned());
    Some(
        ordered
            .into_iter()
            .enumerate()
            .map(|(sort_order, id)| ledger_categories::ReorderItem {
                id,
                sort_order: sort_order as i64,
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// 视图（设置页「分类」页签消费面）。
// ---------------------------------------------------------------------------

/// 分类页签内容（旧壳 CategoryManager 同构：两类页签 + 各类卡组）。
pub(crate) fn categories_pane(palette: views::ViewPalette) -> View {
    let owner = Owner::current();
    let kind: Signal<&'static str> = signal(KIND_EXPENSE);
    // 软删确认气泡开合（单实例受控：同一时刻只允许一个气泡在屏——商户字典
    // 先例同形；ESC / 点外由弹层宿主关闭）。
    let popconfirm_open: Signal<Option<String>> = signal(None);
    // 行集快照（两类共用一份全量，切页签零重读——旧壳 reference store 同口径）。
    let rows = signal(Vec::<CategoryItem>::new());
    let loading = signal(true);
    let error = signal(None::<String>);
    let status = signal(None::<String>);

    // 装载（读接缝单点；失败诚实呈现不静默假空态）。删装载接线 → 清单永远
    // 空 → headless 落位旅程红。
    let reload = {
        let (rows, loading, error) = (rows, loading, error);
        move || {
            loading.set(true);
            error.set(None);
            let db = db_slot().clone();
            owner.run(move || {
                spawn(async move {
                    let outcome = blocking(move || match db.as_ref() {
                        Some(db) => boot::category_rows(db),
                        None => Err(session_missing_err()),
                    })
                    .await;
                    match outcome {
                        Ok(Ok(rows_data)) => {
                            rows.set(category_items(rows_data));
                            error.set(None);
                        }
                        Ok(Err(e)) => error.set(Some(format!("{LOAD_FAILED_PREFIX}{e}"))),
                        Err(_) => error.set(Some(MSG_READ_TASK_FAILED.to_owned())),
                    }
                    loading.set(false);
                });
            });
        }
    };
    reload();

    // 写路径共用出口：成功 → 状态反馈 + 交易失效广播 + 清单重读（分类名进
    // 交易列表分类列 / 表单分类下拉——广播接线删除即红，headless 旅程钉）。
    let on_written = {
        let (status, reload) = (status, reload);
        move |message: &'static str| {
            status.set(Some(message.to_owned()));
            signals::notify_transactions_invalidated();
            reload();
        }
    };
    let on_write_error = move |prefix: &'static str, e: ledger_infra::error::AppError| {
        status.set(Some(format!("{prefix}{e}")));
    };

    // 新增表单（旧壳 CategoryAddForm 同字段闭集）。
    let create_name = signal(String::new());
    let create_parent = signal(None::<String>);
    let create_icon = signal(String::new());
    let create_error = signal(None::<String>);
    let create_kind = kind;
    let perform_create = move || {
        create_error.set(None);
        status.set(None);
        let name = create_name.get().trim().to_owned();
        if name.is_empty() {
            create_error.set(Some(MSG_NAME_REQUIRED.to_owned()));
            return;
        }
        let kind = create_kind.get_untracked().to_owned();
        let parent_id = create_parent.get_untracked().filter(|p| !p.is_empty());
        let icon = create_icon.get_untracked().trim().to_owned();
        let icon = (!icon.is_empty()).then_some(icon);
        let db = db_slot().clone();
        owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::create_category(
                        db,
                        ledger_categories::CategoryInput {
                            name: name.clone(),
                            kind: kind.clone(),
                            parent_id,
                            icon,
                        },
                    ),
                    None => Err(session_missing_err()),
                })
                .await;
                match outcome {
                    Ok(Ok(_)) => {
                        create_name.set(String::new());
                        create_parent.set(None);
                        create_icon.set(String::new());
                        on_written(MSG_ADDED);
                    }
                    Ok(Err(e)) => {
                        create_error.set(Some(format!("{MSG_ADD_FAILED_PREFIX}{e}")));
                    }
                    Err(_) => create_error.set(Some(MSG_WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // 编辑弹窗态（目标按 id 持有；打开时回填——旧壳 watch immediate 同果，
    // 关闭统一清回 None 终态）。
    let edit_target: Signal<Option<String>> = signal(None);
    let edit_name = signal(String::new());
    let edit_icon = signal(String::new());
    let edit_parent = signal(None::<String>);
    let edit_error = signal(None::<String>);
    let open_edit = move |id: String| {
        if let Some(cat) = rows.get_untracked().iter().find(|c| c.id == id).cloned() {
            edit_name.set(cat.name);
            edit_icon.set(cat.icon.unwrap_or_default());
            edit_parent.set(cat.parent_id);
            edit_error.set(None);
            edit_target.set(Some(cat.id));
        }
    };
    let close_edit = move || edit_target.set(None);
    let perform_edit = move || {
        edit_error.set(None);
        status.set(None);
        let Some(id) = edit_target.get_untracked() else {
            return;
        };
        let name = edit_name.get_untracked().trim().to_owned();
        if name.is_empty() {
            edit_error.set(Some(MSG_NAME_REQUIRED.to_owned()));
            return;
        }
        // 三态入参（域 double-option 形态）：图标 / 父分类都落定值——「无」
        // 即清空（编辑弹窗选「无（顶级）」提升为顶级，#1327 语义）。
        let icon = Some(Some(edit_icon.get_untracked().trim().to_owned()));
        let parent_id = Some(edit_parent.get_untracked().filter(|p| !p.is_empty()));
        let db = db_slot().clone();
        owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::update_category(
                        db,
                        &id,
                        ledger_categories::CategoryUpdateInput {
                            name: Some(name.clone()),
                            icon,
                            parent_id,
                        },
                    ),
                    None => Err(session_missing_err()),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        close_edit();
                        on_written(MSG_UPDATED);
                    }
                    Ok(Err(e)) => {
                        edit_error.set(Some(format!("{MSG_UPDATE_FAILED_PREFIX}{e}")));
                    }
                    Err(_) => edit_error.set(Some(MSG_WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // 软删（轻量确认气泡锚定行内删除按钮；预算删除守卫错误落状态条位）。
    let perform_delete = move |id: String| {
        status.set(None);
        let db = db_slot().clone();
        owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::delete_category(db, &id),
                    None => Err(session_missing_err()),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => on_written(MSG_DELETED),
                    Ok(Err(e)) => on_write_error(MSG_DELETE_FAILED_PREFIX, e),
                    Err(_) => on_write_error(
                        MSG_DELETE_FAILED_PREFIX,
                        ledger_infra::error::AppError::Db(MSG_WRITE_TASK_FAILED.to_owned()),
                    ),
                }
            });
        });
    };

    // 上移 / 下移（同父兄弟序列重排；越界不动作）。删执行接线 → 行序永不
    // 变 → headless 重排旅程红。
    let perform_move = move |id: String, delta: i32| {
        status.set(None);
        let kind = kind.get_untracked();
        let snapshot = rows.get_untracked();
        let siblings: Vec<&CategoryItem> = snapshot
            .iter()
            .filter(|c| c.kind == kind)
            .filter(|c| match snapshot.iter().find(|p| p.id == id) {
                Some(target) => c.parent_id == target.parent_id,
                None => false,
            })
            .collect();
        let Some(items) = reorder_items_for_move(&siblings, &id, delta) else {
            return;
        };
        let db = db_slot().clone();
        owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::reorder_categories(db, items),
                    None => Err(session_missing_err()),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => on_written(MSG_UPDATED),
                    Ok(Err(e)) => on_write_error(MSG_SORT_FAILED_PREFIX, e),
                    Err(_) => on_write_error(
                        MSG_SORT_FAILED_PREFIX,
                        ledger_infra::error::AppError::Db(MSG_WRITE_TASK_FAILED.to_owned()),
                    ),
                }
            });
        });
    };

    // 树行行集（切页签重新投影；渲染期零派生——行文本已是装配产物）。
    let display_rows = computed(move || category_row_views(&rows.get(), kind.get()));

    // 两类页签（旧壳 CategoryManager NTabs 同形态）。
    let kind_bar = view().app_class("border-b border-border").child(
        nsh_base::tabs()
            .indicator_class("bg-accent")
            .child(view().flex_row().gap_2().child(each(
                move || [KIND_EXPENSE, KIND_INCOME],
                |k: &&'static str| (*k).to_owned(),
                move |item: ReadSignal<&'static str>, _index: ReadSignal<usize>| {
                    let k = item.get();
                    let label = if k == KIND_EXPENSE {
                        TAB_KIND_EXPENSE
                    } else {
                        TAB_KIND_INCOME
                    };
                    let is_active = move || kind.get() == k;
                    nsh_base::tab()
                        .selected(is_active)
                        .accessibility_label(label)
                        .app_class("px-2 py-1 rounded-(--radius) selected:bg-accent")
                        .child(
                            text(label)
                                .text_size(px(font_size::SECONDARY))
                                .text_color(move || {
                                    if is_active() {
                                        palette.fg
                                    } else {
                                        palette.muted
                                    }
                                })
                                .test_id(if k == KIND_EXPENSE {
                                    "cat-tab-expense"
                                } else {
                                    "cat-tab-income"
                                }),
                        )
                        .on_press(move |_: &()| kind.set(k))
                },
            ))),
    );

    // 新增卡（当前页签 kind 的表单；父分类下拉选项随 kind 派生）。
    let add_card = view()
        .flex_col()
        .app_class("rounded-(--radius-card) border border-border p-3 gap-2")
        .child(
            text(CARD_ADD)
                .text_size(px(font_size::SECONDARY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.fg),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .flex_wrap()
                .child(
                    view().flex_col().app_class("gap-1").child(
                        text(FIELD_NAME)
                            .text_size(px(font_size::CAPTION))
                            .text_color(palette.fg),
                    ),
                )
                .child(
                    input()
                        .test_id("cat-create-name")
                        .value(move || create_name.get())
                        .placeholder(NAME_PLACEHOLDER)
                        .optional_label(NAME_PLACEHOLDER)
                        .text_color(palette.fg)
                        .w(px(160.0))
                        .app_class(
                            "bg-background rounded-(--radius) border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(move |value: &str| create_name.set(value.to_owned())),
                )
                .child(
                    view().flex_col().app_class("gap-1").child(
                        text(FIELD_PARENT)
                            .text_size(px(font_size::CAPTION))
                            .text_color(palette.fg),
                    ),
                )
                .child(pinyin_select::pinyin_select(
                    "cat-create-parent",
                    move || parent_options(&rows.get_untracked(), kind.get_untracked(), None),
                    create_parent,
                    PARENT_NONE,
                    true,
                ))
                .child(
                    view().flex_col().app_class("gap-1").child(
                        text(FIELD_ICON)
                            .text_size(px(font_size::CAPTION))
                            .text_color(palette.fg),
                    ),
                )
                .child(
                    input()
                        .test_id("cat-create-icon")
                        .value(move || create_icon.get())
                        .placeholder(ICON_PLACEHOLDER)
                        .optional_label(ICON_PLACEHOLDER)
                        .text_color(palette.fg)
                        .w(px(120.0))
                        .app_class(
                            "bg-background rounded-(--radius) border border-border px-2 py-1 \
                             placeholder:text-muted-foreground",
                        )
                        .on_change(move |value: &str| create_icon.set(value.to_owned())),
                )
                .child(
                    nsh_base::button()
                        .app_class("px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90")
                        .on_press(move |_: &()| perform_create())
                        .child(
                            text(BUTTON_ADD)
                                .text_size(px(font_size::SECONDARY))
                                .text_color(palette.primary_fg)
                                .test_id("cat-add"),
                        ),
                ),
        )
        .child(
            when(
                move || create_error.get().is_some(),
                move || {
                    text(move || create_error.get().unwrap_or_default())
                        .text_size(px(font_size::CAPTION))
                        .text_color(palette.danger_fg)
                        .test_id("cat-create-error")
                },
            )
            .otherwise(view),
        );

    // 清单卡（树行行集；行内编辑 / 删除气泡 / 上移 / 下移）。链式一次构
    // 成：装载态区域 → 树行 each → 状态条位（子件顺序一次定齐，不二次追加）。
    let list_card = view()
        .flex_1()
        .min_h(px(0.0))
        .flex_col()
        .app_class("rounded-(--radius-card) border border-border p-3 gap-2")
        .child(
            text(CARD_LIST)
                .text_size(px(font_size::SECONDARY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.fg),
        )
        .child(
            when(
                move || loading.get(),
                move || {
                    text(LOADING_PLACEHOLDER)
                        .text_size(px(font_size::SECONDARY))
                        .text_color(palette.muted)
                        .test_id("cat-loading")
                },
            )
            .otherwise(view),
        )
        .child(status_line("cat-status", status, palette))
        .child(
            // 树行滚动容器（长清单裁出视口的行不进文档——headless 用例经
            // scroll 锚点滚入视野；交易列表滚动先例同形）。
            scroll()
                .flex_1()
                .min_h(px(0.0))
                .test_id("cat-list-scroll")
                .child(each(
                    move || display_rows.get(),
                    // 行键 = id + 路径名：改名（同 id）换键即重建行——行视图是
                    // 构建期快照（锚点 test_id 静态），同键就地补丁不重跑行闭
                    // 包，新名不达（each 补丁语义）。
                    |row: &CategoryRowView| format!("{}:{}", row.id, row.label),
            move |item: ReadSignal<CategoryRowView>, _index: ReadSignal<usize>| {
                let row = item.get();
                let row_id = row.id.clone();
                view()
                    .flex_row()
                    .app_class("items-center gap-2 py-0.5")
                    .app_class(if row.depth > 0 { "pl-6" } else { "" })
                    .test_id(format!("cat-row-{row_id}").as_str())
                    .child(
                        text(row.label.clone())
                            .text_size(px(font_size::SECONDARY))
                            .text_color(palette.fg)
                            .number_of_lines(1),
                    )
                    .child(
                        nsh_base::button()
                            .app_class("px-2 py-0.5 rounded-sm border border-border text-foreground hover:bg-accent")
                            .on_press({
                                let row_id = row_id.clone();
                                move |_: &()| open_edit(row_id.clone())
                            })
                            .child(
                                text(ROW_EDIT)
                                    .text_size(px(font_size::CAPTION))
                                    .text_color(palette.fg)
                                    .test_id(format!("cat-edit-{row_id}").as_str()),
                            ),
                    )
                    .child(popconfirm::popconfirm(
                        "cat-delete",
                        {
                            let row_id = row_id.clone();
                            move || popconfirm_open.get().as_deref() == Some(row_id.as_str())
                        },
                        {
                            let row_id = row_id.clone();
                            move |shown: bool| popconfirm_open.set(shown.then(|| row_id.clone()))
                        },
                        popconfirm::PopconfirmSpec {
                            message: DELETE_CONFIRM.to_owned(),
                            confirm_label: ROW_DELETE.to_owned(),
                            cancel_label: "取消".to_owned(),
                        },
                        // 触发件用纯视图（nsh-base Button 自含按压语义会吞掉外
                        // 层按压——商户删除先例同判）。
                        view()
                            .app_class("px-2 py-0.5 rounded-sm border border-border text-foreground hover:bg-accent")
                            .child(
                                text(ROW_DELETE)
                                    .text_size(px(font_size::CAPTION))
                                    .text_color(palette.danger_fg)
                                    .test_id(format!("cat-delete-{row_id}").as_str()),
                            ),
                        {
                            let row_id = row_id.clone();
                            move || perform_delete(row_id.clone())
                        },
                    ))
                    .child(
                        nsh_base::button()
                            .app_class("px-2 py-0.5 rounded-sm border border-border text-foreground hover:bg-accent disabled:opacity-40")
                            .disabled(move || row.sibling_index == 0)
                            .on_press({
                                let row_id = row_id.clone();
                                move |_: &()| perform_move(row_id.clone(), -1)
                            })
                            .child(
                                text(ROW_UP)
                                    .text_size(px(font_size::CAPTION))
                                    .text_color(palette.fg)
                                    .test_id(format!("cat-up-{row_id}").as_str()),
                            ),
                    )
                    .child(
                        nsh_base::button()
                            .app_class("px-2 py-0.5 rounded-sm border border-border text-foreground hover:bg-accent disabled:opacity-40")
                            .disabled(move || row.sibling_index + 1 >= row.sibling_count)
                            .on_press({
                                let row_id = row_id.clone();
                                move |_: &()| perform_move(row_id.clone(), 1)
                            })
                            .child(
                                text(ROW_DOWN)
                                    .text_size(px(font_size::CAPTION))
                                    .text_color(palette.fg)
                                    .test_id(format!("cat-down-{row_id}").as_str()),
                            ),
                    )
            },
                )),
        );

    // 编辑弹窗（名称 / 图标 / 父分类三字段 + 保存；域校验错误在弹窗内呈现
    // 不关闭）。
    let edit_modal = app_modal(
        move || edit_target.get().is_some(),
        || EDIT_MODAL_TITLE.to_owned(),
        close_edit,
        move || {
            view()
                .flex_col()
                .gap_2()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            text(FIELD_NAME)
                                .text_size(px(font_size::CAPTION))
                                .text_color(palette.fg),
                        )
                        .child(
                            input()
                                .test_id("cat-edit-name")
                                .value(move || edit_name.get())
                                .placeholder(NAME_PLACEHOLDER)
                                .optional_label(NAME_PLACEHOLDER)
                                .text_color(palette.fg)
                                .w(px(200.0))
                                .app_class(
                                    "bg-background rounded-(--radius) border border-border px-2 py-1 \
                                     placeholder:text-muted-foreground",
                                )
                                .on_change(move |value: &str| edit_name.set(value.to_owned())),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            text(FIELD_ICON)
                                .text_size(px(font_size::CAPTION))
                                .text_color(palette.fg),
                        )
                        .child(
                            input()
                                .test_id("cat-edit-icon")
                                .value(move || edit_icon.get())
                                .placeholder(ICON_PLACEHOLDER)
                                .optional_label(ICON_PLACEHOLDER)
                                .text_color(palette.fg)
                                .w(px(120.0))
                                .app_class(
                                    "bg-background rounded-(--radius) border border-border px-2 py-1 \
                                     placeholder:text-muted-foreground",
                                )
                                .on_change(move |value: &str| edit_icon.set(value.to_owned())),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            text(FIELD_PARENT)
                                .text_size(px(font_size::CAPTION))
                                .text_color(palette.fg),
                        )
                        .child(pinyin_select::pinyin_select(
                            "cat-edit-parent",
                            move || {
                                let target = edit_target.get_untracked();
                                parent_options(
                                    &rows.get_untracked(),
                                    kind.get_untracked(),
                                    target.as_deref(),
                                )
                            },
                            edit_parent,
                            PARENT_NONE,
                            true,
                        )),
                )
                .child(
                    when(
                        move || edit_error.get().is_some(),
                        move || {
                            text(move || edit_error.get().unwrap_or_default())
                                .text_size(px(font_size::CAPTION))
                                .text_color(palette.danger_fg)
                                .test_id("cat-edit-error")
                        },
                    )
                    .otherwise(view),
                )
                .child(
                    view().flex_row().justify_end().child(
                        nsh_base::button()
                            .app_class("px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90")
                            .on_press(move |_: &()| perform_edit())
                            .child(
                                text(BUTTON_SAVE)
                                    .text_size(px(font_size::SECONDARY))
                                    .text_color(palette.primary_fg)
                                    .test_id("cat-edit-save"),
                            ),
                    ),
                )
        },
    );

    // 树行 each 置尾（清单卡是本页唯一的 each 宿主；引擎对 each 之后的反应
    // 式兄弟件的绑定失活怪象见交付报告注记——状态条位 / 错误行 / 编辑弹窗
    // 一律排在 each 之前，反应式绑定全部先行）。
    view()
        .flex_1()
        .min_h(px(0.0))
        .flex_col()
        .app_class("gap-3")
        .child(kind_bar)
        .child(add_card)
        .child(
            when(
                move || error.get().is_some(),
                move || {
                    text(move || error.get().unwrap_or_default())
                        .text_size(px(font_size::CAPTION))
                        .text_color(palette.danger_fg)
                        .test_id("cat-load-error")
                },
            )
            .otherwise(view),
        )
        .child(edit_modal)
        .child(list_card)
}

// ---------------------------------------------------------------------------
// 视图（设置页「分类」页签消费面）。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(id: &str, name: &str, kind: &str, parent: Option<&str>) -> CategoryItem {
        CategoryItem {
            id: id.to_owned(),
            name: name.to_owned(),
            kind: kind.to_owned(),
            parent_id: parent.map(str::to_owned),
            icon: None,
        }
    }

    /// 树行投影（旧壳两级树口径）：根行保域读序、子行随父带「父 > 子」路径
    /// 名与同父兄弟定位（上移 / 下移可用性的判定源）。
    #[test]
    fn category_row_views_project_two_level_tree_with_sibling_positions() {
        let rows = vec![
            cat("a", "甲", "expense", None),
            cat("a1", "子一", "expense", Some("a")),
            cat("a2", "子二", "expense", Some("a")),
            cat("b", "乙", "expense", None),
            cat("c", "丙", "income", None),
        ];
        let views = category_row_views(&rows, "expense");
        assert_eq!(
            views.iter().map(|v| v.label.as_str()).collect::<Vec<_>>(),
            ["甲", "甲 > 子一", "甲 > 子二", "乙"],
            "根行保序、子行路径名、income 不入支出视图"
        );
        let a2 = views.iter().find(|v| v.id == "a2").unwrap();
        assert_eq!((a2.depth, a2.sibling_index, a2.sibling_count), (1, 1, 2));
        let b = views.iter().find(|v| v.id == "b").unwrap();
        assert_eq!((b.depth, b.sibling_index, b.sibling_count), (0, 1, 2));
    }

    /// 父分类选项闭集：首项「无（顶级）」哨兵空 id + 同类根分类（排除自身
    /// ——编辑弹窗自引用不可选）。
    #[test]
    fn parent_options_cover_root_sentinel_and_exclude_self() {
        let rows = vec![
            cat("a", "甲", "expense", None),
            cat("a1", "子一", "expense", Some("a")),
            cat("b", "乙", "income", None),
        ];
        let options = parent_options(&rows, "expense", None);
        assert_eq!(
            options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
            ["", "a"],
            "哨兵 + 同类根；子分类与 income 不入"
        );
        assert_eq!(options[0].label, PARENT_NONE);
        let excluding = parent_options(&rows, "expense", Some("a"));
        assert_eq!(
            excluding.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
            [""],
            "编辑弹窗排除自身"
        );
    }

    /// 上移 / 下移的重排提交项（矩阵：位移一步、到头回 None、全集落定项重
    /// 编 sort_order 0..n）。
    #[test]
    fn reorder_items_for_move_matrix() {
        let rows = [
            cat("a", "甲", "expense", None),
            cat("b", "乙", "expense", None),
            cat("c", "丙", "expense", None),
        ];
        let sibs: Vec<&CategoryItem> = rows.iter().collect();
        // 中间行上移一步：b → 0 位，a、c 顺延。
        let items = reorder_items_for_move(&sibs, "b", -1).unwrap();
        assert_eq!(
            items
                .iter()
                .map(|i| (i.id.as_str(), i.sort_order))
                .collect::<Vec<_>>(),
            [("b", 0), ("a", 1), ("c", 2)]
        );
        // 首行上移 / 末行下移 = 到头，不发起写。
        assert!(reorder_items_for_move(&sibs, "a", -1).is_none());
        assert!(reorder_items_for_move(&sibs, "c", 1).is_none());
        // 未知 id 不动作。
        assert!(reorder_items_for_move(&sibs, "zz", -1).is_none());
    }

    /// 词表锁定（旧壳 settings.categories zh-CN 词条原样承载）：文案走样即
    /// 红（用户可观察面的单测落点）。
    #[test]
    fn word_list_mirrors_old_shell_zh_cn() {
        assert_eq!(TAB_KIND_EXPENSE, "支出");
        assert_eq!(TAB_KIND_INCOME, "收入");
        assert_eq!(CARD_ADD, "新增分类");
        assert_eq!(CARD_LIST, "分类列表");
        assert_eq!(PARENT_NONE, "无（顶级）");
        assert_eq!(DELETE_CONFIRM, "确认删除？");
        assert_eq!(MSG_NAME_REQUIRED, "请输入分类名称");
        assert_eq!(MSG_ADDED, "已添加分类");
        assert_eq!(MSG_UPDATED, "已更新分类");
        assert_eq!(MSG_DELETED, "已删除");
    }
}
