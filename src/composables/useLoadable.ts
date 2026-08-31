import { ref } from 'vue'
import { errorMessage } from '@/utils/errors'

/**
 * Loadable：前端异步任务统一生命周期模块（工厂形态 composable，ADR-0040 / issue #320）。
 *
 * 「发起动作进、终态出」：调用方以 0 元闭包声明任务（闭包内自读响应式参数），
 * 模块内化 loading 置收、错误捕获与文案归一、竞态裁决与错误展示，
 * 产出可观察的 loading/error 状态；「刷新」即再次发起（同一 run 方法重复触发）。
 *
 * 接口纪律：
 * - 发起永不 reject：成功回结果、失败回空且 error 置位（error 是唯一成败判据）；
 * - 不持任务结果：数据存取归调用方（成功结果从 run 返回值取）；
 * - 无生命周期钩子、无 immediate：首跑时序归调用方。
 */

/** toast sink 最小结构面：模块默认策略只用 error 一路；
 * naive-ui 的 MessageApi 结构相容，可在消息提供器内直接注册。 */
export interface ToastSink {
  error: (content: string) => void
}

/** 模块级单点 sink：应用入口在消息提供器内注册（useMessage 只在该上下文可用），
 * 注册前为 no-op，测试注入假 sink。sink 与策略正交。 */
let toastSink: ToastSink = { error: () => {} }

/** 应用入口注册 toast sink（须在 NMessageProvider 的组件上下文内调用）；重复注册即覆盖。 */
export function registerToastSink(sink: ToastSink): void {
  toastSink = sink
}

/** 错误展示策略：默认统一 toast——策略收口此一处，全局换策略只碰这里（不设注入机制）。 */
function showErrorToast(message: string): void {
  toastSink.error(message)
}

export function useLoadable<T>(task: () => Promise<T>) {
  const loading = ref(false)
  const error = ref<string | null>(null)

  // 请求序号守卫：竞态后发覆盖先发，终态 = 最后一次发起的结果；
  // 迟到的前发结果连同其 loading 收尾（及错误 toast）一并作废。
  let seq = 0

  /** 发起（「刷新」即再次发起）：永不 reject——成功回结果、失败回空且 error 置位。 */
  async function run(): Promise<T | null> {
    const mySeq = ++seq
    loading.value = true
    try {
      const result = await task()
      if (mySeq !== seq) return null
      error.value = null
      return result
    } catch (e) {
      if (mySeq !== seq) return null
      error.value = errorMessage(e)
      showErrorToast(error.value)
      return null
    } finally {
      if (mySeq === seq) loading.value = false
    }
  }

  return { loading, error, run }
}
