# IPC 契约类型护栏（ts-rs / serde fixtures）

本项目**当前不**为 `packages/types` 引入跨语言类型生成（ts-rs / schemars / specta）或 serde fixtures round-trip 校验。这不是永远拒绝，而是**带可机检触发线的搁置**——触发线命中任一即重启（见下）。裁决出处：#1305，2026-09-16 grilling。

## 现状与决定

`packages/types`（26 文件 / ~152 导出，裁决时口径 198 个手抄导出）全手工镜像 Rust 侧 serde 契约，snake_case wire 字段；唯一护栏 `scripts/check-commands.ts` 只做命令名双向全等，载荷字段形状零校验。

2026-09-16 grilling 裁决要点：

1. **响应腿失配的现行风险观测 ≈ 0**：近 400 提交抽样，真 wire 形状改动（Rust+types 双侧同动）9 次 ≈ 每周 3 次，纪律保持率 9/9（同提交双侧更新），孤儿镜像修复 0 例。现行风险由「同提交双侧更新」纪律持有；残余风险是 agent 改 Rust 漏镜像。
2. **重方向（ts-rs derive 导出、构建时生成 `packages/types`）**：防回归边际收益低、清债价值真，但 ADR-0047 决策 4 明文将跨语言类型生成划出范围——重启前必须先修订该 ADR。#1305 原文「ADR 无直接冲突」不成立，以本条为准。
3. **light 方向（serde fixtures / API 响应样本做 TS 侧 round-trip）裁决为被支配选项，不再考虑**：一次性中等成本 + 每个新类型持续缴抽样样本税 + 覆盖有洞（可选字段/枚举变体抽不到）+ 信号仅在 CI 期。

## 重启触发线（命中任一，以 ts-rs 重方向重启，含先修订 ADR-0047 决策 4）

1. **首个「孤儿镜像修复」提交**：改 src-tauri wire（`src-tauri/crates`、`src-tauri/src/commands`）的提交之后，跟着只补 `packages/types/src` 的孤儿提交。验证 = 按 commit 对两侧分桶统计，一次 git 分析可验（先例：#1305 内 2026-09-17 复核）。
2. **手抄税体感上升**：每周双侧同动次数显著超过当前 ~3 次。

## 重启时直接引用的事实沉淀（2026-09-16 侦察）

- `packages/types`：private 零依赖叶子、源码直出不建构建链（ADR-0118）⇒ 生成物必须提交入库 + 新鲜度门禁（ADR-0047 决策 1 在 Rust 注册表上否决过生成物入库路线，但否决理由「Rust 侧有编译期」在 TS 镜像上不成立）；包内有非镜像住客（TRANSACTION_KINDS 等运行时常量/守卫函数）⇒「手写文件退役」只对类型层成立；types 与 api 是 16 包中唯二无 `__tests__` 的包。
- Rust 侧：IPC 载荷全部为具名 serde 类型（~100+，住 18 个域 crate，命令是薄壳）；serde 面无 flatten；tag 枚举 4 处、skip_serializing_if 3 处、double_option 三态（deserialize_with）28 处——后者是 codegen/round-trip 保真度的唯一真坑；utoipa ToSchema 双 derive（6 crate）与 build.rs 命令名级生成先例在，derive+生成有既有通道。
- **请求腿真源不是类型**：Rust 命令参数是散参，绑定靠 fn 签名 + Tauri lowerCamelCase 约定，任何类型 codegen 都看不见它——请求腿参数键名护栏是独立扫描器，已拆出先行（#1398，有 check-commands.ts 先例）。
- HTTP 面契约已被机器锁定（utoipa OpenAPI + contract.rs 测试），TS 镜像只覆盖 IPC 面；事件载荷仅 1 个（InstrumentSyncProgress，带 skip_serializing_if）；失效信号无载荷。

## Prior requests

- #1305: 「架构走查：IPC 契约类型从手抄镜子变成被测面（grilling）」——请求腿拆至 #1398 先行；响应腿搁置（本文件）；light 方向否决。
