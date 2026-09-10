/**
 * 定时计划移动档单元格共享件（issue #848 / ADR-0088 决策 11 票⑧）：
 * 「定时」三页签移动档列渲染的同构样式与副行拼接单点，防三处复制漂移。
 * 内联样式收口在列配置消费侧（同 accounts / transaction-columns 渲染函数先例）；
 * 纯数据与样式常量，不引用组件、不接弹层注册表、不持状态（清单模块弹层纯度
 * 同规，ADR-0035 / ADR-0041）。
 */

/** 移动档计划卡单元格布局：纵排堆叠（窄屏无横向滚动前提）。 */
export const PLAN_MOBILE_CELL_STYLE =
  'display: flex; flex-direction: column; gap: 2px; min-width: 0;'

/** 移动档弱化副行：状态/周期/关联对象等次要信息的小字呈现。 */
export const PLAN_MOBILE_SUB_STYLE = 'font-size: 12px; opacity: 0.65;'

/** 副行拼接：过滤空段后以中点连接（空段不产生悬挂分隔符，全空得空串由调用方占位）。 */
export function planSubLine(...parts: Array<string | null | undefined>): string {
  return parts.filter((p): p is string => p !== null && p !== undefined && p !== '').join(' · ')
}
