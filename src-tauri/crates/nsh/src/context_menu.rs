//! 上下文菜单（issue #2022；`App*` 封装族首批件之一，旧壳行右键菜单的 NSH
//! 对应件）。
//!
//! 呈现面是平台原生上下文菜单（`nsh_core::element::desktop::popup_menu_at`，
//! 桌面端在指针处弹出、原生观感与交互语义——ESC/点击外部关闭、选中即收起，
//! ADR-0145 决策 3）：交易表格的行右键事件（`DataTable` 的
//! `DataTableContextMenu`，`x`/`y` 即其文档注明的「`Menu.popup` 的点」）携带
//! 目标行与坐标，本封装只做两件事：
//!
//! 1. 在菜单存续期间经 Overlay Suppression 注册表（[`crate::overlay`]）上报
//!    抑制——弹层（含原生菜单）开着时快捷键不派发（ADR-0035 原生壳重建的
//!    消费面；菜单 token 随菜单关闭（Answer 兑现）经 Drop 自动撤销，与视图
//!    弹层的信号上报通道殊途同归）；
//! 2. 在指定点弹出调用方给的菜单项（词表与业务动作归调用方，本封装零业务
//!    语义）。

use nsh_core::element::desktop::{MenuItem, popup_menu_at};
use nsh_core::prelude::*;

use crate::overlay;

/// 在窗口坐标 `at`（根视图页面坐标，points）弹出上下文菜单 `items`，存续期间
/// 上报快捷键抑制；菜单关闭（选中或收起）后自动撤销上报。
pub fn open_context_menu(items: Vec<MenuItem>, at: Point<Points>) {
    let token = overlay::OverlayToken::new();
    token.report(true);
    // Answer 在菜单关闭（选中或收起）时兑现：兑现即撤销上报（Drop 兜底与
    // 视图弹层的 effect 上报殊途同归）。
    let answer = popup_menu_at(items, at);
    spawn(async move {
        let _ = answer.await;
        drop(token);
    });
}
