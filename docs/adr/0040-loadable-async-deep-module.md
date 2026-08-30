# ADR-0040: 异步样板收进 Loadable 深模块

- 状态：已接受
- 日期：2026-08-30
- 作者：Ledger 项目
- 关联：issue #314（spec，同日 grilling 定型）

## 背景

全仓约 60 处「置 loading → try → catch 提示 → finally 收 loading」异步样板，错误出口实为**三种**风格并存（grilling 探索修正了 spec 初稿「两种」的认知）：视图内 catch 直弹 toast（约 50+ 处，主流）、composable 持 error ref 静默暴露给视图渲染（仪表盘/物品日成本等）、catch 后外抛给调用方（组合概览/组合趋势）。外加两类存量缺陷：try/finally 无 catch 型（预算/报表视图、已实现盈亏、实盘面板首刷）静默失败 + 未处理 rejection；错误对象直接插值打出 `[object Object]`（不过统一归一函数）。全仓仅搜索视图一处手写请求序号守卫。要全局改错误展示策略得逐处判断风格并改数十处——错误出口分裂本身就是接缝缺失的证据。issue #314 决定抽一个异步任务深模块，本 ADR 沉淀 grilling 确认的接缝与规则。

## 决策

1. **新增工厂形态 composable Loadable**：接口三面——发起（单方法）、loading/error 可观察状态；「刷新」即再次发起，不设独立刷新方法。任务约定为 0 元闭包（闭包内自读响应式参数：筛选 refs、表单模型），无传参形态；不持任务结果；无生命周期钩子、无 immediate 选项（首跑时序归调用方）。
2. **发起永不 reject**：成功 resolve 结果、失败 resolve 空值且 error 置位（文案经全仓统一归一函数转中文文本）；error 是唯一成败判据，调用方分支成败看返回值或 error，不需要 try/catch。未处理 rejection 这一类 bug 从此不可表达。
3. **错误展示：默认 toast + error 状态双通道**：默认策略为统一 toast（约 50+ 处多数派行为即事实产品语义）；error 可观察状态同时保留，供视图自选渲染（警示位/空态），「既弹又入警示位」的双通道消费即此形态。「策略单点可换」是物理位置不是机制——策略收口为模块内一处私有函数，换策略 = 改一处，不设注入机制。
4. **toast 实例经模块级单点 sink 获取**：应用入口在消息提供器内部注册 sink（Naive UI 的 useMessage 只在 provider 组件上下文可用，工厂自身与组件外测试都取不到）；注册前 sink 为 no-op，测试注入假 sink 断言。sink 与策略正交。
5. **竞态语义：后发覆盖先发**：请求序号守卫，终态 = 最后一次发起的结果；迟到的前发结果连同其 loading 收尾一并作废。「进行中禁用按钮」是调用方 UI 决策，不进模块语义。
6. **迁移**：先行样板批四个 composable（useDashboardOverview、useItemDailyTotal、usePortfolioOverview、useRealizedPnl）改写为 Loadable 之上薄壳，对外接口词汇不变；usePortfolioTrend **不进样板批**——其 lastFetchedKey 去重/forceRefresh 是面板特有缓存语义而非样板，等其视图重构时跟随。视图约 46 处 toast 风格等候选 1（计划清单模块）或候选 5（弹窗编排）落地时顺带迁移，不单独立项冲刺；刻意静默的 fire-and-forget 吞错（约 9 处 `.catch(() => {})`）是合法形态，不收编。**新代码一律走 Loadable**，旧风格对新代码关闭；存量自然替换，不设硬期限。存量缺陷两类（静默失败型、`[object Object]` 型）随迁移自然治愈，是仅允许的可见变化。
7. **测试策略**：模块测试打接口（发起/成功/失败/重复触发竞态/刷新重入），toast 断言走注入的假 sink，不测内部标志位；组件外直接调用工厂 + 断言 refs 沿用仓库既有 composable 测试形态。先行四 composable 的既有测试迁移为打薄壳接口，外部行为断言保持不变。

## 理由

- **为什么默认 toast**：50+ 对 4，多数派行为就是事实上的产品语义；「零可见」限定于视图风格消费方，先行 composable 的消费方接受一次行为补齐（新增弹提示）——已列入可见变化清单。
- **为什么永不 reject 而非处理后重抛**：纪律是靠不住的接缝——重抛方案里 fire-and-forget 调用方必须 `.catch(() => {})`，否则未处理 rejection 原样复活，等于把刚修掉的 bug 类重新留给调用方纪律。
- **为什么单方法而非 run+refresh 双方法**：先行名单无传参需求；双方法要多背「参数记忆」「从未发起时刷新语义」两条契约。传参需求真出现再扩展 `run(...args)`，既有调用点不受影响。
- **为什么 sink 注册而非 createDiscreteApi**：discrete API 脱离主题上下文，仓库有主题切换，手动跟主题是漂移风险；入口注册一次 + 测试可注入，代价最小。
- **为什么与 TransactionFilter 不冲突**：正交组合——过滤深模块产请求参数与刷新版本号，Loadable 管单次请求任务的生命周期，视图组合两者，接缝不重叠。两者同属「工厂形态 composable 深模块」家族。
- **为什么样板批排除最复杂的 trend**：先行样板应选最典型形态；把缓存语义塞进 Loadable 是给单一消费方的特化需求长全局接口。

## 代价

1. 先行 composable 的消费方新增 toast 行为（原静默/外抛）——行为补齐而非回归，已明示为允许的可见变化。
2. sink 注册前为 no-op：入口接线遗漏则错误静默；入口一次性接线，风险收敛在单点。
3. 单方法 + 永不 reject 的形状对「await 后分支成败」不如 try/catch 直觉——以 error 唯一判据 + 接口文档弥补。
4. 刻意静默任务留在模块外，全仓存在「经 Loadable」与「刻意吞错」两种合法形态——后者数量少（约 9 处）且语义明确（显式吞错），可接受。

## 相关 ADR

- ADR-0030：TransactionFilter 深模块——同族工厂形态先例，本 ADR 的接口纪律沿其房式。
- 词汇表：Loadable 词条见界面状态与交互域 `docs/contexts/CONTEXT-ui-interaction.md`。
