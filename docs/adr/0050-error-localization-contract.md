# ADR-0050: 错误本地化契约——AppError 码化只增不改，前端按码插值、无码透传

- 状态：已接受
- 日期：2026-09-02
- 作者：Ledger 项目
- 关联：issue #342（spec 二期）、#353（实施票）；ADR-0049（i18n 架构，本契约是其错误提示部分）；词汇表「错误码」（核心交易域）

## 背景

错误提示是最后一块不随界面语言本地化的用户可见文案。后端 `AppError` 以
`{"kind": ..., "message": "<中文>"}` 序列化，message 是给人读的中文句子，前端只能
透传——英文界面用户看不懂失败原因。带参数的错误（如「缺少 USD→CNY 汇率」）需要
把动态值从句子里拆出来才能在另一种语言里重组出自然语句。

否决案「后端按请求 locale 返回翻译」：IPC 非 HTTP 语义、后端需维护翻译资源、破坏
「message 恒中文」的调试与测试基线（大量既有测试断言中文消息）。

## 决策

1. **序列化只增不改**：`AppError` 的 wire 形态在既有 `kind`/`message` 之外只增两个
   字段——稳定 `code`（错误条件唯一标识）与可选 `params`（插值参数字符串数组，按
   消息中动态值出现顺序）。既有字段的取值、顺序、语义完全不变；无码错误保持原两字段
   形态。既有消费方与测试零破坏。序列化由手写 `Serialize` 实现承载（derive 无法表达
   「字段条件性缺席」）。
2. **每个用户可见错误条件一个码，用领域语言命名**：`<域>.<条件>`，全小写 kebab-case
   （如 `transfer.to-account-required`、`fx.rate-missing`、`instrument.not-found`）。
   码表词表式收敛现有全部用户可见错误条件：生产代码里的 `Invalid`/`NotFound` 构造点
   逐点转为码化构造器（`coded` / `codedp` / `coded_not_found`），消息逐字保留；
   程序性/内部错误可不转（前端降级透传）。`Db`/`Parse`/`Io` 三类系统错误由序列化层
   统一携带稳定通用码（`db.error` / `parse.error` / `io.error`），底层驱动消息不翻译
   也不进码表。
3. **码化错误携带归类**：`Coded` 变体内含 `class`（Invalid→400 / NotFound→404），
   序列化后 `kind` 取归类同值（`Invalid` / `NotFound`），HTTP 状态与 IPC 语义同既有
   口径。
4. **前端按码本地化，降级透传**：错误展示唯一接缝 `errorMessage(e)`（`src/utils/errors.ts`）
   扩展——错误带码且当前语言配置了 `errors.<code>` 文案（码内点号即文案嵌套路径）则
   用 `params` 插值翻译（如缺汇率错误插出 USD→CNY 完整语句）；无码、未知码或未配
   翻译的新错误一律透传后端中文原文——**用户永远读到可读的错误信息，绝不显示 key
   代号**。中文界面的 zh 模板与后端消息模板逐字一致，输出与现状无差别。
5. **码表落双语文案资源**：全部码的 zh/en 模板收进 `packages/i18n/src/locales/{zh-CN,en-US}/errors.json`
   （zh = 后端消息模板的同形物，en = 新译；文案资源随 @ledger/i18n 包落位，issue #1151），与其他文案资源同受 key 集合全等校验
   （ADR-0049 决策 6）——「码表覆盖全部用户可见错误条件」由码表与后端码点的对照评审
   保证，双语 parity 由门槛脚本保证。新增错误条件 = 后端用码化构造器 + errors.json
   两份补翻译，漏翻在 CI 拦截。
6. **测试归口**：错误契约的字段断言（`code`/`params` 存在且稳定、`message` 中文不变）
   归 `src-tauri/tests/api_server/` 集成测试（HTTP 端点层既有归口，#296/#304 先例），
   不新增 BDD `.feature`；`error.rs` 内单测钉死序列化形态基线；前端插值/降级行为归
   Vitest（`errors.test.ts`）。

## 理由

- 码是**条件**的标识而非消息的标识：同一条件在不同上下文触发产出同一码，前端文案
  才能稳定命中；message 保持权威中文原样，调试、日志、既有测试基线全部不动。
- 前端插值而非后端拼装：翻译资源留在前端（与全部界面文案同域），后端不感知语言；
  params 把「值」与「句子」解耦，en 语序可自由重组。
- 降级透传是故意留的失败安全：未码化/未翻译的新错误不会以 key 代号或英文占位符的
  形态暴露给用户，i18n 覆盖率可以渐进提升而不阻塞功能。

## 代价与边界

- zh 模板是后端消息的同形复制：后端改消息文案时必须同步改 zh 模板（对照关系在码表
  评审中人工保证），否则中文界面错误提示与后端原文出现分叉。
- 码即对外契约：改名等于破坏消费方，定名需一次到位（kebab-case、域前缀）。
- `errorMessage` 的翻译发生在调用时点：语言切换后已弹出的 toast 不回溯重译（预期行为）。

## 相关 ADR

- ADR-0049（i18n 架构与 key 全等门槛；本契约复用其文案资源与校验机制）
- ADR-0047（api_server 集成测试归口先例的延续）

## 修订记录

- 2026-09-12 grilling 复核（模型审查）：决策 5 的「码表覆盖全部用户可见错误条件」存在实现缺口——至少十个已 `coded` 的错误码在 zh/en 模板中缺失（含投资域写入接缝码与出资账户接缝码），用户会看到未翻译的码。已立 issue #1188，逐码按「补模板，或判定为程序缺陷降级为裸内部错误」收口；本契约本身不变。
- 2026-09-12 收口（issue #1188）：全量差集枚举生产代码全部码化构造点（`coded` / `codedp` / `coded_not_found` / `codedp_not_found` 字面量与 `const NAME: &str` 一级引用静态枚举；动态构造点 12 处逐一人工核查：closed_set 宏 1、engine.rs 1、validation.rs 3、trade.rs 2、trend.rs 1、unwind.rs 4）共 258 码，与 zh/en 模板差集 12 条全部补齐模板、零豁免降级——其中 `balance.cache-row-missing` 是 ADR-0067 有意的码化设计（缓存行缺失报码化错误引导审计修复），其余 11 条为接缝/守卫码（`transaction.*-unregistered`、`sync-engine.truncate-not-owner`）：虽语义上是接线缺失类程序缺陷，但触发时前端确实展示 message 且码值已被单测钉死为对外契约，按「补模板」主路径收口。非逐字同形项共两处（模板无法逐字复刻后端消息，取等义主干）：`transaction.investment-hook-unregistered`（消息含内部槽名动态段且无 params，模板取无槽名主干）；`balance.cache-row-missing`（两调用点形状不一致：一处 message 插账户名而 params 传账户 id、另一处 message 无名称段，模板 {0} 渲染账户 id）。守门机器化：`scripts/check-i18n-keys.ts` 在双语 key 全等之外增查「每个静态可枚举的码化构造码必须在 zh/en errors.json 均有模板」（动态构造点无法静态解析，计入 unresolved 供人审，漏报方向安全；`db.error` / `parse.error` / `io.error` 系统通用码无构造点、按决策 2 不进码表，无需白名单），挂入 check.sh。检视修正（同票）：`physical-asset.<估值|处置>-date-future` 原以中文短名拼码，运行时永不匹配既有 `valuation-/disposal-date-future` 模板键（en 用户降级读中文），已改为 slug 与文案分离传参并对齐模板键（域单测钉住）。
