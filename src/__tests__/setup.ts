import { vi, beforeEach, afterEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { enableAutoUnmount } from '@vue/test-utils'
import { invoke } from '@tauri-apps/api/core'
import { mockInvoke, unexpectedInvoke } from './helpers/invoke-mock'
import { mockListen } from './helpers/listen-mock'
import { messageApi, resetMessageApi } from './helpers/message-mock'

// jsdom 环境下 localStorage 不可用，使用 polyfill
if (typeof localStorage === 'undefined' || localStorage === null) {
  const store: Record<string, string> = {}
  const mockStorage = {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, value: string) => { store[key] = value },
    removeItem: (key: string) => { delete store[key] },
    clear: () => { Object.keys(store).forEach((k) => delete store[k]) },
    get length() { return Object.keys(store).length },
    key: (index: number) => Object.keys(store)[index] ?? null,
  }
  Object.defineProperty(globalThis, 'localStorage', { value: mockStorage })
}

// Mock Tauri IPC invoke - 所有测试共享同一 mock
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

// Mock Tauri event listener
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}))

// jsdom 缺少 matchMedia
if (typeof window !== 'undefined' && !window.matchMedia) {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  })
}

// 组件卸载入全局壳层（issue #746，ADR-0085 决策 2）：enableAutoUnmount 全局只允许
// 注册一次，而 32 个测试文件自带 `enableAutoUnmount(afterEach)`（迁移批次才删除）。
// 此处把出口包装为幂等：首次调用（本 setup，先于任何测试文件求值）完成注册，
// 后续文件级调用降级为 no-op——文件挂载的 wrapper 同样被首次注册的全局钩子
// 跟踪并在每测 afterEach 统一卸载，语义不丢失。
vi.mock('@vue/test-utils', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@vue/test-utils')>()
  let registered = false
  return {
    ...actual,
    enableAutoUnmount: (hook: (cb: () => void) => void) => {
      if (registered) return
      registered = true
      actual.enableAutoUnmount(hook)
    },
  }
})

// Mock Naive UI useMessage（Composable 中使用它显示通知）——
// 每测发放同一稳定实例 + 全局每测自动清零（ADR-0085 决策 4），实例经
// helpers/message-mock.ts 出口获取，供需要断言消息的测试读取。
vi.mock('naive-ui', async (importOriginal) => {
  const actual = await importOriginal<typeof import('naive-ui')>()
  return {
    ...actual,
    useMessage: () => messageApi,
  }
})

// —— 全局清理基座（清理四件套，issue #746 / ADR-0085 决策 2，对全部测试文件生效） ——
// 每测自动执行：桩复位后重挂未命中报错基座、状态容器（Pinia）重置、本地存储
// 清空、消息接口清零；随同自动执行的还有组件卸载（上方 enableAutoUnmount）
// 与文档体清空（下方 afterEach）。
beforeEach(() => {
  mockInvoke.mockReset()
  mockInvoke.mockImplementation(unexpectedInvoke as typeof invoke)
  mockListen.mockReset()
  mockListen.mockResolvedValue(vi.fn())
  setActivePinia(createPinia())
  localStorage.clear()
  resetMessageApi()
})

enableAutoUnmount(afterEach)

afterEach(() => {
  document.body.innerHTML = ''
})
