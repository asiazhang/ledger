# ADR 0035: 弹层抑制改响应式注册表——「弹层是否打开」显式上报，不做 DOM 推断

- 状态：已接受
- 日期：2026-12-19
- 关联：取代 ADR-0021 的机制部分（其事实教训保留）；Context：界面状态与交互域词汇表 Overlay Suppression 词条

## 背景

视图快捷键（Cmd/Ctrl+1..9）与记一笔快捷键（裸键 a/z/i/b/s）共用弹层抑制判定 `hasOpenOverlay()`。该判定历经三个版本，前两版同因「对 Naive UI 渲染策略做错误假设」而翻车：

1. **v0（容器存在性）**：嗅探 `.n-modal-container` 等。Naive UI 的 `VLazyTeleport` 用 `useFalseUntilTruthy`——容器首次显示后永久残留 DOM，第一次关弹窗后快捷键即永久失效。
2. **v1（ADR-0021，遮罩 + 白名单存在性）**：改嗅探 `.n-modal-mask`，并维持 `.n-base-select-menu`、`.n-dropdown-menu`、`.n-date-panel` 信号集。当时断言「筛选下拉基于 Follower + v-if，关闭即卸载，存在性本就可靠」——**对 NSelect 不成立**：NSelect 的 `displayDirective` 默认值是 `'show'`（naive-ui `select/src/Select.mjs`），菜单首次打开后仅 `v-show` 隐藏、节点永不卸载。于是「交易页任一筛选下拉（账户/商户 PinyinSelect、类型 NSelect）用过一次，`.n-base-select-menu` 就永久残留 body」，`hasOpenOverlay()` 恒 true，两套快捷键静默失效且不可恢复（本次 bug，最常见复现：筛选中选一项并关闭）。NDatePicker/NDropdown/NPopconfirm 关闭即卸载，非残留源；焦点残留（关闭后焦点在 selection wrapper div，非可编辑元素）经运行时实验排除，不是共因。
3. **v2（本决策，注册表）**：嗅探路线的根本问题是**把「弹层是否打开」这个应用状态交给 DOM 推断**，而推断所依赖的「class 名 ⟺ 打开状态」契约由第三方库的渲染细节隐式提供，既不稳定也不完备——每换一种弹层形态就要手动养一次信号集（`.n-date-panel` 当年即手动补入），且失效模式是静默永久失效。

## 决策

**新建 `src/composables/overlayRegistry.ts` 响应式注册表**：模块级 `Set<token>`，弹层封装组件在实例作用域持有 token，随自身 show 状态上报开/关（`set` 幂等）；`hasOpenOverlay()` 读注册表，`OVERLAY_SELECTORS` DOM 嗅探整体删除。

上报接线收口在**弹层封装组件**（延续 AppModal 收口惯例，本轮新增/扩展六个）：

| 封装 | 覆盖 |
| --- | --- |
| `AppModal`（已有，扩展） | 全部模态弹窗 |
| `AppSelect`（新） | 裸 NSelect 全部用点 |
| `AppTreeSelect`（新） | NTreeSelect（其菜单关闭即卸载，非残留源，纳入是为语义统一：弹层开着就拦） |
| `AppDatePicker`（新） | NDatePicker 日历面板 |
| `AppDropdown`（新） | NDropdown（含侧栏排序手动触发菜单） |
| `AppPopconfirm`（新） | NPopconfirm 气泡确认 |
| `useAppDialog`（新，包 useDialog） | 命令式确认框——打开即上报，`onAfterLeave`（离场动画完成、确定关闭）时撤销 |

接线要点（踩过验证过的坑，后来者勿重蹈）：

- **封装组件刻意不声明 `show` prop**。类型-only 的 `show?: boolean` 经 Vue 的 Boolean 缺席转型，未传时解析为 `false` 而非 `undefined`；把它绑回根组件会把非受控用法变成「受控关闭」，下拉永远打不开（编译器对 `boolean | undefined` 也不生成 `default: undefined`）。`:show`/`@update:show` 一律走 attrs 透传（单根 fallthrough 的事件合并语义，AppModal 生产先例）。
- 上报双通道：根组件上的 `@update:show` 监听覆盖非受控开合与受控下组件内部触发的关闭；`watch(attrs.show)` 兜底覆盖受控调用方直接改 prop 的开合（如 `trigger="manual"` 的侧栏排序菜单）。`set` 幂等，双通道重复上报无副作用。
- token 必须在组件实例作用域创建（每实例一个），禁止模块级共享——同类弹层多实例并存时共享 token 会互相污染开关状态。
- 失效模式由此反转：未来漏接线（绕过封装直接用裸组件）的症状是「弹层开着时快捷键误触发」——当场可见、当场修；不再是静默永久失效。

## 候选方案对比

| 方案 | 结论 |
| --- | --- |
| A. 存在性嗅探 + 可见性检查（`getClientRects`） | 否——仍是 DOM 推断，信号集仍要人工维护，治标 |
| B. 全部 NSelect 设 `displayDirective: 'if'` 恢复「关闭即卸载」 | 否——要记住给每个用点加，且 class 名耦合与信号集维护成本原样保留 |
| **C. 响应式注册表（采纳）** | 语义最正：状态显式声明、可测、与 AppModal 收口惯例同构 |
| D. 统一声明式快捷键分发器（三处监听合一 + when 谓词） | 暂缓——与抑制信号源正交，注册表落地后如快捷键继续增多再议 |

ADR-0021 曾以「改动全部弹层使用点，成本高且新弹层容易漏注册」否决注册表（其方案 C）。本次推翻该权衡的前提已变：其一，嗅探路线实际发生了第二次静默全灭故障，代价被证明远高于预估；其二，「漏注册」的失效模式经设计反转为即时可见的误触发，可接受；其三，封装组件收口后新增弹层天然带上报，与「弹窗一律走 AppModal」的既有惯例同一条纪律。

## 代价与边界

- 封装组件是新契约：**应用内弹层一律经 App* 封装/useAppDialog 使用**，绕过封装直接用裸组件会脱离抑制（症状为误触发，可见可修）。已写入 AGENTS.md 编码约定。
- naive-ui 的 `update:show` 事件是接线前提（四类组件均已验证 emit；DatePicker 面板同理）。若某弹层形态无该事件，需在对应封装内另找等价信号——仍收口在封装内，不回到全局嗅探。
- 弹层关闭淡出动画期间（`onAfterLeave` 前）仍视为打开，属预期行为而非缺陷。
- 测试策略随之改变：抑制类单测从「手工造 DOM 元素」改为「token 上报」；新增真实交互回归（真实 PinyinSelect 开→选→关，注册表必须归零），这正是历次残留 bug 的测试盲区。
