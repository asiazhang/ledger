import { computed, onScopeDispose, ref } from 'vue'
import type { ComputedRef } from 'vue'

/**
 * 输入轴（Input Mode，ADR-0088 决策 6 / 词汇表「输入轴」）：交互形态轴唯一事实源，
 * 与窗口分级的宽度轴正交——宽度档位不决定交互形态（平板横屏 = 桌面档 + 触控轴）。
 * 本 composable 只做「hover / pointer 信号 → 输入轴」纯映射，交互替换的落地
 * （快捷键退役渲染、hover 一击可达等）归消费方与各实施票。
 */

/** hover 能力查询：主指针环境可悬停（鼠标 / 触控板）。 */
const HOVER_QUERY = '(hover: hover)'
/** 主指针精度查询：精细主指针。 */
const FINE_POINTER_QUERY = '(pointer: fine)'

/** 输入轴闭集：触控轴 / 指针轴。 */
export type InputMode = 'touch' | 'pointer'

/**
 * 输入轴 composable：hover / pointer 媒体查询 → 触控轴 / 指针轴。
 *
 * 映射矩阵（保守方向：宁可触控轴兜底，不可指针轴漏判）——
 * - hover 可用 **且** 主指针精细 → 指针轴（桌面鼠标 / 触控板）；
 * - 其余全部组合 → 触控轴：不可悬停或主指针粗糙，任一触控信号成立即触控轴。
 *   误判触控的代价是多一份一击可达替代（常显按钮、点按展开），
 *   误判指针的代价是悬停盲区与快捷键失达，后者不可接受。
 *
 * 监听两路查询变化（设备主输入变化实时换轴），作用域销毁时注销监听；
 * hover / pointer 查询收口于本文件，与断点常量并列为两根轴的唯一事实源。
 */
export function useInputMode(): ComputedRef<InputMode> {
  const hoverMql = window.matchMedia(HOVER_QUERY)
  const finePointerMql = window.matchMedia(FINE_POINTER_QUERY)
  const canHover = ref(hoverMql.matches)
  const hasFinePointer = ref(finePointerMql.matches)
  const onHoverChange = (event: MediaQueryListEvent): void => {
    canHover.value = event.matches
  }
  const onPointerChange = (event: MediaQueryListEvent): void => {
    hasFinePointer.value = event.matches
  }
  hoverMql.addEventListener('change', onHoverChange)
  finePointerMql.addEventListener('change', onPointerChange)
  onScopeDispose(() => {
    hoverMql.removeEventListener('change', onHoverChange)
    finePointerMql.removeEventListener('change', onPointerChange)
  })
  return computed<InputMode>(() =>
    canHover.value && hasFinePointer.value ? 'pointer' : 'touch',
  )
}
