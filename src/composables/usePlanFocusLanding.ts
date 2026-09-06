import { onMounted } from 'vue'

/**
 * 计划来源落点的页签侧消费（spec #704 / issue #707，词汇表「实体定位参数
 * （focus 参数）」）：定时视图三形态页签（订阅/分期/定时转账）共用的装配时序
 * ——focus 计划 id 在场即打开计划详情弹窗（弹窗按 id 独立取数，不受清单状态
 * 过滤影响——已取消计划照常可开），随后回报消费（视图清闸，页签切换不复开；
 * 刷新/重进 = 新实例重定位）。三页签各持自己的弹窗实例（@changed 回报各页签
 * 清单刷新，ScheduledPlanList 接缝不变），本工厂只收编「读 id → 开窗 → 回报」
 * 这一份时序，页签侧零手搓（三份拷贝必然漂移，spec #704 深模块动机同构）。
 *
 * 工厂形态 composable（useModalIntent 同型）：纯时序编排，不持状态；mount 时
 * 消费一次——视图在 setup 期已把待开 id 就位（先于子页签装配，时序唯一一份）。
 */
export function usePlanFocusLanding(options: {
  /** 待开计划 id（视图 focus 读一次后的暂存 prop；空则无落点）。 */
  focusPlanId: () => string | null | undefined
  /** 开窗动作：页签自己的详情弹窗 open（按 id 独立取数）。 */
  openDetail: (planId: string) => void
  /** 消费回报：视图据此清闸（prop 置空）。 */
  onConsumed: () => void
}): void {
  onMounted(() => {
    const planId = options.focusPlanId()
    if (!planId) return
    options.openDetail(planId)
    options.onConsumed()
  })
}
