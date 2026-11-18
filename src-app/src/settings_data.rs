//! 数据安全与连接卡宿主（issue #2054 NSH 迁移㉓；旧壳 `EncryptionSettings` /
//! `SyncSettings` + `useSyncCard` / `useBookSwitcher` 的重生，ADR-0022 数据
//! 页签 / ADR-0078 危险确认 / ADR-0091 多端同步 / ADR-0114 跨账本）。
//!
//! #2167 拆分：备份管理五卡（`BackupSettings` 全量面，超越 #2038 最小入
//! 口——动作 / 目录 / 自动 / 列表 / 恢复卡）已按职责归位
//! [`crate::backup_cards`]；本模块收敛为数据页签装配与卡间共用件。
//!
//! **卡片组**（挂 [`crate::settings`] 数据页签的卡片宿主与状态条位，容器面
//! 单点不变， [`DataSafetyCards`] 为入口组件）：
//! - **加密**（`EncryptionSettings` 全量面，#2088 写面平价）：明文库形态 =
//!   warning 提示 + 主口令两框 + ≥8 位 / 一致守卫 + error 级无后门确认；密
//!   文会话形态（已开启 + 修改主口令 / 关闭加密 / 自动解锁注记）：修改主
//!   口令 error 级确认（旧壳同分级）、关闭加密 warning 级确认（破坏性但
//!   有兑底，旧壳同分级）、自动解锁按 Linux 平台不支持在屏注记（旧壳非
//!   macOS 隐藏选项的同款平价）。三形态转换完成后原位换连本会话继续可用
//!   （旧壳「应用自动重启」的本壳替代口径，无重启丢写窗口）；口令强度实
//!   时条豁免留痕（zxcvbn Rust 依赖的字典体积对单二进制的增量，守卫闭集
//!   ≥8 位 / 两次一致保留，CHANGELOG 偏差条目）。
//! - **多端同步**（`useSyncCard`）：明文警示 / 状态回显（上次同步 / 挂起
//!   数量与明细）、立即同步（成功 → 轮次报告反馈 + 失效信号广播）、通道配
//!   置表单（S3 七字段 + 同步空间 + 厂商预设预填 / 端点反查回显 + 密钥不回
//!   显口径）、保存前「测试连接」与检查点发布 / 新端引导向导。同步写语义
//!   不经本壳（轮次 / 发布 / 引导全在域内单点编排，本壳只解包口令与触发，
//!   票面实施决策）；不依赖同步协议 crate（壳只依赖域，ADR-0112）。
//! - **账本切换**（`useBookSwitcher`）：清单卡（跨账本汇总**置顶入口** +
//!   行按压切换〔warning 级轻量确认〕+ 行级改名弹窗 / 移除气泡确认 + 新建
//!   弹窗），清单读注册表最新落盘态；切换写活动指针，本壳无原位重引导
//!   （旧壳 restart_app 自动重载的偏差不在屏披露，反馈句与侧栏 #2129
//!   干净变体同文）。
//!
//! 动作反馈落卡内状态条位（#2038 裁决，NSH 无 toast）；阻塞 IO（加密转
//! 换、同步网络段、注册表读写）一律 `blocking` 后台执行；信号读写都在引
//! 擎 UI 线程。界面文案仅 zh-CN（旧壳 settings.* / books.* zh-CN 词条原样
//! 承载）；码化错误的中文原样呈现。跨卡共用件两族住本模块：呈现件（密文
//! 输入行 [`pass_input`] / 表单字段行 [`field_input`]）与装配纯函数（备份
//! 产物大小 / 时刻格式化 [`format_backup_size`] / [`format_backup_time`]，
//! [`crate::backup_cards`] 与同步卡共用）。
//!
//! 弹窗条件段机制选型（#2146 定案，行为零变化）：**布尔有无 / 分支翻转 →
//! `when`**（nsh-core `flow.rs` `Region::If`：条件翻转入支重造、离支销毁，
//! 分支闭包 untracked 求值，内容活性靠 reactive 绑定逐次求值）；**多态内容
//! 匹配或整段重造 → `dynamic`**（`Region::Dynamic`：读什么变什么重造）。
//! 全部弹窗条件段按此归位（各卡弹窗各族段均为布尔有无 → `when` 同款；
//! 恢复确认弹窗的机制注记随备份卡住 [`crate::backup_cards`]。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use crate::backup_cards;

use nsh_base::{button, input};
use nsh_core::element::FontWeight;
use nsh_core::prelude::*;

use crate::app::book_dir;
use crate::danger::{DangerConfirmSpec, DangerLevel, DangerOutcome, danger_confirm};
use crate::design::font_size;
use crate::encryption_card;
use crate::modal::app_modal;
use crate::nav_model::NavRequest;
use crate::popconfirm;
use crate::settings::{card, status_line};
use crate::sync_card;
use crate::shell_msgs::MSG_WRITE_TASK_FAILED;
use crate::write_load::session_missing_err;
use crate::{boot, nav, nav_model, settings, views};

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
// 装配层（纯函数与纯投影；渲染期零格式化——行文本已是装配产物）。
// ---------------------------------------------------------------------------

/// 备份产物大小展示（旧壳 formatSize 同口径：B / ×1 小数 KB / MB）。
pub(crate) fn format_backup_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// 备份时刻展示（旧壳 formatIsoMinute 同口径：ISO 剥到分钟 `YYYY-MM-DD
/// HH:mm`；形状不符原样透传，不做第二份口径的格式化猜测）。
pub(crate) fn format_backup_time(iso: &str) -> String {
    let bytes: Vec<char> = iso.chars().collect();
    match (bytes.get(10), bytes.get(16)) {
        (Some('T'), _) if bytes.len() >= 16 => {
            let date: String = bytes[..10].iter().collect();
            let clock: String = bytes[11..16].iter().collect();
            format!("{date} {clock}")
        }
        _ => iso.to_owned(),
    }
}



// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// 共用呈现件（密文输入行 / 表单字段行）；破坏色便捷面收
// [`crate::views::ViewPalette`] 的 `danger_fg` 成员（通面五件套唯一住所）。
// ---------------------------------------------------------------------------

/// 密文输入行（标签 + masked 输入框；旧壳 NInput type=password 对应件）。
pub(crate) fn pass_input(
    target: Signal<String>,
    palette: views::ViewPalette,
    id: &'static str,
    label: &'static str,
    placeholder: &'static str,
) -> View {
    view()
        .flex_col()
        .app_class("gap-1")
        .child(
            text(label)
                .text_size(px(font_size::CAPTION))
                .text_color(palette.fg),
        )
        .child(
            input()
                .test_id(id)
                .value(move || target.get())
                .masked(true)
                .placeholder(placeholder)
                .optional_label(label)
                .text_color(palette.fg)
                .w(px(260.0))
                .app_class(
                    "bg-background rounded-(--radius) border border-border px-2 py-1 placeholder:text-muted-foreground",
                )
                .on_change(move |value: &str| target.set(value.to_owned())),
        )
}

/// 通道表单普通字段行（占位即标签；无障碍名 = 占位原文）。
pub(crate) fn field_input(
    palette: views::ViewPalette,
    id: &'static str,
    placeholder: &'static str,
    target: Signal<String>,
    masked: bool,
    dynamic_placeholder: Option<impl Fn() -> &'static str + Copy + 'static>,
) -> View {
    let width = if masked { px(260.0) } else { px(420.0) };
    let base = input()
        .test_id(id)
        .value(move || target.get())
        .optional_label(placeholder)
        .text_color(palette.fg)
        .w(width)
        .app_class(
            "bg-background rounded-(--radius) border border-border px-2 py-1 placeholder:text-muted-foreground",
        )
        .on_change(move |value: &str| target.set(value.to_owned()));
    let base = match dynamic_placeholder {
        Some(provider) => base.placeholder(provider),
        None => base.placeholder(placeholder),
    };
    let base = if masked { base.masked(true) } else { base };
    view().child(base)
}


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
fn books_card(palette: views::ViewPalette) -> View {
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

/// 数据安全四卡入口（数据页签消费面）。
#[component]
pub fn DataSafetyCards() -> impl IntoElement {
    let palette = views::view_palette();
    view()
        .flex_col()
        .app_class("gap-3")
        .child(backup_cards::backup_cards(palette))
        .child(encryption_card::encryption_card(palette))
        .child(sync_card::sync_card(palette))
        .child(books_card(palette))
        .child(balance_cache_card(palette))
}

// ---------------------------------------------------------------------------
// 余额缓存审计修复卡（issue #2136 设置③·数据修复；旧壳 BalanceCacheSettings.vue
// 的重生，ADR-0067 决策 5「手动审计兑底」的界面入口）：账户余额与净资产读
// 持久化缓存（V017），缓存缺失按裁决报码化错误、不静默回退实时计算——修复
// 动作不能藏在读路径里，只能由此处显式触发。修复语义全在域内单正文
//（[`ledger_accounts::audit_balance_cache`]：全账户实时重算 vs 缓存逐户比对
// → 整体重算回写 → 返回差异快照，幂等可重复），本卡只触发命令与呈现报告，
// 不持第二份口径。
// ---------------------------------------------------------------------------

const CARD_BALANCE_CACHE: &str = "账户余额缓存";
const BALANCE_HINT: &str = "账户余额与净资产读的是持久化缓存。缓存行缺失或与实时重算不一致时，相关页面会直接报错而不回退实时计算。一键修复会逐户比对并整体重算回写，返回修复前的差异快照，可重复执行。";
const BALANCE_REPAIR: &str = "一键修复";
const BALANCE_REPORT_DONE_TITLE_PREFIX: &str = "修复完成：已校准 ";
const BALANCE_REPORT_DONE_TITLE_SUFFIX: &str = " 个账户";
const BALANCE_REPORT_DONE_BODY: &str = "差异账户已按实时口径整体重算回写，余额与净资产恢复一致。";
const BALANCE_REPORT_NOOP_TITLE: &str = "无需修复";
const BALANCE_REPORT_NOOP_BODY_PREFIX: &str = "全部 ";
const BALANCE_REPORT_NOOP_BODY_SUFFIX: &str = " 个账户的缓存与实时重算一致，未做改动。";
const BALANCE_MISSING_TAG: &str = "（缓存行缺失）";
const BALANCE_DRIFT_COLUMNS: [&str; 3] = ["账户", "修复前缓存", "实时重算"];
const BALANCE_REPAIR_FAILED_PREFIX: &str = "修复失败: ";

/// 修复报告的呈现投影（旧壳 reportView computed 同口径：有漂移 → 已修复
///（列出逐户差异）；零漂移 → 无需修复。命令返回的报告恒为「修复已完成后的
/// 差异快照」，不存在「未修复」态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BalanceCacheReportView {
    /// 横幅标题。
    pub title: String,
    /// 横幅正文。
    pub body: String,
    /// 逐户差异行（None = 零漂移，表格不呈现）。
    pub drifts: Option<Vec<(String, String, String)>>,
}

/// 域审计报告 → 呈现投影（差异行的三列文本在装配期定格：账户名 / 缓存值
///（缺失独立文案，不显示成 0）/ 实时重算值〔按账户币种格式化，币种反查
/// 自同批读的账户行集，未收录退化为无币种格式化——旧壳 reference 未收录
/// 同口径〕）。渲染期零格式化。
pub(crate) fn balance_cache_report_view(
    report: &ledger_accounts::BalanceCacheAudit,
    currencies: &crate::assembly::CurrencyTable,
    currency_of: &std::collections::BTreeMap<String, String>,
) -> BalanceCacheReportView {
    if !report.drifts.is_empty() {
        let drifts = report
            .drifts
            .iter()
            .map(|drift| {
                let code = currency_of
                    .get(&drift.account_id)
                    .cloned()
                    .unwrap_or_default();
                let fmt = currencies.format_of(&code);
                let cached = drift
                    .cached_cents
                    .map(|cents| crate::assembly::format_amount_basic(cents, &fmt))
                    .unwrap_or_else(|| BALANCE_MISSING_TAG.to_owned());
                (
                    drift.account_name.clone(),
                    cached,
                    crate::assembly::format_amount_basic(drift.actual_cents, &fmt),
                )
            })
            .collect();
        BalanceCacheReportView {
            title: format!(
                "{BALANCE_REPORT_DONE_TITLE_PREFIX}{}{BALANCE_REPORT_DONE_TITLE_SUFFIX}",
                report.drifts.len()
            ),
            body: BALANCE_REPORT_DONE_BODY.to_owned(),
            drifts: Some(drifts),
        }
    } else {
        BalanceCacheReportView {
            title: BALANCE_REPORT_NOOP_TITLE.to_owned(),
            body: format!(
                "{BALANCE_REPORT_NOOP_BODY_PREFIX}{}{BALANCE_REPORT_NOOP_BODY_SUFFIX}",
                report.accounts_checked
            ),
            drifts: None,
        }
    }
}

/// 余额缓存审计修复卡（数据页签卡组末位）。
fn balance_cache_card(palette: views::ViewPalette) -> View {
    let owner = Owner::current();
    let report = signal(None::<BalanceCacheReportView>);
    let status = signal(None::<String>);
    let repairing = signal(false);

    // 一键修复（幂等可重复；报告就地覆盖呈现）。删触发接线（boot 审计调用
    // 或卡内任务）→ 报告永不出现 → headless 旅程红。
    let run_repair = move || {
        if repairing.get_untracked() {
            return; // 单飞兜底
        }
        repairing.set(true);
        status.set(None);
        owner.run(move || {
            spawn(async move {
                let db = crate::app::db_slot().clone();
                // 参考币种行集与审计同批读（差异行金额格式化的币种反查源；
                // 读失败退化为无币种格式化——呈现面降级，修复本身不受影响）。
                let outcome = blocking(move || match db.as_ref() {
                    Some(db) => {
                        let currencies = crate::assembly::CurrencyTable::from_currencies(
                            crate::boot::currency_rows(db).unwrap_or_default().iter(),
                        );
                        // 差异行金额格式化的账户币种反查源（读失败退化为无
                        // 币种格式化——呈现面降级，修复本身不受影响）。
                        let currency_of = crate::boot::account_rows(db)
                            .unwrap_or_default()
                            .into_iter()
                            .map(|a| (a.id, a.currency_code))
                            .collect::<std::collections::BTreeMap<String, String>>();
                        boot::audit_balance_cache(db).map(|audit| {
                            balance_cache_report_view(&audit, &currencies, &currency_of)
                        })
                    }
                    None => Err(session_missing_err()),
                })
                .await;
                repairing.set(false);
                match outcome {
                    Ok(Ok(view)) => {
                        let done = view.title.clone();
                        report.set(Some(view));
                        status.set(Some(done));
                    }
                    Ok(Err(e)) => status.set(Some(format!("{BALANCE_REPAIR_FAILED_PREFIX}{e}"))),
                    Err(_) => status.set(Some(format!(
                        "{BALANCE_REPAIR_FAILED_PREFIX}{MSG_WRITE_TASK_FAILED}"
                    ))),
                }
            });
        });
    };

    // 报告横幅（有报告时在屏；错误优先呈现于状态条位，报告保持上次成功面）。
    let report_row = view().child(
        when(
            move || report.get().is_some(),
            move || {
                let snapshot = report
                    .get_untracked()
                    .unwrap_or_else(|| BalanceCacheReportView {
                        title: String::new(),
                        body: String::new(),
                        drifts: None,
                    });
                let mut block = view()
                    .flex_col()
                    .app_class("rounded-sm border border-border bg-accent p-2 gap-1")
                    .child(
                        text(snapshot.title)
                            .text_size(px(font_size::CAPTION))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.fg)
                            .test_id("balance-repair-report"),
                    )
                    .child(
                        text(snapshot.body)
                            .text_size(px(font_size::CAPTION))
                            .text_color(palette.muted)
                            .test_id("balance-repair-body"),
                    );
                // 逐户差异表（零漂移不呈现）。
                if let Some(drifts) = snapshot.drifts
                    && !drifts.is_empty()
                {
                    let mut table = view()
                        .flex_col()
                        .app_class("gap-0.5")
                        .test_id("balance-cache-drifts")
                        .child(view().flex_row().app_class("gap-3").children(
                            BALANCE_DRIFT_COLUMNS.map(|title| {
                                text(title)
                                    .text_size(px(font_size::CAPTION))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette.fg)
                            }),
                        ));
                    for (account, cached, actual) in drifts {
                        table = table.child(
                            view()
                                .flex_row()
                                .app_class("gap-3")
                                .child(
                                    text(account)
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(palette.fg),
                                )
                                .child(
                                    text(cached)
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(palette.muted),
                                )
                                .child(
                                    text(actual)
                                        .text_size(px(font_size::CAPTION))
                                        .text_color(palette.fg),
                                ),
                        );
                    }
                    block = block.child(table);
                }
                block
            },
        )
        .otherwise(view),
    );

    card(
        CARD_BALANCE_CACHE,
        palette,
        vec![
            view().child(
                text(BALANCE_HINT)
                    .text_size(px(font_size::CAPTION))
                    .text_color(palette.muted),
            ),
            report_row,
            view().flex_row().app_class("items-center gap-2").child(
                nsh_base::button()
                    .app_class(
                        "px-3 py-1.5 rounded-(--radius) bg-primary hover:opacity-90 disabled:opacity-40",
                    )
                    .disabled(move || repairing.get())
                    .on_press(move |_: &()| run_repair())
                    .child(
                        text(BALANCE_REPAIR)
                            .text_size(px(font_size::SECONDARY))
                            .test_id("balance-repair"),
                    ),
            ),
            view().child(status_line("balance-repair-status", status, palette)),
        ],
    )
}

// ---------------------------------------------------------------------------
// lib 单测：装配层纯面（大小 / 时刻格式化、来源 / 设备词表、厂商预设闭集
// 与反查）。headless 组件测试见 tests/app_headless.rs 设置②节。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 大小格式化闭集：B / KB（一位小数）/ MB（一位小数）三段。
    #[test]
    fn backup_size_covers_unit_closure() {
        assert_eq!(format_backup_size(512), "512 B");
        assert_eq!(format_backup_size(2048), "2.0 KB");
        assert_eq!(format_backup_size(1536 * 1024), "1.5 MB");
    }

    /// 时刻格式化：ISO 剥到分钟（YYYY-MM-DD HH:mm）；形状不符原样透传。
    #[test]
    fn backup_time_projects_iso_minute() {
        assert_eq!(
            format_backup_time("2026-01-01T10:20:30Z"),
            "2026-01-01 10:20"
        );
        assert_eq!(format_backup_time("非ISO形状"), "非ISO形状");
    }

    /// 修复报告投影：有漂移 → 「已修复」横幅 + 逐户差异行（金额按账户币种
    /// 格式化）；零漂移 → 「无需修复」横幅、无差异表。
    #[test]
    fn balance_cache_report_view_projects_done_and_noop() {
        let currencies = crate::assembly::CurrencyTable::from_currencies(
            [ledger_currencies::Currency {
                code: "CNY".to_owned(),
                name: "人民币".to_owned(),
                symbol: "¥".to_owned(),
                decimal_places: 2,
            }]
            .iter(),
        );
        let mut currency_of = std::collections::BTreeMap::new();
        currency_of.insert("acc-1".to_owned(), "CNY".to_owned());

        let drift_report = ledger_accounts::BalanceCacheAudit {
            accounts_checked: 3,
            repaired: true,
            drifts: vec![ledger_accounts::BalanceCacheDrift {
                account_id: "acc-1".to_owned(),
                account_name: "现金".to_owned(),
                cached_cents: Some(5_000),
                actual_cents: 8_000,
            }],
        };
        let view = balance_cache_report_view(&drift_report, &currencies, &currency_of);
        assert_eq!(view.title, "修复完成：已校准 1 个账户");
        let drifts = view.drifts.unwrap();
        assert_eq!(
            drifts[0],
            ("现金".to_owned(), "¥50".to_owned(), "¥80".to_owned())
        );

        let noop_report = ledger_accounts::BalanceCacheAudit {
            accounts_checked: 2,
            repaired: false,
            drifts: Vec::new(),
        };
        let view = balance_cache_report_view(&noop_report, &currencies, &currency_of);
        assert_eq!(view.title, BALANCE_REPORT_NOOP_TITLE);
        assert!(view.drifts.is_none());
        assert!(view.body.contains("2 个账户"));
    }



    /// 差异行的金额列：缓存缺失（None）独立文案呈现，不显示成 0（旧壳
    /// cachedText 同口径）；账户币种未收录退化为无币种格式化。
    #[test]
    fn drift_rows_project_missing_cache_and_unlisted_currency() {
        let currencies = crate::assembly::CurrencyTable::default();
        let currency_of = std::collections::BTreeMap::new();
        let report = ledger_accounts::BalanceCacheAudit {
            accounts_checked: 1,
            repaired: true,
            drifts: vec![ledger_accounts::BalanceCacheDrift {
                account_id: "acc-x".to_owned(),
                account_name: "孤儿账户".to_owned(),
                cached_cents: None,
                actual_cents: 1_234,
            }],
        };
        let view = balance_cache_report_view(&report, &currencies, &currency_of);
        let drifts = view.drifts.unwrap();
        assert_eq!(
            drifts[0].1, BALANCE_MISSING_TAG,
            "缺失列独立文案，不显示成 0"
        );
        assert_eq!(drifts[0].2, "12.34", "未收录币种退化为基础格式化（无币符）");
    }
}
