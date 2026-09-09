import { computed, onScopeDispose, ref, useAttrs, watch } from 'vue'
import type { Ref } from 'vue'
import { createOverlayToken } from '@/composables/overlayRegistry'

/**
 * 弹层封装统一上报与关闭通道（issue #845 / ADR-0035 / ADR-0088 决策 7）。
 *
 * App* 弹层薄封装族（AppModal / AppDrawer / AppDropdown / AppPopover /
 * AppPopconfirm / AppSelect / AppDatePicker / AppTreeSelect）共用的单一机制点，
 * 收口两件事：
 *
 * 1. **开/关上报（ADR-0035 既有语义）**：token 随显隐状态上报注册表，驱动快捷键
 *    抑制与系统返回的「有弹层」判定。刻意不声明 show prop（Vue 对缺席 Boolean
 *    prop 转型为 false，会把非受控用法变成受控关闭，先例 AppSelect）——:show /
 *    @update:show 经 attrs 原样透传；非受控开合与受控模式下组件内部触发的开合
 *    经根上的 update:show 监听上报，受控调用方直接改 :show prop 的开合由 attrs
 *    watch 兜底。
 *
 * 2. **关闭请求通道（系统返回桥接的「关最上层」出口）**：受控用法把 false 中继
 *    给调用方的 update:show 监听器（与 ESC 关闭同一条路径——naive-ui emit 到
 *    v-model 的那条线）；非受控用法落影子态（shadowShow 绑定根组件 :show，点击
 *    触发的开合经 update:show 回写影子态，行为与非受控一致）。调用方只传 :show
 *    不传监听器的退化用法不可关，requestClose 返回 false，消费方（closeTopOverlay
 *    → useSystemBack）据此吞掉本次返回键、不回退路由。
 *
 * 卸载兜底（先例 AppDrawer）：跨断点换档时壳层整体卸载，弹层可能仍开着——卸载
 * 不产生 update:show(false)，注册表会滞留开放态导致快捷键永久抑制；作用域销毁
 * 时显式撤销上报（幂等，关闭态重复撤销无副作用）。
 */
export function useOverlayReporting(name: string): {
  /** 根组件 update:show 统一监听：回写影子态 + 上报注册表 */
  onUpdateShow: (value: boolean) => void
  /** 根组件 :show 绑定值：受控跟随调用方，非受控跟随影子态 */
  resolvedShow: Ref<boolean>
} {
  const attrs = useAttrs()
  const overlay = createOverlayToken(name, requestClose)
  const shadowShow = ref(false)

  // 受控调用方直接改 :show prop 的开合由 attrs watch 兜底上报
  watch(
    () => attrs.show,
    (value) => {
      if (value !== undefined) {
        shadowShow.value = Boolean(value)
        overlay.set(Boolean(value))
      }
    },
    { immediate: true },
  )

  const onUpdateShow = (value: boolean) => {
    shadowShow.value = value
    overlay.set(value)
  }

  const resolvedShow = computed(() =>
    attrs.show !== undefined ? Boolean(attrs.show) : shadowShow.value,
  )

  function requestClose(): boolean {
    const handler = attrs['onUpdate:show'] as ((value: boolean) => void) | undefined
    if (typeof handler === 'function') {
      // 受控：中继给调用方（v-model / :show + @update:show 均覆盖），调用方改
      // :show 后由上方 attrs watch 上报注册表，不在本处抢报
      handler(false)
      return true
    }
    if (attrs.show !== undefined) {
      // 受控但无监听器：调用方状态不可达，无法关闭（退化用法，全仓无此调用形态）
      return false
    }
    // 非受控：落影子态并自报——prop 驱动的关闭不产生 update:show，无 emit 可等
    shadowShow.value = false
    overlay.set(false)
    return true
  }

  onScopeDispose(() => overlay.set(false))

  return { onUpdateShow, resolvedShow }
}
