//! 账本切换卡（数据页签卡组；旧壳 `useBookSwitcher` 的重生，ADR-0114，
//! #2167 自 [`crate::settings_data`] 按职责拆出）：清单卡（跨账本汇总置顶
//! 入口 + 行按压切换〔warning 级轻量确认〕+ 行级改名弹窗 / 移除气泡确认 +
//! 新建弹窗）。清单读注册表最新落盘态；切换写活动指针，本壳无原位重引导
//!（旧壳 restart_app 自动重载的偏差不在屏披露，反馈句与侧栏 #2129 干净变
//! 体同文）。
//!
//! 危险确认弹窗经 [`crate::danger`] 封装族；汇总入口消费 nav 单点接缝不自
//! 建；注册表读写等阻塞 IO 一律 `blocking` 后台执行，信号读写都在引擎 UI
//! 线程。弹窗条件段机制选型布尔有无 → `when`（#2146 定案，模块族注记见
//! [`crate::settings_data`]）。

use nsh_base::{button, input};
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::app::book_dir;
use crate::danger::{DangerConfirmSpec, DangerLevel, DangerOutcome, danger_confirm};
use crate::design::font_size;
use crate::modal::app_modal;
use crate::nav_model::NavRequest;
use crate::popconfirm;
use crate::settings;
use crate::shell_msgs::MSG_WRITE_TASK_FAILED;
use crate::{boot, nav, nav_model, views};

const CARD_BOOKS: &str = "账本切换";

const BOOKS_ENTRY_SUMMARY: &str = "全部账本（投资汇总）";
const BOOKS_FALLBACK_TITLE: &str = "账本注册表异常，已回退默认账本";
const BOOKS_PANEL_CURRENT: &str = "当前";
const BOOKS_NEW: &str = "新建账本";
const BOOKS_RENAME: &str = "重命名";
const BOOKS_REMOVE: &str = "移除";
const BOOKS_REMOVE_CONFIRM_PREFIX: &str =
    "移除后可随时重新登记找回，账本文件仍保留在磁盘。确定移除「";
const BOOKS_REMOVE_CONFIRM_SUFFIX: &str = "」？";
const BOOKS_REMOVE_ACTION: &str = "移除";
const BOOKS_CANCEL: &str = "取消";
const BOOKS_CREATE_TITLE: &str = "新建账本";
const BOOKS_RENAME_TITLE: &str = "重命名账本";
const BOOKS_NAME_LABEL: &str = "账本名称";
const BOOKS_CREATE_PLACEHOLDER: &str = "例如：家庭账本";
const BOOKS_CREATE_CONFIRM: &str = "创建";
const BOOKS_RENAME_CONFIRM: &str = "保存";
const BOOKS_SWITCH_TITLE: &str = "切换账本";
const BOOKS_SWITCH_CONFIRM: &str = "切换并重启后生效";
const BOOKS_SWITCH_DETAIL: &str =
    "将写活动账本指针：重启应用后进入目标账本。未保存的界面状态会丢失，账本数据本身不受影响。";
const BOOKS_SWITCHING_PREFIX: &str = "已切换到「";
const BOOKS_SWITCHING_SUFFIX: &str = "」，重启应用后进入新账本";
const BOOKS_ACTION_FAILED_PREFIX: &str = "操作失败: ";
const BOOKS_SET_FAILED_PREFIX: &str = "设置失败: ";
const BOOKS_SESSION_UNAVAILABLE: &str = "账本注册表尚未就绪，请重启应用后再试";
const BOOKS_CREATED_PREFIX: &str = "已创建账本「";
const BOOKS_CREATED_SUFFIX: &str = "」，进入即自动建库";
const BOOKS_RENAMED_PREFIX: &str = "已重命名为「";
const BOOKS_RENAMED_SUFFIX: &str = "」";
const BOOKS_REMOVED_PREFIX: &str = "已移除账本「";
const BOOKS_REMOVED_SUFFIX: &str = "」，文件仍保留在磁盘";

// ---------------------------------------------------------------------------
// 账本切换卡（清单 + 跨账本汇总置顶入口 + 行级改名 / 移除 + 新建 + 切换）。
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct BooksState {
    /// 清单行集（最新落盘态读注册表）。
    books: Signal<Vec<ledger_infra::db::book_registry::Book>>,
    /// 活动账本 id。
    active: Signal<Option<String>>,
    /// 登记变更可用性（可变 = 注册表可读且无推迟搬迁窗口）。
    mutable: Signal<bool>,
    /// 回退警示（回退原因随行）。
    fallback_reason: Signal<Option<String>>,
    /// 状态条位（动作成功 / 失败反馈）。
    status: Signal<Option<String>>,
    /// 命名弹窗意图（新建 / 改名）。
    name_intent: Signal<Option<BookNameIntent>>,
    /// 命名输入草稿。
    name_draft: Signal<String>,
    /// 命名提交执行中。
    name_busy: Signal<bool>,
    /// 切换确认待决目标（id, name）。
    switch_pending: Signal<Option<(String, String)>>,
    /// 切换执行中（在途拒绝再次发起）。
    switching: Signal<bool>,
    /// 移除气泡确认的开合（行级 id = 已开气泡的行）。
    popconfirm_open: Signal<Option<String>>,
}

/// 命名弹窗意图（旧壳 BookNameIntent 闭集同形）。
#[derive(Clone, Debug, PartialEq)]
enum BookNameIntent {
    /// 新建。
    Create,
    /// 改名（携带目标账本行快照）。
    Rename(ledger_infra::db::book_registry::Book),
}

/// 清单装载（聚合读注册表最新落盘态；本壳默认目录 = 启动参数账本目录）。
/// 删装载接线 → 清单永远空态 → 红。
fn refresh_books(owner: Owner, state: BooksState) {
    owner.run(move || {
        spawn(async move {
            let launch = book_dir();
            let outcome = blocking(move || match launch {
                Some(launch) => Ok(boot::book_list(&launch)),
                None => Err(ledger_infra::error::AppError::coded(
                    "book.registry-unavailable",
                    BOOKS_SESSION_UNAVAILABLE,
                )),
            })
            .await;
            match outcome {
                Ok(Ok(info)) => {
                    state.books.set(info.books.clone());
                    state.active.set(info.active_id.clone());
                    state.mutable.set(info.mutable);
                    state.fallback_reason.set(info.fallback_reason.clone());
                }
                Ok(Err(_)) | Err(_) => {
                    state.status.set(Some(format!(
                        "{BOOKS_ACTION_FAILED_PREFIX}{BOOKS_SESSION_UNAVAILABLE}"
                    )));
                }
            }
        })
    });
}

/// 账本切换卡（清单 + 汇总入口 + 行操作 + 新建 + 切换确认）。
pub(crate) fn books_card(palette: views::ViewPalette) -> View {
    let owner = Owner::current();
    let state = BooksState {
        books: signal(Vec::new()),
        active: signal(None),
        mutable: signal(false),
        fallback_reason: signal(None),
        status: signal(None),
        name_intent: signal(None),
        name_draft: signal(String::new()),
        name_busy: signal(false),
        switch_pending: signal(None),
        switching: signal(false),
        popconfirm_open: signal(None),
    };

    refresh_books(owner, state);

    // 跨账本汇总置顶入口（非账本条目、仅导航——不动活动指针；消费 nav 单
    // 点接缝不自建；汇总视图本体已随㉔就位，注册表不可达 / 投资关闭时不可用）。
    let open_summary = move || {
        nav::open(NavRequest::bare(nav_model::ViewName::CrossBookSummary));
    };
    let summary_available = move || {
        state.mutable.get()
            && state.fallback_reason.get().is_none()
            && !nav::store().is_closed(nav_model::ViewName::Investments)
    };

    // 行级动作入口。
    let open_create = move || {
        state.name_draft.set(String::new());
        state.name_intent.set(Some(BookNameIntent::Create));
    };
    let open_rename = move |book: ledger_infra::db::book_registry::Book| {
        state.name_draft.set(book.name.clone());
        state.name_intent.set(Some(BookNameIntent::Rename(book)));
    };
    let switch_request = move |book: ledger_infra::db::book_registry::Book| {
        let is_active = state.active.get_untracked().as_deref() == Some(book.id.as_str());
        if is_active || !state.mutable.get_untracked() || state.switching.get_untracked() {
            return;
        }
        state
            .switch_pending
            .set(Some((book.id.clone(), book.name.clone())));
    };

    // 切换确认对话框（轻量确认语义，warning 级；确认后写活动指针并反馈重
    // 启提示——本壳无原位重引导，删除切换写链 → 指针不写 → 旅程用例红）。
    let switch_dialog = {
        let (state, owner) = (state, owner);
        let on_outcome = move |outcome: DangerOutcome| {
            let Some((id, _name)) = state.switch_pending.get_untracked() else {
                return;
            };
            state.switch_pending.set(None);
            if outcome != DangerOutcome::Confirmed {
                return;
            }
            state.switching.set(true);
            owner.run(move || {
                spawn(async move {
                    let launch = book_dir();
                    let outcome = blocking(move || match launch {
                        Some(launch) => boot::switch_book(&launch, &id),
                        None => Err(ledger_infra::error::AppError::coded(
                            "book.registry-unavailable",
                            BOOKS_SESSION_UNAVAILABLE,
                        )),
                    })
                    .await;
                    state.switching.set(false);
                    match outcome {
                        Ok(Ok(book)) => {
                            state.status.set(Some(format!(
                                "{BOOKS_SWITCHING_PREFIX}{}{BOOKS_SWITCHING_SUFFIX}",
                                book.name
                            )));
                            refresh_books(owner, state);
                        }
                        Ok(Err(e)) => state
                            .status
                            .set(Some(format!("{BOOKS_ACTION_FAILED_PREFIX}{e}"))),
                        Err(_) => state.status.set(Some(format!(
                            "{BOOKS_ACTION_FAILED_PREFIX}{MSG_WRITE_TASK_FAILED}"
                        ))),
                    }
                })
            });
        };
        danger_confirm(
            move || state.switch_pending.get().is_some(),
            DangerConfirmSpec {
                level: DangerLevel::Warning,
                title: BOOKS_SWITCH_TITLE.to_owned(),
                lead: None,
                strong_warning: None,
                detail: Some(BOOKS_SWITCH_DETAIL.to_owned()),
                confirm_label: BOOKS_SWITCH_CONFIRM.to_owned(),
                cancel_label: BOOKS_CANCEL.to_owned(),
            },
            on_outcome,
        )
    };

    // 移除动作（行级气泡确认的确认回调；仅摘登记，目录与库文件原样保留）。
    let remove_row = {
        let (state, owner) = (state, owner);
        move |book: ledger_infra::db::book_registry::Book| {
            owner.run(move || {
                spawn(async move {
                    let launch = book_dir();
                    let outcome = blocking(move || match launch {
                        Some(launch) => boot::remove_book(&launch, &book.id),
                        None => Err(ledger_infra::error::AppError::coded(
                            "book.registry-unavailable",
                            BOOKS_SESSION_UNAVAILABLE,
                        )),
                    })
                    .await;
                    match outcome {
                        Ok(Ok(())) => {
                            state.status.set(Some(format!(
                                "{BOOKS_REMOVED_PREFIX}{}{BOOKS_REMOVED_SUFFIX}",
                                book.name
                            )));
                            refresh_books(owner, state);
                        }
                        Ok(Err(e)) => state
                            .status
                            .set(Some(format!("{BOOKS_ACTION_FAILED_PREFIX}{e}"))),
                        Err(_) => state.status.set(Some(format!(
                            "{BOOKS_ACTION_FAILED_PREFIX}{MSG_WRITE_TASK_FAILED}"
                        ))),
                    }
                })
            });
        }
    };

    // 命名弹窗提交（新建 / 改名共用；空名不提交，失败保持打开可改）。
    let submit_name = {
        let (state, owner) = (state, owner);
        move || {
            let Some(intent) = state.name_intent.get_untracked() else {
                return;
            };
            let name = state.name_draft.get_untracked().trim().to_owned();
            if name.is_empty() || state.name_busy.get_untracked() {
                return;
            }
            state.name_busy.set(true);
            owner.run(move || {
                spawn(async move {
                    let launch = book_dir();
                    let is_create = matches!(intent, BookNameIntent::Create);
                    let outcome = blocking(move || match launch {
                        Some(launch) => match intent {
                            BookNameIntent::Create => {
                                boot::create_book(launch.as_path(), name.as_str())
                            }
                            BookNameIntent::Rename(book) => {
                                boot::rename_book(launch.as_path(), &book.id, name.as_str())
                            }
                        },
                        None => Err(ledger_infra::error::AppError::coded(
                            "book.registry-unavailable",
                            BOOKS_SESSION_UNAVAILABLE,
                        )),
                    })
                    .await;
                    state.name_busy.set(false);
                    match outcome {
                        Ok(Ok(book)) => {
                            state.name_intent.set(None);
                            state.status.set(Some(if is_create {
                                format!("{BOOKS_CREATED_PREFIX}{}{BOOKS_CREATED_SUFFIX}", book.name)
                            } else {
                                format!("{BOOKS_RENAMED_PREFIX}{}{BOOKS_RENAMED_SUFFIX}", book.name)
                            }));
                            refresh_books(owner, state);
                        }
                        Ok(Err(e)) => state
                            .status
                            .set(Some(format!("{BOOKS_SET_FAILED_PREFIX}{e}"))),
                        Err(_) => state.status.set(Some(format!(
                            "{BOOKS_SET_FAILED_PREFIX}{MSG_WRITE_TASK_FAILED}"
                        ))),
                    }
                })
            });
        }
    };

    // 命名弹窗（旧壳 ModalIntent 二闭集同形；标题 / 确认文案随意图切换）。
    let name_modal = app_modal(
        move || state.name_intent.get().is_some(),
        move || match state.name_intent.get_untracked() {
            Some(BookNameIntent::Rename(_)) => BOOKS_RENAME_TITLE.to_owned(),
            _ => BOOKS_CREATE_TITLE.to_owned(),
        },
        move || {
            if !state.name_busy.get_untracked() {
                state.name_intent.set(None);
            }
        },
        move || {
            let confirm_label = match state.name_intent.get_untracked() {
                Some(BookNameIntent::Rename(_)) => BOOKS_RENAME_CONFIRM,
                _ => BOOKS_CREATE_CONFIRM,
            };
            let submit_row = view()
                .flex_row()
                .justify_end()
                .app_class("gap-2")
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-(--radius) border border-border text-foreground hover:bg-accent disabled:opacity-40",
                        )
                        .disabled(move || state.name_busy.get())
                        .child(
                            text(BOOKS_CANCEL)
                                .text_size(px(font_size::SECONDARY))
                                .test_id("book-name-cancel"),
                        )
                        .on_press(move |_: &()| {
                            if !state.name_busy.get_untracked() {
                                state.name_intent.set(None);
                            }
                        }),
                )
                .child(
                    button()
                        .app_class(
                            "px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90 disabled:opacity-40",
                        )
                        .disabled(move || {
                            state.name_busy.get() || state.name_draft.get().trim().is_empty()
                        })
                        .child(
                            text(confirm_label)
                                .text_size(px(font_size::SECONDARY))
                                .text_color(palette.primary_fg)
                                .test_id("book-name-submit"),
                        )
                        .on_press(move |_: &()| submit_name()),
                );
            view()
                .flex_col()
                .app_class("gap-2")
                .child(
                    text(BOOKS_NAME_LABEL)
                        .text_size(px(font_size::CAPTION))
                        .text_color(palette.fg),
                )
                .child(
                    input()
                        .test_id("book-name-input")
                        .value(move || state.name_draft.get())
                        .placeholder(BOOKS_CREATE_PLACEHOLDER)
                        .optional_label(BOOKS_NAME_LABEL)
                        .text_color(palette.fg)
                        .w(px(260.0))
                        .app_class(
                            "bg-background rounded-(--radius) border border-border px-2 py-1 placeholder:text-muted-foreground",
                        )
                        .on_change(move |value: &str| state.name_draft.set(value.to_owned())),
                )
                .child(submit_row)
        },
    );

    // 卡体（回退警示 + 汇总入口 + 清单行 + 新建 + 状态条位）。
    let body = view()
        .flex_col()
        .app_class("gap-2")
        .child(
            when(
                move || state.fallback_reason.get().is_some(),
                move || {
                    view()
                        .flex_col()
                        .app_class("rounded-sm border border-warning/30 bg-warning/10 p-2 gap-1")
                        .child(
                            text(BOOKS_FALLBACK_TITLE)
                                .text_size(px(font_size::CAPTION))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette.fg)
                                .test_id("book-fallback-title"),
                        )
                        .child(
                            text(move || state.fallback_reason.get().unwrap_or_default())
                                .text_size(px(font_size::CAPTION))
                                .text_color(palette.fg),
                        )
                },
            ),
        )
        .child(
            button()
                .app_class(
                    "px-3 py-1.5 rounded-(--radius) border border-border text-foreground hover:bg-accent disabled:opacity-40",
                )
                .disabled(move || !summary_available())
                .child(
                    text(BOOKS_ENTRY_SUMMARY)
                        .text_size(px(font_size::SECONDARY))
                        .test_id("book-summary-entry"),
                )
                .on_press(move |_: &()| open_summary()),
        )
        .child(each(
            move || state.books.get(),
            // key = id + 名称：组件均一次体内静态文案（如移除气泡确认中容
            // 携带行名），改名后同名 id 旧模版不重渲染；key 含名称让改名即
            // 重建行（极少频登记变更，重建代价可忽略）。
            |book: &ledger_infra::db::book_registry::Book| {
                format!("{}:{}", book.id, book.name)
            },
            move |item: ReadSignal<ledger_infra::db::book_registry::Book>,
                  _index: ReadSignal<usize>| {
                let is_active = move || {
                    state.active.get().as_deref() == Some(item.get().id.as_str())
                };
                book_row_view(
                    palette,
                    state,
                    _index.get(),
                    item,
                    BookRowActions {
                        is_active,
                        open_rename,
                        switch_request,
                        remove_row,
                    },
                )
            },
        ))
        .child(
            view().flex_row().app_class("items-center gap-2").child(
                button()
                    .app_class(
                        "px-3 py-1.5 rounded-(--radius) border border-border text-foreground hover:bg-accent disabled:opacity-40",
                    )
                    .disabled(move || !state.mutable.get())
                    .child(text(BOOKS_NEW).text_size(px(font_size::SECONDARY)).test_id("book-create"))
                    .on_press(move |_: &()| open_create()),
            ),
        )
        .child(settings::status_line("book-status", state.status, palette));

    settings::card(CARD_BOOKS, palette, vec![body])
        .child(switch_dialog)
        .child(name_modal)
}

/// 账本行（行按压切换意图 + 当前标记 + 改名 / 移除〔非活动本，气泡确认〕；
/// 行内容按注册表 key 各建一次，内部状态读走响应式闭包）。
/// 行级动作闭包组（[`book_row_view`] 的消费面；Copy 闭包打包成单参数）。
#[derive(Clone, Copy)]
struct BookRowActions<
    A: Fn() -> bool + Copy + 'static,
    B: Fn(ledger_infra::db::book_registry::Book) + Copy + 'static,
    C: Fn(ledger_infra::db::book_registry::Book) + Copy + 'static,
    D: Fn(ledger_infra::db::book_registry::Book) + Copy + 'static,
> {
    is_active: A,
    open_rename: B,
    switch_request: C,
    remove_row: D,
}

fn book_row_view(
    palette: views::ViewPalette,
    state: BooksState,
    index: usize,
    book: ReadSignal<ledger_infra::db::book_registry::Book>,
    actions: BookRowActions<
        impl Fn() -> bool + Copy + 'static,
        impl Fn(ledger_infra::db::book_registry::Book) + Copy + 'static,
        impl Fn(ledger_infra::db::book_registry::Book) + Copy + 'static,
        impl Fn(ledger_infra::db::book_registry::Book) + Copy + 'static,
    >,
) -> View {
    let pop_id = book.get_untracked().id.clone();
    let row_id = pop_id.clone();
    let popconfirm_state = state;
    let pop_id_confirm = row_id.clone();
    let access_disabled = move || !state.mutable.get();
    // 行锚点（旧壳 .book-row click 同位）：按压 = 切换意图请求（活动本在请
    // 求闭包内挡掉）。无障碍角色按钮（name = 账本名）。
    let name_anchor = view()
        .test_id(format!("book-switch-{row_id}").as_str())
        .role(nsh_core::AccessibilityRole::Button)
        .optional_label(move || Some(SharedString::from(book.get().name.clone())))
        .app_class("rounded-(--radius) min-w-0")
        .on_press(move |_: &PressEvent| (actions.switch_request)(book.get_untracked().clone()))
        .child(
            text(move || book.get().name.clone())
                .text_size(px(font_size::SECONDARY))
                .text_color(move || {
                    if (actions.is_active)() {
                        palette.fg
                    } else {
                        palette.muted
                    }
                })
                .test_id(format!("book-row-{index}-name").as_str()),
        );
    let mut row = view()
        .flex_row()
        .app_class("items-center gap-2 py-1 border-b border-border/60");
    row = row.child(name_anchor);
    row = row.child(when(
        move || (actions.is_active)(),
        move || {
            text(BOOKS_PANEL_CURRENT)
                .text_size(px(font_size::CAPTION))
                .text_color(palette.fg)
                .test_id(format!("book-row-{index}-current").as_str())
        },
    ));
    row = row.child(
        view()
            .flex_row()
            .app_class("items-center gap-1")
            .child(
                button()
                    .app_class(
                        "px-2 py-0.5 rounded-sm border border-border text-foreground hover:bg-accent disabled:opacity-40",
                    )
                    .disabled(access_disabled)
                    .child(
                        text(BOOKS_RENAME)
                            .text_size(px(font_size::CAPTION))
                            .test_id(format!("book-rename-{row_id}").as_str()),
                    )
                    .on_press(move |_: &()| (actions.open_rename)(book.get_untracked().clone())),
            ),
    );
    row = row.child(
        when(
            move || !(actions.is_active)() && state.mutable.get(), // 移除入口与登记可用性同门（旧壳 disabled 同口径）
            move || {
                popconfirm::popconfirm(
                    "book-remove",
                    {
                        let pop_id_open = pop_id_confirm.clone();
                        move || {
                            popconfirm_state
                                .popconfirm_open
                                .get()
                                .as_deref()
                                == Some(pop_id_open.as_str())
                        }
                    },
                    {
                        let pop_id = pop_id_confirm.clone();
                        move |shown: bool| {
                            popconfirm_state
                                .popconfirm_open
                                .set(shown.then(|| pop_id.clone()));
                        }
                    },
                    popconfirm::PopconfirmSpec {
                        message: format!(
                            "{BOOKS_REMOVE_CONFIRM_PREFIX}{}{BOOKS_REMOVE_CONFIRM_SUFFIX}",
                            book.get().name
                        ),
                        confirm_label: BOOKS_REMOVE_ACTION.to_owned(),
                        cancel_label: BOOKS_CANCEL.to_owned(),
                    },
                    // 触发件用纯视图（nsh-base Button 自含按压语义会吞掉外
                        // 层按压，预算删除先例同判；封装的按压源是外层包裹视
                        // 图）。
                        view()
                            .app_class(
                                "px-2 py-0.5 rounded-sm border border-border text-foreground hover:bg-accent",
                            )
                            .child(
                                text(BOOKS_REMOVE)
                                    .text_size(px(font_size::CAPTION))
                                    .test_id(format!("book-remove-{row_id}").as_str()),
                            ),
                    move || (actions.remove_row)(book.get_untracked().clone()),
                )
            },
        ),
    );
    row
}
