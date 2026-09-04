// 全局忙碌状态模块（issue #500 / spec #498）：统一 invoke 封装之下的在途 IO 聚合计数器。
//
// 语义（词汇表 GlobalBusyBar 词条；与 Loadable（ADR-0040，单任务 loading/error）、
// 同步进度条（确定进度、事件驱动）三足鼎立）：
// - 并发聚合计数：每次在途 IO 计数 +1，收尾（成功或失败）-1；多个并发调用聚合为一条忙碌条。
// - 300ms 延迟显示：聚合计数自首个调用起持续非零越过阈值才置可见——快操作永远看不见它；
//   在途窗口在并发间连续（前一个未收尾后一个已起跳）视为同一段忙碌期。
// - 计数归零即隐藏：最后一个调用收尾立即撤下忙碌条，显示不设最短驻留、隐藏不设延迟。
// - 错误路径递减：收尾对成功/失败同一通道恰好执行一次，reject 照常递减，条不卡死；
//   错误契约不变，仍按原样上抛调用方（成败语义归 Loadable，本模块不承载）。
// - 非模态环境指示：不注册 Overlay Suppression（ADR-0035），忙碌期间快捷键照常工作。

import { readonly, ref } from 'vue'

/** 显示阈值：聚合计数持续非零越过该时长才显示（毫秒） */
export const BUSY_SHOW_DELAY_MS = 300

const pendingCount = ref(0)
const visible = ref(false)

let showTimer: ReturnType<typeof setTimeout> | null = null

function clearShowTimer(): void {
  if (showTimer !== null) {
    clearTimeout(showTimer)
    showTimer = null
  }
}

/** 忙碌条可见状态：顶部条组件唯一消费的状态出口（只读） */
export const busyVisible = readonly(visible)

/**
 * 把一次在途 IO 的完整生命周期纳入忙碌聚合计数：调用即计数 +1 并武装显示定时器
 * （仅在聚合窗口起点武装一次，越过阈值置可见），收尾在任何结束路径恰好递减一次，
 * 计数归零立即撤下定时器与可见状态。统一 invoke 封装是唯一生产消费方——
 * 新 IO 走 api 层即自动贡献计数，零额外接线。
 */
export function trackBusy<T>(task: Promise<T>): Promise<T> {
  pendingCount.value++
  if (showTimer === null) {
    showTimer = setTimeout(() => {
      showTimer = null
      if (pendingCount.value > 0) visible.value = true
    }, BUSY_SHOW_DELAY_MS)
  }
  // 收尾闭包：then 两回调恰只执行其一，每次纳入恰好递减一次，无需幂等守卫
  const end = () => {
    pendingCount.value--
    if (pendingCount.value === 0) {
      clearShowTimer()
      visible.value = false
    }
  }
  // Promise.resolve 对原生 promise 恒等返回（生产路径零失真），仅对非 thenable
  // 兑底包装（测试替身的 invoke 可能返回裸值，先例：invokeHandler 裸值处理器，
  // 行为有测试钉住）；递减挂在收尾通道，不改变值与错误的传递契约，原样返回调用方
  const p = Promise.resolve(task)
  p.then(end, end)
  return p
}

/** 测试隔离用：清零聚合计数、撤下可见状态与在途定时器（先例：resetToastSink） */
export function resetGlobalBusy(): void {
  clearShowTimer()
  pendingCount.value = 0
  visible.value = false
}
