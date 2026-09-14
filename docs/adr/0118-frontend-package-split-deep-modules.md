# ADR-0118: 前端 pnpm workspace 拆包——深模块成包裁定、ui-kit 边界与守门规则⑦

- 状态：已接受（spec #1148 子票 #1157 grilling 定稿；第一波 workspace 骨架与底层包由 #1149–#1156、#1158 落地，见各自票；本文裁定第二波——深模块与 ui-kit 的成包取舍；实施 #1314–#1323 逐票交付，落地状态见各票而非本文）
- 日期：2026-09-14
- 作者：Ledger 项目
- 关联：spec #1148（顶层 spec）；设计票 #1157（本文为其验收产出）；实施序：#1314 / #1315 / #1316 / #1317 / #1323（并行首批）→ #1318 / #1319 / #1320（第二批）→ #1321 / #1322（第三批）→ #1159（按域归位收尾）；方法参照 ADR-0112（后端 crate 拆分——本文借其「门禁硬前置、逐包一交付、结构守门为边界知识唯一来源」的方法论，范围独立）；关联 ADR-0083（守门 Bun 运行时）、ADR-0050（错误本地化契约）、ADR-0035（弹层注册表）、ADR-0030/0094（TransactionFilter）、ADR-0040（Loadable）、ADR-0041（ScheduledPlanList）、ADR-0045（TransactionModalState）、ADR-0058（FieldError）、ADR-0072（ModalIntent）、ADR-0077（RowContextMenu）、ADR-0088（WindowTier）、ADR-0093（样式方案）

## 背景

spec #1148 把前端 `src/`（约 8 万行单包、7 节点强连通依赖分量）按「底层共享包先抽、深模块接缝后抽」拆为 pnpm workspace 私有包。第一波已落地：workspace 骨架 + 前端结构守门基线（#1149）、`@ledger/types`（#1150）、`@ledger/i18n` / `@ledger/storage`（#1151）、`@ledger/test-support`（#1152）、`@ledger/money`（#1153）、`@ledger/theme`（#1154）、`@ledger/api`（#1155）、utils 上行违规归位（#1156）、守门脚本测试搬迁（#1158）。

强制手段三层分工（与后端拆 crate 的本质差别——TS 类型系统不检查包边界，编译器不是强制执行者）：pnpm 严格 node_modules 管「能不能解析」，每包 tsconfig 口径管「类型检查范围」，`scripts/check-frontend-structure.ts` 管「方向对不对 + 深导入禁令 + 成员登记」。

剩余待裁定的是深模块接缝的成包取舍（spec 有意留白，#1157 单独开票逐项 grilling）：候选深模块各自带着既有 ADR 禁令（不得 import 组件 / store），但「带禁令」不等于「该成包」；`App*` 封装族与 overlay 注册表（ADR-0035）构成模块级单例族；`useWindowTier`（ADR-0088）被 `vite.config.ts` 构建期按源码路径读取，成包会触碰构建期契约。

## 决策

### 1. 成包判据：对壳零依赖

前端包只许依赖 vue 生态（vue / naive-ui / @vanilla-extract 等）与已登记 `@ledger/*` 包；禁止依赖壳（`src/`）内任何目录——stores / components / composables / views / utils。判据与已落地方向表同构（包只依赖包、不依赖壳），是 workspace 边界的本义；依赖壳内状态的深模块（如依赖 pinia store 者）天然不成包。

### 2. `@ledger/utils` 全量成包，`errors.ts` 随 utils

`src/utils/` 经 #1156 已是零上行违规的叶子层，**20 个文件全量**搬迁为 `@ledger/utils`，一次到位，不做「渐进收留」的劈半中间态（同目录两套导入形态并存的认知与守门成本高于一次性搬迁）。方向 `utils → i18n`：`errors.ts` 的码化错误本地化（ADR-0050）消费 i18n 渲染文案，但它是错误域工具而非翻译机制——i18n 包保持纯翻译机制，不收留 errorMessage。跨包导入一律包名形态，`@/utils/` 不再可达。

### 3. 粒度：每深模块一包

深模块不合并成杂烩包：包无构建链、源码直出，小包固定成本低；方向表粒度即模块边界知识（spec 目标「边界交给工具强制」），与后端「每域一 crate」哲学同构。新增九包：`@ledger/utils`、`@ledger/window-tier`、`@ledger/modal-intent`、`@ledger/row-context-menu`、`@ledger/loadable`、`@ledger/field-errors`、`@ledger/transaction-modal-state`、`@ledger/scheduled-plan-list`、`@ledger/ui-kit`。

### 4. 深模块逐项裁定

| 候选 | 裁定 | 依据 |
| --- | --- | --- |
| TransactionFilter（ADR-0030/0094） | **留壳 + 规则⑦守门**（白名单 = `src/views`） | 依赖壳内 pinia store（交易页会话级 store），判据 1 排除；消费面仅交易页与报表页 |
| Loadable（ADR-0040） | **成包** `@ledger/loadable` | 仅 vue + utils；toast sink 见决策 6 |
| ModalIntent（ADR-0072） | **成包** `@ledger/modal-intent` | 仅 vue |
| TransactionModalState（ADR-0045） | **成包** `@ledger/transaction-modal-state` | deps: modal-intent + api + i18n + utils，判据 1 内；直接 import api 与 useMessage 的既有形态随包保持 |
| RowContextMenu（ADR-0077） | **成包** `@ledger/row-context-menu` | 仅 vue；不接弹层注册表的禁令随包保持 |
| FieldError / useFieldErrors（ADR-0058） | **成包** `@ledger/field-errors` | vue + utils（field-error 归 utils 包） |
| ScheduledPlanList（ADR-0041） | **成包** `@ledger/scheduled-plan-list` | deps: loadable + api + i18n + utils；「不得 import 组件与弹层注册表」禁令随包保持 |
| useWindowTier（ADR-0088） | **成包** `@ledger/window-tier` | 见决策 5 |
| viewResetRegistry（ADR-0061/0094 会话保留重置回调注册表） | **留壳**，#1159 按域落位 | 消费面全在壳（session stores + views），零包消费者，成包无边界收益 |
| i18n 全局 `t()` | 已成包 `@ledger/i18n` | 无需裁定 |

### 5. `@ledger/ui-kit` 成员闭集与 `@ledger/window-tier` 构建期契约

`@ledger/ui-kit` 成员逐项定界，不得自行增删：

- **`App*` 封装 8 件**（DatePicker / Drawer / Dropdown / Modal / Popconfirm / Popover / Select / TreeSelect）+ **overlay 单例族**（`overlayRegistry` + `useOverlayReporting`，ADR-0035 一体族）——8 件封装全部依赖 `useOverlayReporting`，拆开任何一侧都切出自然族；注册表消费面（5 个壳内 composable）全在壳侧，壳 → ui-kit 方向合法，模块级单例随包（决策 6）。
- **通用件 4 项**：`AppDangerConfirmModal`、`PinyinSelect`、`NoteCopyButton`、`app-modal.css.ts`。
- **留壳**：`CreateFab`、`instrument-link.css.ts`、`create-fab.css.ts` 及其余全部 `components/`——域/应用专属，#1159 按域归位。

`useWindowTier` **成包**（消费面 20+ 全在壳，壳 → 包合法；`AppModal` 进 ui-kit 要求其可被包依赖）：`vite.config.ts` 构建期读取路径改为包内文件，断点常量单一来源不变，漂移检查文案同步——构建期契约保留，只换收口点坐标。

### 6. 模块级单例通则：随包 ESM 模块级持有

Loadable 的 toast sink、overlay 注册表等模块级单例：**实例随包 ESM 模块级持有**（现状即此形态，`globalBusy` 归位 `@ledger/api` 已有先例），包对外只暴露注册接口，壳经导入完成接线；不引入注入框架。sink 与策略正交的既有语义不改。

### 7. 不成包者的守门：规则⑦深模块边界登记表

留壳深模块靠 `scripts/check-frontend-structure.ts` 新增**规则⑦「深模块边界登记表」**守门，形制与规则⑥同构：登记 `{ 模块文件 → 允许消费方白名单 }` 闭集，登记表全等断言 + 夹具违规即红 + 删登记项即红。首批登记唯一条目：`useTransactionFilter` → `src/views`。守门规则是包边界之外「目录级边界知识」的唯一事实源。

### 8. 交付序：按依赖分层，同层并行

utils（被最多包依赖）与零依赖包（window-tier / modal-intent / row-context-menu）及守门票先行并行；其次 loadable / field-errors / ui-kit；再 transaction-modal-state / scheduled-plan-list；#1159 前端按域归位在全部包落定后收尾（其目录边界取自本文决策 4/5 的通用件与跨域件结论）。每包一次独立交付、可独立验证、可回退。

## 后果

- 依赖方向由三层工具强制：pnpm 严格隔离（解析）、tsconfig 口径（检查范围）、守门脚本 + 规则⑦（方向与登记）。深模块边界从人记变为登记表与方向表。
- 新增九包的登记与交付固定成本，换取边界知识可证伪；跨包 import 形态改写量大但纯机械，行为等价由既有测试原样跑绿提供（spec Testing Decisions：不新增行为测试接缝，唯一新接缝是规则⑦及其测试）。
- `utils → i18n` 使底层包不再全为零依赖叶子；类型层（`@ledger/types`）仍是唯一恒空方向表。
- 机制术语不进词汇表（ADR-0083 既有口径）；包边界知识落 PACKAGES 登记册与本 ADR，词汇表只保留深模块的领域语义词条（TransactionFilter、Loadable 等）。
