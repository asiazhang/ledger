# ADR-0123: push-first 清单生命周期工厂——四店机制塌缩为 createPushFirstList 单点，写后重拉失败统一不反转写动作成败

- 状态：已接受（grilling 定稿，2026-09-16；实现随 #1302 落地）
- 日期：2026-09-16
- 作者：Ledger 项目
- 关联：ADR-0012（参考失效信号与 stale-while-revalidate 重拉语义——本工厂是其实现载体，领域规则不变）；ADR-0040（Loadable——平行的单任务生命周期路线，本工厂不收编）；ADR-0041（ScheduledPlanList——「计划清单」专名划界与「特化留适配器」纪律同构）；ADR-0064（实物资产域——写后重拉失败语义的裁决出处 #467）；ADR-0085 / ADR-0087（行为等价判据、断言强度与测试归属）；ADR-0118（不成包先例 viewResetRegistry、「机制术语不进词汇表」口径）；词汇表：核心交易域「失效信号」、参考数据与设置域「Reference Data」

## 背景

四个前端领域 store——物品域 `useItemsStore`、保险域 `usePoliciesStore`、实物资产域 `usePhysicalAssetsStore`、参考数据域 `useReferenceStore`——各自手写同一套 push-first 清单生命周期：status 四态（idle/loading/ready/error）+ version 成功计数 + inFlight 在途合并 + stale-while-revalidate 整体替换 + self-init + 失效信号（`ledger:changed`）订阅重拉 + 写动作成功即 refresh。每份机制约 30–35 行，测试脚手架四份重复。`ledger:changed` 的前端消费者恰为这四个 store，没有潜藏第五处；但删除测试成立——删掉任何一个 store，机制复杂度会在其余三处与每个未来领域清单重现，在途合并、事件重拉与竞态语义的 bug 无 locality。

四份实现已现真实漂移：写动作后重拉失败的成败语义，items / policies 传播（重拉失败令 create/update reject，视图误报「保存失败」，尽管写入已落库），physicalAssets 吞掉（写入成功即成功，status 是唯一失败信号，#467 裁决）。`version` 是活跃接缝（物品每日成本合计与商户管理视图以它为 watch 源），必须保留。

## 决策

### 1. 机制塌缩为单一工厂 `createPushFirstList`，留壳

新工厂 `createPushFirstList`（住壳内跨域件 composables 根目录，模块名 `push-first-list`）内化全部机制：status / version / inFlight 合并 / self-init / `ledger:changed` 订阅 / SWR 整体替换。四个 store 仍是 Pinia 单一来源单例（词汇表明文约束），在各自 defineStore setup 内调用工厂；四店对外返回面形状一字不变。留壳依据 ADR-0118 先例（viewResetRegistry）：消费面全在壳内，成包无边界收益。`listen('ledger:changed')` 订阅内化进工厂，事件名字面量与「注册完成前到达的事件会丢失（窗口极窄）」的既有取舍随迁单点化。

### 2. 接缝形状：load 快照 + apply 落位

工厂接口为 `{ load: () => Promise<T>, apply: (snapshot: T) => void }`：`load` 产出一次完整快照（单列表、双列表、多表 Promise.all 同形），`apply` 由调用方把快照写入自己的 refs。工厂钉死时序：status=loading → 成功才 apply + version++ → ready；任一失败 → error、不 apply。「整体替换、不闪空、部分失败不落位」由此成为工厂测试可单点钉死的语义。软删拆分、派生映射（statsById、deletedMaps、分类树）、筛选参数（statusFilter 闭包进 load）全部留店，与 ScheduledPlanList 的「纪律门槛：特化留适配器」同构。

### 3. 写动作后重拉失败统一不反转写动作成败

四店统一 physicalAssets 语义（#467 论证普适化）：写入已落库后，重拉失败不令写动作 reject；`status=error` 是唯一失败信号，后续 ledger:changed 或显式 refresh 兜底。写动作保持 await 重拉（成功时动作返回即列表已含新数据），失败静默吞掉。这是本裁决唯一的用户可见行为变化：items / policies 在「写入成功 + 重拉失败」的窄窗口下不再误报「保存失败」。

### 4. physicalAssets 筛选切换竞态保真迁移

`setStatusFilter` 与在途重拉合并时沿用旧筛选结果的窄窗口竞态（现状取舍，非本次引入）随迁移原样保留；缺陷另立票记录，修复大概率落在工厂的作废在途语义内（Loadable invalidate 先例），不在塌缩票内顺手修。

> 注（2026-09-16 修订，issue #1381）：竞态已按本决策预告的作废在途语义修复——工厂新增 `invalidate()` 出口（ADR-0040 同名先例）：推进竞态纪元并对 loading 收尾（有成功快照 → ready，初载在途 → idle，error 不动），此后迟到的旧纪元结果与失败一并作废（不落位、不置 error、对旧调用方静默 resolve）；`setStatusFilter` 在参数变化时先作废再重拉，旧筛选在途结果不再被合并。机制测试仍钉工厂单点（决策 6 口径不变）。
  > 注（2026-09-24 修订，issue #1678）：竞态纪元簿记改经共享 module `@ledger/latest-wins` 消费（refresh 采样当前纪元不推进的在途合并、invalidate 推进作废语义不变）——本决策的作废在途语义与测试口径不变。

### 5. 四店同票迁移，reference 迁移即能力上界证明

items / policies / physicalAssets / reference 四店在同一实施票内迁移。reference 的多表 `Promise.all` + 含删全量拆分正是快照 + apply 的最重用例；其体量本就集中在派生 computed 与软删缓存（留店不动），生命周期核与另三家全同。四店既有测试原样跑绿即行为等价证明（ADR-0085 口径）。

### 6. 测试归属：机制钉工厂单点，店测试退化为领域动作

新增工厂机制测试单点钉死：self-init、SWR 整体替换、在途合并、事件重拉、status / version 变化（经 test-support 的 invoke / listen 替身）。四店既有测试中的机制断言删除不双写，保留领域动作断言（写命令调用、写后触发重拉、领域特有行为）。`ledger:changed` 订阅接线的「删除即变红」由工厂测试承担，店级不加重复负向条目（ADR-0087 断言强度）。参考数据推送集成测试钉的是参考数据域规则（失效机制唯一，ADR-0012），保留店级不动。

### 7. 命名与词汇表：机制术语不进词汇表

模块与叙述用「push-first 清单生命周期」，不设词汇表词条（ADR-0118 机制术语口径），避免与 ScheduledPlanList（计划清单）的「清单」专名撞车；reference 是字典非清单，「清单」在叙述中泛指四店的列表/字典数据面。参考数据域「Reference Data」词条的重拉语义（领域规则）一字不动——工厂只是它的第 N 个实现载体；四店注释里的机制叙述收敛为对工厂的指针。

## 否决

- **composable 工厂每次调用新实例（照 useScheduledPlanList 形态）**：与单一来源单例冲突；外面包一层 store 等于绕远路。
- **defineListStore 造店工厂**：连 store id 与返回面一起造，接口最宽、迁移面最大，领域字段仍得外挂。
- **caller 自给完整 reload**：SWR 整体替换不由工厂保证，机制塌缩退化为骨架塌缩。
- **工厂持有数据 ref + 店 computed 派生**：返回面 Ref→ComputedRef 的无谓形状变化。
- **保留写后重拉失败分叉、经钩子或开关暴露**：分叉是历史漂移不是领域差异，制度化差异与接口变宽相抵后仍亏。
- **成包 `@ledger/*`**：满足「对壳零依赖」判据但消费者全在壳内，登记与方向表成本无边界收益（viewResetRegistry 先例）。
- **既有四份测试原样保留 + 工厂另测**：机制断言店级与工厂级双写，改机制从此五处同步，locality 只在生产侧成立。
- **顺手修筛选竞态**：行为变更与机制塌缩搅在同一票，违背独立交付可回退（ADR-0118 交付序口径）。

## 后果

- 机制改动 locality 收口一处；未来领域清单（如 merchants / insurers 拆出 reference）免费继承整套生命周期，接口面收窄为一个工厂、四个调用点。
- 与 issue #1302 草图的偏差：「可选动作钩子」不再需要——动作留店后写后重拉是动作内一行，接口比草图更窄。
- 四个 Status 类型别名（店外零消费）随迁移收敛为工厂单一定义。
- 唯一用户可见行为变化见决策 3；其余行为等价由四店既有测试跑绿证明。
- 词汇表零新增；ADR-0118 拆包格局不变（留壳跨域件）。
