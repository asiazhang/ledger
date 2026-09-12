import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createApp } from 'vue'
import { createPinia } from 'pinia'
import { mockInvoke, wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { installGlobalErrorHandler } from '@/utils/global-error-handler'
import { useRenderErrorsStore } from '@/stores/render-errors'

/**
 * 全局渲染错误兜底测试（issue #926）：errorHandler 的三个可见面——
 * 提示条状态落位、后端回传、防洪水护栏（同文案去重 / 每会话上限）。
 */

type ErrorInfo = string

function makeApp() {
  const app = createApp({ template: '<div />' })
  const pinia = createPinia()
  app.use(pinia)
  installGlobalErrorHandler(app, pinia)
  return app
}

function errorHandlerOf(app: ReturnType<typeof makeApp>): (err: unknown, info: ErrorInfo) => void {
  expect(app.config.errorHandler, '安装后应存在全局 errorHandler').toBeTruthy()
  return (err, info) => app.config.errorHandler!(err, null, info)
}

beforeEach(() => {
  wireInvokeSeam()
})

describe('installGlobalErrorHandler 渲染错误兜底', () => {
  it('渲染错误落位提示条 store（非阻断可见面）', async () => {
    const app = makeApp()
    const store = useRenderErrorsStore(app.config.globalProperties.$pinia)
    expect(store.message).toBeNull()
    errorHandlerOf(app)(new Error('boom-render'), 'render')
    expect(store.message).toContain('boom-render')
    expect(store.message).toContain('render')
    // 用户可关闭
    store.dismiss()
    expect(store.message).toBeNull()
  })

  it('回传后端日志：log_frontend_error 携带摘要（含堆栈首段）', async () => {
    const app = makeApp()
    errorHandlerOf(app)(Object.assign(new Error('chart-exploded'), { stack: 'Error: chart-exploded\n  at line-1' }), 'render')
    await vi.waitFor(() => {
      expect(mockInvoke.mock.calls.some(([c]) => c === 'log_frontend_error')).toBe(true)
    })
    const call = mockInvoke.mock.calls.filter(([c]) => c === 'log_frontend_error').at(-1)!
    const payload = call[1] as { message: string }
    expect(payload.message).toContain('chart-exploded')
    expect(payload.message).toContain('at line-1')
  })

  it('防洪水：同一文案窗口内去重，不重复回传', async () => {
    const app = makeApp()
    const handler = errorHandlerOf(app)
    handler(new Error('same-loop'), 'render')
    handler(new Error('same-loop'), 'render')
    handler(new Error('same-loop'), 'render')
    // 等动态 import 的 api 模块就绪后再断言
    await vi.waitFor(() => {
      expect(mockInvoke.mock.calls.some(([c]) => c === 'log_frontend_error')).toBe(true)
    })
    await Promise.resolve()
    const forwards = mockInvoke.mock.calls.filter(([c]) => c === 'log_frontend_error')
    expect(forwards.length).toBe(1)
  })

  it('不同文案各自回传（去重只按文案）', async () => {
    const app = makeApp()
    const handler = errorHandlerOf(app)
    handler(new Error('first'), 'render')
    handler(new Error('second'), 'render')
    await vi.waitFor(() => {
      const forwards = mockInvoke.mock.calls.filter(([c]) => c === 'log_frontend_error')
      expect(forwards.length).toBe(2)
    })
  })

  it('会话上限：超过 50 条后停止回传（错误风暴不刷爆日志）', async () => {
    const app = makeApp()
    const handler = errorHandlerOf(app)
    for (let i = 0; i < 60; i += 1) {
      handler(new Error(`flood-${i}`), 'render')
    }
    await vi.waitFor(() => {
      const forwards = mockInvoke.mock.calls.filter(([c]) => c === 'log_frontend_error')
      expect(forwards.length).toBe(50)
    })
    // 上限后不再新增
    handler(new Error('flood-after-cap'), 'render')
    await Promise.resolve()
    expect(mockInvoke.mock.calls.filter(([c]) => c === 'log_frontend_error').length).toBe(50)
  })
})
