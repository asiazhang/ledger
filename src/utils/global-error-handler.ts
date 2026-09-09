import type { App } from 'vue'
import type { Pinia } from 'pinia'
import { useRenderErrorsStore } from '@/stores/render-errors'
import { api } from '@/api'

/**
 * 全局渲染错误兜底（issue #926）：`app.config.errorHandler` 单点安装。
 *
 * 动机：渲染层异常（组件 render / 生命周期钩子 / 侦听器回调抛错）默认只进
 * console——生产 WebView 控制台不可见，用户只见「界面冻住」不见报错（见
 * 走势图表注册缺失事故）。本模块把这类错误变为两个可见面：
 *  1. 非阻断全局错误提示条（render-errors store → GlobalErrorBanner），
 *     非模态环境指示，不接弹层注册表、不抑制快捷键（GlobalBusyBar 同口径）；
 *  2. 回传后端日志（`log_frontend_error` IPC，tracing ERROR 落盘），带
 *     防洪水护栏：同一错误文案去重、每秒至多 1 条、每会话至多 50 条——
 *     超限后仅更新提示条、不再发 IPC（错误循环不刷爆日志）。
 *
 * 语义边界：errorHandler 不重抛、不替代既有错误通道——Loadable 已捕获的
 * IPC 错误照旧走 toast，不会到达本兜底；到达这里的都是无主渲染错误。
 * 必须在 pinia 安装后调用（handler 内经传入的 pinia 实例解析 store）。
 */

/** 每会话回传上限：超过后只亮提示条、不再发 IPC（错误风暴护栏） */
const MAX_FORWARDS = 50

/** 同文案去重窗口（毫秒）：错误循环里同一条错误不重复回传 */
const DEDUPE_WINDOW_MS = 3000

/** 单条回传载荷的长度上限（message / stack 各自截断） */
const MESSAGE_LIMIT = 200
const STACK_LIMIT = 600

export function installGlobalErrorHandler(app: App, pinia: Pinia): void {
  const store = useRenderErrorsStore(pinia)

  let lastForwardedMessage: string | null = null
  let lastForwardedAt = 0
  let forwardedCount = 0

  function forwardToBackend(summary: string, error: unknown): void {
    if (forwardedCount >= MAX_FORWARDS) return
    const now = Date.now()
    if (summary === lastForwardedMessage && now - lastForwardedAt < DEDUPE_WINDOW_MS) return
    lastForwardedMessage = summary
    lastForwardedAt = now
    forwardedCount += 1
    const err = error as { stack?: string } | null
    const stack = typeof err?.stack === 'string' ? err.stack.slice(0, STACK_LIMIT) : ''
    // 回传失败静默：诊断通道自身绝不成为第二错误源（rejection 就地消化）
    api
      .logFrontendError(stack ? `${summary}\n${stack}` : summary)
      .catch(() => {})
  }

  app.config.errorHandler = (err, _instance, info) => {
    const raw = err instanceof Error ? err.message : String(err)
    // 提示条文案：摘要 + 场景信息，帮助用户反馈时说清「哪一步炸的」
    const summary = `${raw}（${info}）`.slice(0, MESSAGE_LIMIT)
    store.report(summary)
    forwardToBackend(summary, err)
  }
}
