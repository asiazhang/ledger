# ADR-0101: 同步重放注册表——ReplayBinding 单点承载可重放契约，裁决域派生归域，三道门守契约

- 状态：已接受（grilling 定稿，2026-09-11）
- 日期：2026-09-11
- 作者：Ledger 项目
- 关联：#1006（spec 与实施票，架构走查 2026-09-11 候选 4）；ADR-0091（哑通道 + 语义命令重放——本决策是其接口化补充，不改变其语义）；ADR-0047（命令注册注解驱动——被否决的 build.rs 扫描路线的先例与对照）；ADR-0087（断言强度——门的验收判据）；ADR-0056（后端分层，域不依赖壳）；#860/#861（DomainCommand 14 实体的既有落点）

## 背景

外来 op 的可重放要求——① 载荷可 serde 且只增不改；② 有裁决域派生（subject）供 LWW；③ 域暴露重放入口；④ 重放不得再产出本地 op；⑤ 确定性——没有任何 trait 或注册表承载，只落在 ADR-0091 的散文与 14 个同形自由函数里。14 个 dispatch 入口分布 11 个域模块，用 6 个函数名、2 种返回形状（`Result<()>` × 13、`Result<ReplayEffect>` × 1）；`entity()`、`subject()`、dispatch 三处 14 臂 match 各自为政；serde tag 派生的实体字符串与 `entity()` 手写 14 个字面量构成双源，无测试兜底。

需校准的一个事实：三处 match 均对 enum 穷尽，新增变体时编译器已强制改三处——「漏掉一处不会报错」的真正缺口是双源漂移、形状发散无强制与契约不可发现，而非「臂可以漏」。本次目标据此重述：契约单点可发现、消灭双源、形状归一由编译器拦；不追求消掉 dispatch 的臂（纯 Rust 中 enum 变体到数据的路由只有 match，那只有 codegen 能做到）。

## 决策

### 1. 载体：sync_engine 内注册表——trait 单点定义，14 个适配绑定同居一文件

契约定义（`DomainCommand`、`ReplayEffect`、trait `ReplayBinding`）集中住在既有契约文件（`command`），14 个适配绑定（adapter impl）集中住在 sync_engine 内与 ops/parked/positions 平级的注册表文件（`registry`）。trait 携带 `ENTITY: &'static str`、`subject()`、`replay()`；绑定 wrap 既有域函数——6 个函数名与 2 种返回形状由适配层吸收，**域侧除裁决域派生归位（决策 2）外零改动**。依赖方向保持 sync→domains 单向；业务域对 sync_engine 的合法引用仅限契约类型（决策 4b）。

- **备选与否决**：
  - trait impl 放进各域文件——「接口即文档」离域更近，但新增 13 个业务域→sync_engine import，把「同步消费所有域」的自然单向倒成双向；签名归一的收益被依赖噪声抵消。
  - build.rs 扫描生成（ADR-0047 先例）——该先例由跨语言（Rust↔TS）双源所迫；此处纯 crate 内、穷尽 match 已是编译期守门，扫描器的维护成本（fail loud 规则、两处扫描同步扩展）买不到新守门力。
  - 声明宏单点清单（一个宏调用生成 enum 与三方法）——真单点，但已发布 wire 契约 enum 变成宏展开物，可读性、可评审性、IDE 导航全受损，与「契约面可评审」冲突。

### 2. 裁决域派生归域：subject() 归一为 (实体标签, 可空实体键)，entity() 删除

`DomainCommand::subject()` 签名改为 `(&'static str, Option<Cow<'_, str>>)`：实体标签恒有（op 落库与挂起行 entity 列取值），实体键可空（None = 无实体指向，冲突域在 OccurrenceKey、不参与同实体 LWW——对准 TotalOrder 既有划界）。`entity()` 整个删除（非测试消费者仅 op 落库与挂起行构造两处，改读标签），真消掉一处 14 臂 match。14 个语义命令类型统一自带 subject 派生——实体自然键是域自身知识，今日 8 个由引擎内联调 `subject_id()` 的域各补一个数行方法；引擎只拥有 LWW 比较策略与全序编排。

### 3. 返回形状不归一：适配层吸收差异，ReplayEffect 定位为契约类型

13 个 `Result<()>` 由适配层映射为 `Applied`，scheduled 透传 `ReplayEffect`——期次触发的幂等命中是 OccurrenceKey 独有语义（CONTEXT-sync 期次词条），逼 13 个无此概念的域返回它只会让类型说谎；差异留在适配层并注明缘由，比假统一诚实。`ReplayEffect` 从引擎实现文件移入契约文件：它是重放契约的词汇（`Applied`/`IdempotentHit` 对应 `OpOutcome::Applied`/`Deduped`），不是引擎内部件。

### 4. 三道门

- **(a) 双源门：样本轮询测试**。每变体一条最小样本，断言 serde tag == `ENTITY`。样本表以「穷尽 match + 兜底 panic 臂」实现：新增变体漏补样本时测试运行期红并列出变体名——双源缺口不对第 15 个变体静默重开。
- **(b) 契约④门：结构守门严形态**。check-structure.ts 增规则：业务域引用 sync_engine 仅允许契约模块（command），引用 ops/engine/model 等内部件一律红——「重放不产本地 op」今天靠「域不知道 ops 存在」的结构巧合成立，扫描把巧合变规格。严形态一步到位，未来合法 import 走既有认许边机制逐条留痕；夹具正反例为变红证明。基线勘误：scheduled 今日 import 的是 `engine::ReplayEffect`，严形态在基线为红；绿点 = `ReplayEffect` 挪入契约模块之后——门 (b) 与决策 3 的挪位同批落地，不拆分。
- **(c) 注册完备门：编译器**。穷尽 match 的臂引用绑定，删任一绑定即编译红——无需额外机制。

### 5. wire 契约零变化

enum、serde 输出、`sync_ops.entity` 列取值、挂起行字段全部不动；本决策是接口收敛，不改重放语义与载荷兼容规则（只增不改），ADR-0091 保持原样，无发布边界问题。

## 代价与边界

1. 适配绑定含 13 处 `() → Applied` 的仪式性转发——形状差异诚实的对价。
2. 样本 JSON 随新增变体维护；兜底 panic 臂把漏补变成运行期红，非编译红。
3. 门 (b) 是文本级扫描，别名改写不可达——与 check-structure.ts 既有边界一致，靠评审兜底。
4. 域侧 subject 派生的签名形状不强制统一（Option 包装与否由域自定），由适配层归一——域保留自然形状。
5. 「新增一个可重放域」仍非字面加一行：加 enum 变体 + 两处一行臂 + 一个绑定 + 域侧 subject 派生与重放入口 + 样本——每步都被门拦住漏项，但步数是诚实的。
6. 词汇表收「重放注册表」词条（接缝名与契约承载关系）；机制细节以本 ADR 为唯一解释处（与 ADR-0047 决策 6 同一取舍，但本机制属 sync 分域自有接缝，故词条落 CONTEXT-sync）。
