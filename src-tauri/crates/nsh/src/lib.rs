// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块与外挂 tests.rs）经 crate 根 cfg(test) 整体
// 放行，生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

//! NSH 原生壳（issue #2017 骨架 / issue #2018 交易表格首屏，ADR-0145 拆票①，spec #2015）。
//!
//! 双壳并存期的第一块桩：以 NSH（Native Shell）Rust 模式承载的最小壳竖井——
//! 启动参数给账本目录，引导域栈打开**未加密**账本库（无解锁向导），就绪窗口
//! 以 DataTable 渲染真实交易数据（100 行/页服务端 offset 分页走域读路径、
//! 分页条点击翻页、搜索框接入服务端统一模糊搜索、忙碌指示与空态）。读路径
//! 直调域服务（`ledger-transaction` 读路径），不经任何 IPC 命令边界——ADR-0145
//! 决策 4 的接缝形态立在此处；Tauri 旧壳零改动、保持唯一发版产品。
//!
//! 依赖方向沿用壳 → 域 → 基础设施 → 协议（ADR-0112）：本 crate 是壳层 bin
//! crate（结构守门政策核 CRATES 登记在册），只消费域 crate 与基础设施，不定义
//! 账本语义；金额与折算口径以核心交易域为唯一真源，本壳只消费不另立。
//!
//! 模块地图：
//! - [`app`]：NSH 根组件与启动结果视图模型（装配入口可注入，headless 可测）；
//! - [`nav`]：进程内导航 API 与导航状态（#2032 壳导航骨架）——单点入口
//!   `open`（守卫裁决 → 落位 → 持久化）、下钻意图读一次消费、键位带派发、
//!   侧栏序/收纳清单/关闭集合/上次视图的存储读写（`nsh_core::storage::Storage`
//!   平台文件侧 KV，键值形状镜像旧壳 `view_state:*` 族、不与旧壳共享、不进
//!   SQLite）；
//! - [`nav_model`]：导航纯模型（#2032）——视图/分组闭集与词表、侧栏出厂定义、
//!   持久化载荷解析防御（旧壳 parse 家族重生）、键位带推导（ADR-0065 固定组
//!   带）、守卫落位裁决（旧壳路由守卫族重生）、跳转目标计算（旧壳 source-jump
//!   重生）与移动纯函数；
//! - [`sidebar`]：侧栏（#2032）——品牌行、固定项、三组导航、组「更多」链接、
//!   键位提示与右键排序菜单；
//! - [`views`]：壳内视图层（#2032）——未迁移视图空态占位（下钻接缝示范消费
//!   面）与组「更多」容器页签壳；
//! - [`assembly`]：装配层——行集进入表格前产出每行每格最终展示文本与列定义
//!   （纯 Rust、无 UI 依赖；#2019 全家桶：数字分组、金额语义色、隐私遮蔽、
//!   分类路径、备注列、通用 kind 标签；#2020 派生行形态：购买项逐项行与订单级
//!   格首行语义、双账户/出资分解标注、来源名与状态、借贷派生标签、商户名；
//!   #2022 删后页码决策）；
//! - [`period`]：时间维度行——期间游标/步进/预设/边界钳制的纯决策逻辑
//!   （旧壳 time-period.ts 的重生，#2021 / ADR-0141）；
//! - [`overlay`]：Overlay Suppression 注册表——弹层开合显式上报、快捷键
//!   派发前的共用闸门（旧壳 ADR-0035 语义的原生壳重建，#2021；#2022 上下文
//!   菜单与危险确认对话框消费）；
//! - [`context_menu`]：上下文菜单封装（`App*` 封装族首批件，#2022）——原生
//!   弹出 + 存续期间抑制上报；
//! - [`danger`]：危险确认对话框封装（`App*` 封装族首批件，#2022）——
//!   ADR-0078 error/warning 两级形态、遮罩不关/ESC 可关、抑制上报自含；
//! - [`signals`]：进程内失效信号——交易数据失效的订阅/广播单点，写路径
//!   成功后广播、列表视图订阅重读（#2022；词汇表失效信号语义的壳内通道）；
//! - [`boot`]：域栈引导——库文件处置判定（复用基础设施引导层）、成对建连、
//!   交易列表页与币种字典读取、交易删除（#2022）；
//! - [`wiring`]：域接缝接线——读路径全部读时投影接缝 + 交易写入协议的余额刷新
//!   接缝，与旧壳启动接线同形（ADR-0112 决策 5）。
//!
//! 测试权威（ADR-0087 断言归属在本壳的落点）：装配层格文本/列定义/分页边界归
//! [`assembly`] 单测；壳行为（引导与读接线）归本 crate 的 lib 单测；视图树接线
//! （窗口呈现）归 headless 组件测试（`tests/app_headless.rs`,
//! docs/research/nsh-testing-capabilities.md §3.1）；域语义归各域单测，不在本壳
//! 重复。

pub mod app;
pub mod boot;
pub mod context_menu;
pub mod danger;
pub mod nav;
pub mod nav_model;
pub mod overlay;
pub mod period;
pub mod sidebar;
pub mod signals;
pub mod views;
pub mod wiring;

mod assembly;

#[cfg(test)]
mod tests;
