import { groupOfView, type SidebarGroupId } from '@/stores/sidebar-order'
import type { TransactionSourceKind } from '@/types'

/**
 * 来源跳转目标计算（spec #704 / issue #705，词汇表「来源列」「实体定位参数
 * （focus 参数）」）：交易列表来源列点击 → 路由目标的唯一计算点——输入来源
 * 类型闭集 + 实体 id + 组内收纳状态，输出 { 视图名, query }。落点分流与 focus
 * 装配收口此处，后续消费票（#706 保单 / #707 计划 / #708 物品 / #709 标的）
 * 的点击接线只调本函数，不各写一遍路由细节（与行菜单/弹窗编排深模块收编
 * 手搓时序同构，spec #704「来源跳转深模块（唯一新接缝）」）。
 *
 * 落点语义（词汇表 focus 参数「落点尊重组内收纳」）：
 * - 计划三形态——主项态落定时独立路由（query.tab 即形态页签）；收纳态落记账
 *   「更多」定时页签（容器 tab=scheduled），形态页签以 scheduledTab 叠加——
 *   内嵌定时页签为内存态、query.tab 归容器（issue #473 双写互踩约定），叠加
 *   通道由落点侧消费（#707 接线内嵌 focus 通道）。
 * - 保单——主项态落独立路由（收纳时既有 beforeEnter 重定向兜底）；收纳态直接
 *   落资产「更多」保单页签（重定向透传 focus 由消费票在路由守卫侧接线，本票
 *   语义先行，验收标准 2）。
 * - 物品 / 标的——主项路由直达；#474 起任一主项可移入组「更多」，收纳态同样
 *   落所在组「更多」对应页签（落点尊重用户布局）。
 * - focus 统一装配：全部目标一律携带 focus=<实体 id>（一名一义，消费语义见
 *   useFocusParam——目标视图侧读一次助手，两文件共同构成来源跳转深模块）。
 *
 * 纯函数纪律：不见 router、不见 store 实例——收纳状态经谓词入参注入（调用方
 * 传 sidebar-order store 的 isViewContained 绑定），组归属经顺序源模块词表
 * groupOfView 只读推导（收纳落点路由名沿用 App.vue「更多」链接的 `<组 id>-more`
 * 既有命名）；测试以普通函数直打（先例：行菜单编排、弹窗意图编排工厂测试形态）。
 */

/** 来源类型闭集（wire 契约单一定义点在 `@/types` 交易行来源，issue #706 定型；
 *  此处再导出供既有消费面兼容——本模块是消费方，不另持一份词表）。 */
export type { TransactionSourceKind } from '@/types'

/** 定时视图形态页签词表（ScheduledView TABS 同源；#707 接线时视图侧改引此处收口）。 */
export type ScheduledFormTab = 'subscriptions' | 'installments' | 'transfers'

/** 来源可达的目标视图词表（收纳判定与落点分流的问询对象，均为收纳视图名子集）。 */
export type SourceTargetView = 'scheduled' | 'policies' | 'items' | 'investments'

/**
 * 来源 → 目标视图与形态页签（闭集穷尽映射，无分支——新增来源种类编译器强制
 * 补行）；计划三形态携带形态页签，其余来源无。 */
const KIND_TARGETS: Record<TransactionSourceKind, { view: SourceTargetView; formTab?: ScheduledFormTab }> = {
  installmentPlan: { view: 'scheduled', formTab: 'installments' },
  subscription: { view: 'scheduled', formTab: 'subscriptions' },
  scheduledTransfer: { view: 'scheduled', formTab: 'transfers' },
  policy: { view: 'policies' },
  item: { view: 'items' },
  instrument: { view: 'investments' },
}

/** 来源落点路由名：四个目标视图独立路由 + 组「更多」聚合页（洞察组无收纳来源，路由预建不达）。 */
export type SourceJumpRouteName =
  | SourceTargetView
  | 'bookkeeping-more'
  | 'assets-more'
  | 'insights-more'

/** 组 → 「更多」聚合页路由名（路由镜像侧栏层级，ADR-0063；与 router 记录一致）。 */
const GROUP_MORE_ROUTE: Record<SidebarGroupId, SourceJumpRouteName> = {
  bookkeeping: 'bookkeeping-more',
  assets: 'assets-more',
  insights: 'insights-more',
}

/** 跳转目标：vue-router push 的 name + query（query 值全为字符串参数）。 */
export interface SourceJumpTarget {
  name: SourceJumpRouteName
  query: Record<string, string>
}

/**
 * 来源跳转目标计算：六类来源 × 主项/收纳两态 → 路由目标。
 * isContained 只会被问询本次跳转的目标视图（逐一定问，不问无关视图），
 * 调用方传 `(v) => useSidebarOrderStore().isViewContained(v)` 绑定即可。
 */
export function resolveSourceJumpTarget(
  kind: TransactionSourceKind,
  entityId: string,
  isContained: (view: SourceTargetView) => boolean,
): SourceJumpTarget {
  const { view, formTab } = KIND_TARGETS[kind]

  // 收纳态：落所在组「更多」对应页签（tab=目标视图名）；计划形态页签以
  // scheduledTab 叠加（容器 query.tab 归容器，内嵌定时页签内存态，见文件头注释）。
  // 目标词表均为侧栏在册视图，groupOfView 恒有组；空值分支为词表防御
  // （回退独立路由，语义仍成立）。
  if (isContained(view)) {
    const gid = groupOfView(view)
    if (gid) {
      const query: Record<string, string> = { tab: view, focus: entityId }
      if (formTab) query.scheduledTab = formTab
      return { name: GROUP_MORE_ROUTE[gid], query }
    }
  }

  // 主项态：独立路由直达；计划形态页签即定时视图 query.tab（既有页签通道）。
  const query: Record<string, string> = { focus: entityId }
  if (formTab) query.tab = formTab
  return { name: view, query }
}
