//! 保司管理视图（issue #2051 NSH 迁移⑳；旧壳 `InsurerManager.vue` +
//! `InsurerEditModal.vue` 的重生，issue #714 / ADR-0082 决策 3 / ADR-0063）：保险域自有
//! 名字字典的管理视图——新增表单卡（名称）+ 清单卡（名称序平表 + 搜索 +
//! 「显示已删」只读切换）+ 单字段编辑弹窗 + 软删轻量确认气泡（Popconfirm
//! 套件，删除不升危险确认对话框——软删可同步恢复语义）。
//!
//! 读路径直调域（[`boot::insurer_rows`]，含已删全量一次装载，在用/已删双分区
//! 拆分归装配层纯面）；写路径直调域公开写入口（不经 IPC，ADR-0145 决策 4），
//! 字典完整性守卫（在用行唯一 / 空名拒绝 / NotFound）住域写入口单正文，本壳
//! 不自建。旧壳「写入成功后 ledger:changed 参考失效重拉」在壳内对应物 = 写
//! 成功回调自拉重读（保司不挂交易，交易失效信号不涉——参考数据依赖面仅
//! 未迁的保单表单）。搜索 = 统一模糊搜索语义本地过滤（ADR-0027 唯一定义点的
//! Rust 侧原语，装配层 [`assembly::filter_insurer_rows`] 单点）——保序不重排，
//! 空词恢复全量；保司字典前端全量驻留，属本地过滤形态（旧壳同口径，无分页）。
//!
//! 界面文案仅 zh-CN（旧壳 policies.insurers.* zh-CN 词表原样承载）；码化错误
//! zh 原样呈现不加二级前缀；NSH 无 toast，动作反馈落 `ins-status` 条位
//!（#2038 裁决）。视图本地快照、离开再回 = 全新装载（旧壳 onMounted 同口径）；
//! 无下钻定位形态（落位即消费意图封闸——保司路由不读 focus 同旧壳语义）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use ledger_infra::error::AppError;
use nsh_base::{DataTableAlign, DataTableColumn, DataTableRows, button, data_table, input};
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::adaptive;
use crate::app::{db_slot, field_label, table_classes};
use crate::assembly::{self, INSURER_COLUMNS, InsurerRowCells};
use crate::boot;
use crate::modal;
use crate::nav;
use crate::nav_model::ViewName;
use crate::popconfirm;

// ---------------------------------------------------------------------------
// 词表（旧壳 policies.insurers.* zh-CN 词条原样；i18n 框架级方案不在本票，
// 字面承载）。
// ---------------------------------------------------------------------------
const ADD_CARD_TITLE: &str = "新增保险公司";
const LIST_CARD_TITLE: &str = "保险公司列表";
const LABEL_NAME: &str = "名称";
const BUTTON_ADD: &str = "添加";
const BUTTON_SAVE: &str = "保存";
const BUTTON_EDIT: &str = "编辑";
const BUTTON_DELETE: &str = "删除";
const DELETE_CONFIRM: &str = "确认删除该保险公司？相关保单仍会显示它的名字。";
const BUTTON_CANCEL: &str = "取消";
const EDIT_MODAL_TITLE: &str = "编辑保险公司";
const SEARCH_PLACEHOLDER: &str = "搜索保险公司（名称/拼音）";
const SHOW_DELETED: &str = "显示已删";
const DELETED_TAG: &str = "已删除";
const PLACEHOLDER_NAME: &str = "保险公司名称";
const MSG_NAME_REQUIRED: &str = "请输入保险公司名称";
const MSG_ADDED: &str = "已添加保险公司";
const MSG_ADD_FAILED: &str = "添加失败";
const MSG_UPDATED: &str = "已更新保险公司";
const MSG_UPDATED_FAILED: &str = "更新失败";
const MSG_DELETED: &str = "已删除保险公司";
const MSG_DELETE_FAILED: &str = "删除失败";
const READ_TASK_FAILED: &str = "内部错误：读任务异常退出";
const WRITE_TASK_FAILED: &str = "内部错误：写入任务异常退出";

/// 保司管理视图（组「更多」容器保司页签的真实组件，#2051）。
#[component]
pub(crate) fn InsurersView() -> impl IntoElement {
    let view_owner = Owner::current();

    // ============================ 快照态 ============================
    // 视图本地快照（离开即回占位态，旧壳重挂同口径）：含已删全量一次装配，
    // 在用/已删双分区展示面由装配纯函数派生（参照数据依赖面仅未迁的保单
    // 表单——保司不挂交易，零失效信号订阅）。
    let rows = signal(Vec::<InsurerRowCells>::new());
    let loading = signal(true);
    let error = signal(None::<String>);
    let load_seq = signal(0u64);

    // 装载批：含已删全量单点（旧壳 reference store `listInsurers
    // {includeDeleted: true}` 同口径），分区归装配纯函数。
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
                let outcome = blocking(move || boot::insurer_rows(&db, true)).await;
                if load_seq.get() != seq {
                    return;
                }
                match outcome {
                    Ok(Ok(loaded)) => {
                        // 装配单点（渲染期零格式化）：域行一次装配成展示行集
                        //（含已删，分区/搜索展示面归下方纯派生）。
                        rows.set(assembly::assemble_insurer_rows(&loaded));
                    }
                    Ok(Err(e)) => error.set(Some(e.to_string())),
                    Err(_) => error.set(Some(READ_TASK_FAILED.to_owned())),
                }
                loading.set(false);
            });
        });
    };

    // 落位：首次装载 + 意图消费（读一次封闸：保司路由无自身定位形态，意图
    // 投递即弃——旧壳 insurers 路由不读 focus 同语义）。
    effect(move || {
        let _landed = nav::store().current.get();
        let _ = nav::take_intent();
        reload();
    });

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

    // 操作反馈条位（NSH 无 toast——#2038 裁决；旧壳 message.success/error 的
    // 条位对应物：新动作开始时清回空）。
    let status = signal(None::<String>);

    // ============================ 展示面（搜索 + 显示已删） ============================
    // 搜索词（本地过滤形态：保司字典前端全量驻留，旧壳同口径）；显示已删
    //（默认只显示在用保司，切换后已删保司以只读行追加在尾部）。
    let search_term = signal(String::new());
    let show_deleted = signal(false);

    // ============================ 新增 / 编辑 / 删除 ============================
    // 新增卡（名称单字段）；写入成功后表单清空、错误条位在卡内（重名等后
    // 端校验错误原样上抛展示，表单不清空——旧壳同口径）。
    let add_name = signal(String::new());
    let add_error = signal(None::<String>);

    let perform_add = move || {
        add_error.set(None);
        status.set(None);
        let name = add_name.get_untracked().trim().to_owned();
        if name.is_empty() {
            add_error.set(Some(MSG_NAME_REQUIRED.to_owned()));
            return;
        }
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => {
                        boot::create_insurer(db, ledger_policy::InsurerInput { name: name.clone() })
                            .map(|_id| ())
                    }
                    None => Err(AppError::coded(
                        boot::CODE_SESSION_MISSING,
                        "内部错误：账本库会话缺失",
                    )),
                })
                .await;
                match outcome {
                    Ok(Ok(())) => {
                        add_name.set(String::new());
                        status.set(Some(MSG_ADDED.to_owned()));
                        reload();
                    }
                    // 码化错误 zh 原样呈现（旧壳 addFailed 同口径）；表单不清空、
                    // 用户可直接修正。
                    Ok(Err(e)) => add_error.set(Some(format!("{MSG_ADD_FAILED}: {e}"))),
                    Err(_) => add_error.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // 编辑（单字段轻量弹窗，旧壳 ModalIntent 单成员意图闭集的对应形态——条件
    // 意图即弹、关闭即回占位；打开时回填原值）。改名字典即时生效（引用指向
    // id，不回刷历史行——域内单点）。
    let edit_target = signal(None::<String>);
    let edit_name = signal(String::new());
    let edit_error = signal(None::<String>);

    // 行快照反查（编辑回填目标）：全量装配快照内单点（已删行无编辑入口，
    // 回查仅服务在用行；行缺席并发已删动作静默退场）。
    let open_edit = move |insurer_id: String| {
        if let Some(target) = rows
            .get_untracked()
            .iter()
            .find(|r| r.id == insurer_id)
            .cloned()
        {
            edit_name.set(target.name.clone());
            edit_error.set(None);
            edit_target.set(Some(insurer_id));
        }
    };
    let close_edit = move || edit_target.set(None);

    let perform_update = move || {
        edit_error.set(None);
        status.set(None);
        let Some(insurer_id) = edit_target.get_untracked() else {
            return;
        };
        let name = edit_name.get_untracked().trim().to_owned();
        if name.is_empty() {
            edit_error.set(Some(MSG_NAME_REQUIRED.to_owned()));
            return;
        }
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::update_insurer(
                        db,
                        &insurer_id,
                        ledger_policy::InsurerUpdateInput {
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
                        reload();
                    }
                    // 重名等后端校验错误原样上抛展示，弹窗不关、内容不丢（旧
                    // 壳 updateFailed 同口径）。
                    Ok(Err(e)) => edit_error.set(Some(format!("{MSG_UPDATED_FAILED}: {e}"))),
                    Err(_) => edit_error.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // 软删（轻量确认气泡：旧壳 AppPopconfirm 同形态——软删可同步恢复语义不
    // 升危险确认）。存量保单引用照常显示（软删不可再被新选择由消费方校验，
    // 域内单点）；已删行移出默认位、「显示已删」只读切换可见。
    let popconfirm_open = signal(None::<String>);

    let perform_delete = move |insurer_id: String| {
        status.set(None);
        let db = db_slot().clone();
        view_owner.run(move || {
            spawn(async move {
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => boot::delete_insurer(db, &insurer_id),
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
                    Ok(Err(e)) => status.set(Some(format!("{MSG_DELETE_FAILED}: {e}"))),
                    Err(_) => status.set(Some(WRITE_TASK_FAILED.to_owned())),
                }
            });
        });
    };

    // ============================ 表格装配（渲染期零格式化） ============================
    let partitions = computed(move || assembly::partition_insurer_rows(&rows.get()));
    let active_rows = computed(move || partitions.get().0);
    let deleted_rows = computed(move || partitions.get().1);
    // 展示行集 = (显示已删？在用 + 已删：仅在用) → 搜索过滤（装配纯面单点，
    // 保序不重排）。
    let display_rows = computed(move || {
        assembly::insurer_display_rows(
            &active_rows.get(),
            &deleted_rows.get(),
            show_deleted.get(),
            &search_term.get(),
        )
    });

    let columns = computed(move || map_columns(INSURER_COLUMNS, table_width()));

    // 行操作（旧壳操作列 NButton 同职责；已删行只读，操作格由 cell 渲染屋
    // 空面）。编辑目标由快照反查（open_edit 内单点）。
    let actions_of = move |row: &InsurerRowCells| -> nsh_core::element::View {
        let insurer_id = row.id.clone();
        let edit_insurer_id = insurer_id.clone();
        view()
            .flex_row()
            .gap_1()
            .child(
                view()
                    .test_id(format!("ins-edit-{insurer_id}").as_str())
                    .app_class("rounded-md hover:bg-accent px-1")
                    .on_press(move |_: &PressEvent| open_edit(edit_insurer_id.clone()))
                    .child(text(BUTTON_EDIT).text_size(px(12.0)).text_color(fg)),
            )
            .child(
                // 轻量确认气泡（触发件用纯视图，nsh-base Button 自含按压语义
                // 会吞掉外层按压——封装的按压源是外层包裹视图，items 先例同注）。
                popconfirm::popconfirm(
                    "ins-delete",
                    {
                        let insurer_id = insurer_id.clone();
                        move || popconfirm_open.get().as_deref() == Some(insurer_id.as_str())
                    },
                    {
                        let insurer_id = insurer_id.clone();
                        move |shown: bool| popconfirm_open.set(shown.then(|| insurer_id.clone()))
                    },
                    popconfirm::PopconfirmSpec {
                        message: DELETE_CONFIRM.to_owned(),
                        confirm_label: BUTTON_DELETE.to_owned(),
                        cancel_label: BUTTON_CANCEL.to_owned(),
                    },
                    view()
                        .test_id(format!("ins-delete-{insurer_id}").as_str())
                        .app_class("rounded-md hover:bg-accent px-1")
                        .child(
                            text(BUTTON_DELETE)
                                .text_size(px(12.0))
                                .text_color(danger_fg),
                        ),
                    move || perform_delete(insurer_id.clone()),
                ),
            )
    };

    // 自绘格：名称列（已删行尾部带「已删除」标记，旧壳 deletedTag 同词）、
    // 操作列（行内按钮组 = 编辑 + 删除气泡；已删行只读格空）。
    let cell_of = move |cell: &nsh_base::DataTableCell| -> nsh_core::element::AnyElement {
        let index = cell.row as usize;
        let assembled = display_rows.get_untracked().get(index).cloned();
        match cell.key.as_ref() {
            "name" => {
                let is_deleted = assembled.as_ref().is_some_and(|row| row.is_deleted);
                view()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(
                        text(cell.text.clone())
                            .text_size(px(13.0))
                            .text_color(fg)
                            .number_of_lines(1),
                    )
                    .child(
                        when(
                            move || is_deleted,
                            move || {
                                text(DELETED_TAG)
                                    .text_size(px(12.0))
                                    .text_color(muted)
                                    .test_id("ins-deleted-tag")
                                    .app_class("px-1.5 rounded-md border border-border")
                            },
                        )
                        .otherwise(view),
                    )
                    .into_element()
            }
            "actions" => match assembled {
                Some(row) if !row.is_deleted => actions_of(&row).into_element(),
                _ => view().into_element(),
            },
            _ => text(cell.text.clone())
                .text_size(px(13.0))
                .text_color(fg)
                .number_of_lines(1)
                .into_element(),
        }
    };

    // 表数据（渲染期零格式化：格文本已装配）。
    let data_of = move || {
        let snapshot = display_rows.get();
        DataTableRows::new(snapshot.len())
            .texts("name", snapshot.iter().map(|r| r.name.as_str()))
            .texts("actions", snapshot.iter().map(|_| ""))
    };

    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(
            view().px_4().py_2().child(
                text(ViewName::Insurers.label())
                    .text_size(px(16.0))
                    .text_color(fg)
                    .test_id("view-title"),
            ),
        )
        .child(
            // 装载失败条位（码化错误 zh-CN 原样，不静默——字典读数缺失不静默
            // 回退空清单）。
            when(
                move || error.get().is_some(),
                move || {
                    text(move || format!("加载失败：{}", error.get().unwrap_or_default()))
                        .text_size(px(12.0))
                        .text_color(danger_fg)
                        .test_id("ins-error")
                        .app_class("px-4 py-1")
                },
            )
            .otherwise(view),
        )
        .child(
            // 内容区（储蓄目标视图同形态的字段卡片列；表格虚拟化按自身高度
            // 裁行——根内容列必须 flex_1，卡片弹性高才有确定高来源，#2041
            // 先例）。
            view()
                .flex_1()
                .flex_col()
                .gap_3()
                .p_4()
                .child(
                    // 新增表单卡（交互照商户管理页先例：新增表单卡片 + 列表
                    // 卡片；空名前置拦截不写入）。
                    view()
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
                                .gap_2()
                                .child(field_label(LABEL_NAME, muted))
                                .child(
                                    input()
                                        .test_id("ins-add-name")
                                        .value(move || add_name.get())
                                        .placeholder(PLACEHOLDER_NAME)
                                        .optional_label("保司名称")
                                        .text_color(fg)
                                        .w(px(240.0))
                                        .app_class(
                                            "bg-background rounded-md border border-border \
                                             px-2 py-1 placeholder:text-muted-foreground",
                                        )
                                        .on_change(move |text: &str| add_name.set(text.to_owned())),
                                )
                                .child(
                                    button()
                                        .app_class(
                                            "px-3 py-1.5 rounded-md bg-primary hover:opacity-90",
                                        )
                                        .child(
                                            text(BUTTON_ADD)
                                                .text_size(px(13.0))
                                                .text_color(rgb(0xffffff))
                                                .test_id("ins-add"),
                                        )
                                        .on_press(move |_: &()| perform_add()),
                                ),
                        )
                        .child(
                            // 新增错误条位（码化错误 zh 原样；下次尝试开始时清空）。
                            when(
                                move || add_error.get().is_some(),
                                move || {
                                    text(move || add_error.get().unwrap_or_default())
                                        .text_size(px(12.0))
                                        .text_color(danger_fg)
                                        .test_id("ins-add-error")
                                },
                            )
                            .otherwise(view),
                        ),
                )
                .child(
                    // 清单卡：搜索 + 显示已删 + 名称序平表。
                    view()
                        .flex_1()
                        .flex_col()
                        .gap_2()
                        .app_class("rounded-md border border-border p-3")
                        .child(
                            text(LIST_CARD_TITLE)
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(fg),
                        )
                        .child(
                            view().flex_row().items_center().gap_2().child(
                                input()
                                    .test_id("ins-search")
                                    .value(move || search_term.get())
                                    .placeholder(SEARCH_PLACEHOLDER)
                                    .optional_label("搜索保司")
                                    .text_color(fg)
                                    .w(px(240.0))
                                    .app_class(
                                        "bg-background rounded-md border border-border px-2 \
                                         py-1 placeholder:text-muted-foreground",
                                    )
                                    .on_change(move |text: &str| search_term.set(text.to_owned())),
                            ),
                        )
                        .child(
                            view().flex_row().items_center().gap_2().child(
                                button()
                                    .accessibility_label(if show_deleted.get() {
                                        "隐藏已删保司"
                                    } else {
                                        SHOW_DELETED
                                    })
                                    .app_class(
                                        "px-2 py-1 rounded-md text-foreground hover:bg-accent \
                                             selected:bg-accent",
                                    )
                                    .child(
                                        text(move || {
                                            format!(
                                                "{} {}",
                                                if show_deleted.get() { "☑" } else { "☐" },
                                                SHOW_DELETED
                                            )
                                        })
                                        .text_size(px(12.0))
                                        .text_color(fg)
                                        .test_id("ins-show-deleted"),
                                    )
                                    .on_press(move |_: &()| {
                                        show_deleted.update(|v| *v = !*v);
                                    }),
                            ),
                        )
                        .child(
                            view().test_id("ins-table").flex_1().child(
                                data_table()
                                    .columns(columns)
                                    .data(data_of)
                                    .render_cell(cell_of)
                                    .loading(move || loading.get())
                                    .loading_view(text("正在加载…").text_color(muted))
                                    .classes(table_classes())
                                    .flex_1(),
                            ),
                        ),
                )
                .child(
                    // 操作反馈条位（成功/失败共用一条；新动作开始清回空）。
                    when(
                        move || status.get().is_some(),
                        move || {
                            text(move || status.get().unwrap_or_default())
                                .text_size(px(12.0))
                                .text_color(fg)
                                .test_id("ins-status")
                        },
                    )
                    .otherwise(view),
                )
                .child(
                    // 编辑弹层（显示由目标非空派生；关闭 ✕ / ESC / 保存成功
                    // 统一经 close_edit 终态）。
                    modal::app_modal(
                        move || edit_target.get().is_some(),
                        move || EDIT_MODAL_TITLE.to_owned(),
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
                                        .child(field_label(LABEL_NAME, muted))
                                        .child(
                                            input()
                                                .test_id("ins-edit-name")
                                                .value(move || edit_name.get())
                                                .placeholder(PLACEHOLDER_NAME)
                                                .optional_label("保司名称")
                                                .text_color(fg)
                                                .app_class(
                                                    "bg-background rounded-md border \
                                                     border-border px-2 py-1 \
                                                     placeholder:text-muted-foreground",
                                                )
                                                .on_change(move |text: &str| {
                                                    edit_name.set(text.to_owned())
                                                }),
                                        ),
                                )
                                .child(
                                    // 编辑错误条位（重名等校验错误原样；弹窗不关、
                                    // 内容不丢）。
                                    when(
                                        move || edit_error.get().is_some(),
                                        move || {
                                            text(move || edit_error.get().unwrap_or_default())
                                                .text_size(px(12.0))
                                                .text_color(danger_fg)
                                                .test_id("ins-edit-error")
                                        },
                                    )
                                    .otherwise(view),
                                )
                                .child(
                                    // 按钮行右对齐单主键（轻量单字段编辑保留无
                                    // 取消键语义，旧壳同口径）。
                                    view().flex_row().justify_end().child(
                                        button()
                                            .app_class(
                                                "px-3 py-1.5 rounded-md bg-primary \
                                                     hover:opacity-90",
                                            )
                                            .child(
                                                text(BUTTON_SAVE)
                                                    .text_size(px(13.0))
                                                    .text_color(rgb(0xffffff))
                                                    .test_id("ins-edit-save"),
                                            )
                                            .on_press(move |_: &()| perform_update()),
                                    ),
                                )
                        },
                    ),
                ),
        )
}

// ---------------------------------------------------------------------------
// 视图局部辅助（列映射与快照反查单点：UI 层只做映射不产列不产装配）。
// ---------------------------------------------------------------------------

/// 表格外围定宽扣减（保司列宽计算输入）：内容列 p-4（两侧 16pt）+ 清单卡
/// p-3（两侧 12pt）+ 清单卡描边两侧 1pt——与储蓄目标视图同形。窄窗自适应
/// 随系列尾部横切票。
const INSURERS_TABLE_WIDTH_PAD: f32 = 16.0 * 2.0 + 12.0 * 2.0 + 2.0;

/// 内容可用宽（视口 - 侧栏；列宽计算输入，储蓄目标视图先例同口径）。
fn table_width() -> f32 {
    adaptive::content_width()
}

/// 装配列 → 宿主列（UI 层只做映射不产列；弹性宽度分配同储蓄目标视图先例
/// ——列组件无弹性列机制，富余宽度全给名称列；总宽不得超出表格实测可用
/// 宽，尾列缺席即操作钮灭失）。
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
    let extra = (width - INSURERS_TABLE_WIDTH_PAD - suggested).max(0.0);
    if extra > 0.0
        && let Some(name) = columns.iter_mut().find(|col| col.key == "name")
        && let Some(w) = name.width
    {
        name.width = Some(Points(w.0 + extra));
    }
    columns
}
