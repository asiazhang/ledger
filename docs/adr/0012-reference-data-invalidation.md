# ADR 0012: 参考数据失效策略——通用事件驱动失效（ledger:changed）

- 状态：已接受
- 日期：2026-08-26
- 作者：Ledger 项目

## 背景

Ledger 桌面应用运行期间，AI 编程助手经常经本地 HTTP API（`http://127.0.0.1:9527/api/v1`）导入或修改记账数据（账户 / 分类 / 交易）。这些写入发生在 Tauri 应用之外（外部进程调用应用内嵌的 HTTP 服务器），应用前端的内存态 store 对此一无所知，导致已打开的界面里「账户名 / 分类名映射、表单选项、分类树」等参考数据过期。

现有代码用「每个视图挂载都全量 `loadAll()` 重新拉取」兜底——靠时刻刷新绕开缓存，而非真正感知数据失效。三处实际代价：

1. `App.vue` 与首个路由视图各触发一次 `loadAll()`，首次导航重复拉取；
2. 并发挂载同一批参考数据时无在途去重；
3. 「数据可能过期」的认知被写死在 store 里（注释明言「不缓存、不幂等」），消费者无法推理新鲜度。

本决策把前端参考数据提升为一个带失效信号的深度模块（spec #76 候选 4），并确定失效策略的取舍——事件驱动失效 vs 时刻刷新 vs 按资源粒度失效。以下决策 1–4 已落地（#78 / #79），决策 5–7 为设计目标、随 spec #76 后续子任务（#81 起）落地，落地进度见「影响」节。

## 决策

1. **参考数据 = 带失效信号的深度模块，单一来源**。`useReferenceStore`（`src/stores/reference.ts`）持有 `currencies / accounts / categories` 三张参考表及全部派生映射（`currencyMap / accountMap / categoryMap`）与分类树逻辑（`rootCategories / expenseCategories / incomeCategories / categoryChildren / categoryPath / treeCategoryOptions`），作为 UI 字典 / 枚举的单一来源。
2. **失效策略 = 事件驱动失效（push-first）**。任一参考写入成功后，后端发出通用、粗粒度、无 payload 的 `ledger:changed` 信号；前端 `useReferenceStore` 订阅该信号 → 置为 loading → 重拉三张参考表 → 推回响应式数组，视图消费响应式状态即自动反映新鲜数据，**不再依赖「每次挂载全量刷新」**。
3. **信号设计 = 通用可复用，无 payload**。`ledger:changed` 不与具体资源绑定，未来其他订阅者（如交易视图实时刷新）可低成本复用，无需重设计。**交易类写入本期不 emit**（不改参考表，参考数据消费者不需要交易级信号；日后需要时再补 emitter）。
4. **「是否为参考写入」的判定收口在 events 模块**（`src-tauri/src/events.rs`）：IPC 命令清单 `REFERENCE_WRITE_COMMANDS`（账号 create/delete，分类 create/update/reorder/delete）+ 纯函数 `is_reference_write`，命令层统一经 `emit_reference_changed` 走该判定；HTTP 端点（账号 / 分类 create/delete）结构上即参考写入，直接经 `emit_ledger_changed_if_present` 发射（`Option<AppHandle>` 为集成测试留缝，生产路径注入 `Some`）。新增参考写入命令须同步扩充清单，由单测锁定映射。
5. **push-first 生命周期**。store 首次被访问时自我初始化触发一次 `refresh()`；`ledger:changed` 订阅放在 store setup 体内（仅注册一次）。重拉采用 **stale-while-revalidate**：保留旧数据，成功后整体替换，避免界面闪空。
6. **暴露失效信号与动作**。`status`（`idle | loading | ready | error`）与 `version`（每次成功重拉自增）供消费者显式感知新鲜度；`refresh()`（强制拉取）与 `ensureFresh()`（缓存 + 在途去重 + stale 感知，fresh 时零 IPC）。
7. **消除手工刷新散布**。删除各视图 `onMounted → loadAll()`（含 `App.vue`）与账户 / 分类管理流程里的手工 `loadAccounts()` / `loadCategories()`；应用自身写入（同一批 IPC 命令）天然经信号失效，与外部写入行为一致。

## 理由

1. **事件驱动优于时刻刷新**。参考数据的变化是低频事件（导入 / 管理操作），用「每次挂载全量拉取」兜底，把「数据可能过期」的认知写死在 store，还带来重复拉取、无在途去重、消费者无法推理新鲜度三处代价。事件驱动在数据真正变化时才工作：本地 SQLite + IPC 开销可忽略，且给了消费者可观测的 `status` / `version` 信号，新鲜度可推理。
2. **粗粒度通用优于按资源粒度**。三张参考表互相引用（分类树依赖分类表、映射依赖各自表），且账户 / 分类的改名、删除会级联影响所有引用它的界面；按资源粒度（`accounts:changed` / `categories:changed` 分开）需要为每个资源设计事件，未来订阅者无法复用，而一次全量重拉三张表的开销可忽略——粒度红利不足以抵消接口复杂度。
3. **交易写入不 emit**。交易写入不改参考表，emit 只会触发无谓重拉；参考数据消费者（表单选项、名称映射、分类树）不需要交易级信号。通用事件保留了未来低成本加订阅者的路径（spec #76 Out of Scope）。
4. **应用自身写入也依赖信号回拉，而非乐观更新**。本地 IPC 快，信号回拉保证「应用自身写入」与「外部 AI 写入」走同一条单一代码路径，行为一致，不在每个写入点复制参考数据变更逻辑。
5. **stale-while-revalidate**。刷新时保留旧数据、成功后才替换，界面不闪空，用户不会看到参考数据短暂消失。

## 代价

1. **粗粒度信号有重拉放大**。一次账户写入会重拉三张参考表。三张表为字典级数据、量小（本地 SQLite + IPC），开销可忽略。
2. **写入命令与信号清单耦合**。新增参考写入命令必须同步进入 `REFERENCE_WRITE_COMMANDS`，漏加则界面不刷新；由 `is_reference_write` 单测锁定清单映射，降低漏加风险。
3. **存在短暂过期窗口**。写入成功到前端重拉完成之间，界面仍显示旧数据（本地 IPC 毫秒级，可接受）；`status` / `version` 让消费者能显式感知该窗口。
4. **测试缝取舍**。后端 emit 为薄胶，不为它造 Tauri `AppHandle` 测试桩（HTTP 服务器中 `AppHandle` 为 `Option`，集成测试传 `None` 跳过发射）；「是否为参考写入」的判定抽为纯函数可单测。

## 替代方案

- **时刻刷新（现状：每个视图挂载都全量 `loadAll()`）**：把「数据可能过期」写死在 store、首次导航重复拉取、无在途去重、消费者无法推理新鲜度，否决。
- **按资源粒度失效（`accounts:changed` / `categories:changed` 分开）**：信号更精确，但需为每个资源设计事件、未来订阅者无法复用；参考表互相引用 + 级联影响面大，粒度红利小（全量重拉开销可忽略），否决。
- **乐观更新（前端写入后直接改 store）**：需在每个写入点复制参考数据变更逻辑；外部（HTTP AI）写入无法乐观更新，路径分裂，否决。
- **定时轮询（前端周期拉取）**：与时刻刷新同病——无谓 IPC + 延迟不可控，且无失效信号可观测，否决。

## 影响

- **已完成（分批落地）**：
  - #78：抽出 `useReferenceStore` 作为参考数据单一来源；`useAppStore` 参考数据 getters 全部委托到新 store（共享同一份状态），现有消费者零改动。
  - #79：后端 `ledger:changed` 信号（`src-tauri/src/events.rs`：事件名常量 + `REFERENCE_WRITE_COMMANDS` 清单 + `is_reference_write` 纯函数 + emit 薄胶入口）；`AppHandle` 传入 HTTP 服务器；参考写入端点（账号 / 分类 create/delete）与参考写入 IPC 命令（账号 create/delete，分类 create/update/reorder/delete）成功后 emit；交易类写入不 emit。
  - #81：`useReferenceStore` 内建失效信号（`status` / `version`）+ push 生命周期（订阅 `ledger:changed` → 重拉）+ `refresh()` / `ensureFresh()`。
  - #82–#84：消费端迁移批次（账户 / 分类管理流、交易 / 搜索流、报表 / 预算 / 投资 / 设置流）改为从 `useReferenceStore` 读取并依赖信号刷新。
  - #85：移除 `useAppStore` 参考数据 getters / load 函数（含参考 store 的遗留 `loadAll` / 单表 load），收缩为纯 UI 设置 store（主题 / 默认币种 / 备份设置）。
- **待完成（spec #76 后续子任务）**：
  - #86：端到端整合验证 + 组件反应性测试 + 测试迁移。
- **文档**：本 ADR + `CONTEXT.md` 术语表新增「参考数据 Reference Data」条目（issue #80）。
- 无 schema 变更、无迁移。
