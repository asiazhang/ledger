import { describe, it, expect, vi, beforeEach } from 'vitest'

// restart_app 命令单点 mock：断言调用与后续重载编排，不触真实后端。
const restartApp = vi.fn()
vi.mock('@/api', () => ({
  api: {
    restartApp: (...args: unknown[]) => restartApp(...args),
  },
}))

import { restartAppShortly } from '@/utils/restart'

const reload = vi.fn()

describe('restartAppShortly（原位重引导 + WebView 重载，issue #644）', () => {
  beforeEach(() => {
    restartApp.mockReset()
    reload.mockReset()
    // jsdom 不实现导航：以可配置属性替换 window.location，捕获 reload 调用。
    Object.defineProperty(window, 'location', {
      configurable: true,
      writable: true,
      value: { ...window.location, reload },
    })
    vi.useFakeTimers()
  })

  it('重启命令成功后重载 WebView：重新探测启动相位，落到正确首屏', async () => {
    restartApp.mockResolvedValue(undefined)
    restartAppShortly()
    expect(restartApp).not.toHaveBeenCalled()
    vi.advanceTimersByTime(900)
    await vi.waitFor(() => expect(reload).toHaveBeenCalledTimes(1))
  })

  it('重启命令失败：不重载、保留当前界面（仍可操作），错误进 console', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    restartApp.mockRejectedValue({ kind: 'Db', message: '重启失败' })
    restartAppShortly()
    vi.advanceTimersByTime(900)
    await vi.waitFor(() => expect(warn).toHaveBeenCalled())
    expect(reload).not.toHaveBeenCalled()
    warn.mockRestore()
  })
})
