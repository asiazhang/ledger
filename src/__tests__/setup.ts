import { vi } from 'vitest'

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

// Mock Naive UI useMessage（Composable 中使用它显示通知）
vi.mock('naive-ui', async (importOriginal) => {
  const actual = await importOriginal<typeof import('naive-ui')>()
  return {
    ...actual,
    useMessage: () => ({
      success: vi.fn(),
      warning: vi.fn(),
      error: vi.fn(),
      info: vi.fn(),
      loading: vi.fn(),
      destroyAll: vi.fn(),
    }),
  }
})
