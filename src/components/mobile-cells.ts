/**
 * 移动档表格单元格共享件（issue #848 / ADR-0088 决策 11 票⑧）：
 * 轻适配页（预算列表、定时计划三页签）移动档列渲染的同构样式、触控目标与
 * 副行拼接单点，防多处复制漂移。内联样式收口在列配置消费侧（同 accounts /
 * transaction-columns 渲染函数先例）；纯数据与样式常量，不引用组件、不接
 * 弹层注册表、不持状态（清单模块弹层纯度同规，ADR-0035 / ADR-0041）。
 */

/** 移动档计划卡单元格布局：纵排堆叠（窄屏无横向滚动前提）。 */
export const MOBILE_CELL_STYLE =
  'display: flex; flex-direction: column; gap: 2px; min-width: 0;'

/** 移动档弱化副行：状态/周期/关联对象等次要信息的小字呈现。 */
export const MOBILE_SUB_STYLE = 'font-size: 12px; opacity: 0.65;'

/** 移动档触控目标（ADR-0088 全局验收基线）：显式 min 尺寸（相邻堆叠/并排按钮
 * 热区互不侵入，不用伪元素外扩；账户行「⋯」同款取舍），随文本自然加宽不溢出。 */
export const MOBILE_TOUCH_TARGET_STYLE = { minWidth: '48px', minHeight: '48px' }

/** 副行拼接：过滤空段后以中点连接（空段不产生悬挂分隔符，全空得空串由调用方占位）。 */
export function mobileSubLine(...parts: Array<string | null | undefined>): string {
  return parts.filter((p): p is string => p !== null && p !== undefined && p !== '').join(' · ')
}
