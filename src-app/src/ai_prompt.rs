//! AI 提示词页（NSH 迁移㉔ #2055，旧壳 `AiPromptView.vue` 的重生）：AI 导入
//! 工作流的辅助面——AI 入口提示词全文（markdown 原文）加载与展示、复制到
//! 系统剪贴板与复制反馈；任何写入都发生在账本数据之外（纯展示页，零域调
//! 用、零失效信号）。
//!
//! 数据源（契约单点一份）：提示词模板是 `src-app/prompts/ledger-api.md`
//!（AI 导入契约的入口骨架，include_str! 编译期嵌入——旧壳 `get_ai_prompt`
//! 命令消费同一文件，两壳内容同源零副本，路径即单一事实源）。旧壳加载态/
//! 失败态来自 IPC 往返的时序性呈现；本壳编译期嵌入无失败路径，加载语义平
//! 价为「即显」（loading/获取失败两态在常量数据面不存在，随旧壳契约测试
//! 的骨架锁同源钉在 include 路径上——内容改《契约》票为权威）。
//!
//! 复制真源（平台面）：`nsh_core` 剪贴板写入口（GTK 的 CLIPBOARD 选区，等价
//! 旧壳 `navigator.clipboard.writeText`）；写后回读校验（同一应用进程的自写
//! 自读，结果分成功/失败两态）驱动状态条反馈——旧壳 toast 的状态条等价位
//!（#2038 裁决），headless 可测（nsh-testing `clipboard()` 原语）。复制按钮
//! 在空提示词时禁用（旧壳 disabled 同语义；常量数据面恒非空，防御面平价保
//! 留）。
//!
//! 界面文案仅 zh-CN（旧壳 ai.* zh-CN 词表字面承载）。测试豁免（ADR-0060）：
//! 本模块测试经 crate 根 cfg(test) 整体放行六件套。

use nsh_core::prelude::*;

/// AI 入口提示词全文（markdown 原文，编译期嵌入）。
const AI_PROMPT: &str = include_str!("../prompts/ledger-api.md");

/// 词表（旧壳 ai.* zh-CN 词条原样）。
const VIEW_TITLE: &str = "AI 提示词";
const BUTTON_COPY: &str = "复制";
const MSG_COPIED: &str = "已复制到剪贴板";
const MSG_COPY_FAILED: &str = "复制失败";

/// AI 提示词页（核芯键位 ⌘0 的固定视图，#2032 出厂落点）：标题卡 + 说明 +
/// 提示词正文（等宽 pre 形态）+ 复制按钮 + 复制反馈状态条。
#[component]
pub(crate) fn AiPromptView() -> impl IntoElement {
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
    let ok_fg = AdaptiveColor::new(rgb(0x18a058), rgb(0x63e2b7));

    // 复制反馈状态（None = 无反馈；Some(ok) = 成功/失败两态文案）。
    let copied = signal(None::<bool>);

    // 复制（独立任务：写剪贴板 → 回读校验 → 状态条；同一应用进程的自写自
    // 读，结果分两态——删除回读校验即红钉在 headless 复制反馈旅程）。
    let perform_copy = move || {
        let prompt = AI_PROMPT.to_owned();
        spawn(async move {
            set_clipboard(&prompt);
            let readback = clipboard_string().await;
            let matched = readback == prompt;
            if !matched {
                tracing::warn!("剪贴板回读与提示词原文不一致，复制反馈呈现失败态");
            }
            copied.set(Some(matched));
        });
    };

    view()
        .flex_1()
        .flex_col()
        .gap_3()
        .p_4()
        .app_class("bg-background overflow-y-auto")
        .test_id("ai-prompt-view")
        .child(
            // 标题 + 复制按钮行（旧壳 NCard header-extra 同位）。
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(text(VIEW_TITLE).text_size(px(15.0)).text_color(fg))
                .child(
                    view()
                        .on_press(move |_: &()| perform_copy())
                        .disabled(AI_PROMPT.is_empty())
                        // 空提示词禁用（防御面恒不触发——常量数据面）；禁用态
                        // 退弱化色、可用态主色按钮（旧壳 type=primary 同语义）。
                        .app_class(
                            "px-3 py-1 rounded-md bg-primary hover:opacity-90 disabled:opacity-50",
                        )
                        .child(
                            text(BUTTON_COPY)
                                .text_size(px(13.0))
                                .text_color(rgb(0xffffff))
                                .test_id("ai-copy"),
                        ),
                ),
        )
        .child(
            // 页面说明（旧壳 ai.description 同词）。
            text(DESCRIPTION).text_size(px(13.0)).text_color(muted),
        )
        .child(
            // 提示词正文（markdown 原文，等宽小字 pre 形态；font-mono 是
            // 缺省主题令牌的等宽字体家族名）。
            view()
                .flex_col()
                .app_class("rounded-md border border-border bg-background p-3")
                .child(
                    text(move || AI_PROMPT.to_owned())
                        .font_family("font-mono")
                        .text_size(px(12.0))
                        .text_color(fg)
                        .test_id("prompt-body"),
                ),
        )
        .child(
            // 复制反馈状态条（成功绿 / 失败红；无反馈不占位）。
            when(
                move || copied.get().is_some(),
                move || {
                    text(if copied.get_untracked() == Some(true) {
                        MSG_COPIED
                    } else {
                        MSG_COPY_FAILED
                    })
                    .text_size(px(12.0))
                    .text_color(move || {
                        if copied.get_untracked() == Some(true) {
                            ok_fg
                        } else {
                            danger_fg
                        }
                    })
                    .test_id("ai-copy-status")
                },
            )
            .otherwise(view),
        )
}

/// 页面说明（旧壳 ai.description 同词；「需保持开源记账运行」随 ADR-0076
/// 显示名的措辞在位）。
const DESCRIPTION: &str = "将以下提示词复制给 AI 编程助手（如 Cursor、Claude Code），它会据此通过本地 HTTP API（127.0.0.1:9527）读取与写入账本数据；期间需保持开源记账运行。";
