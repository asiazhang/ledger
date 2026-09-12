import { listen } from '@tauri-apps/api/event'
import { vi, type Mock } from 'vitest'

/**
 * 测试侧 tauri 事件 listen mock 的统一入口（单一事实源，与 invoke-mock 同纪律）。
 *
 * 为什么不直接用 `vi.mocked(listen)`：tauri 的 `listen<T>` 泛型与 `EventCallback<T>`
 * （handler 形参为必填 `Event<T>` 对象、返回 `UnlistenFn`）使每份测试要么手写不匹配
 * 的实现签名（`(...args: unknown[]) => void` / `handler: never` / 裸 `Promise<void>`），
 * 要么在触发端散布 Event 形状。经本助手在单点收窄为测试实际行使的形态：
 * - 捕获的监听器 evt 形参可选且字段可选：测试只表达「信号到达」——既可按 tauri
 *   Event 形状触发（`{ event, payload }`），也可带部分字段（`{ payload }`）或零参触发；
 *   tauri 真 handler（必填 Event 形参）仍可赋值进本形态。
 * - 实现返回真实的 unlisten 函数形态；各测试在 `mockReset()` 后重装实现。
 *
 * 安装实现只经下方两个助手：多监听器捕获用 `captureListenHandlers()`，
 * 单监听器（最后注册者胜）用 `captureLastListener()`；不重装实现、只断言注册
 * 形态（mockResolvedValue 等）的场景可直用 `mockListen`。
 */
export type CapturedListener = (evt?: { event?: string; payload?: unknown }) => void

/** 本应用 listen mock 的实现签名（event 一律字符串事件名）。 */
export type AppListenHandler = (event: string, handler: CapturedListener) => Promise<() => void>

export const mockListen = vi.mocked(listen) as unknown as Mock<AppListenHandler>

/**
 * 安装统一捕获实现：所有 listen 注册的监听器进同一数组（多个 store 各自订阅同一
 * 信号的场景全部捕获、全部触发）；返回数组供用例按需触发。beforeEach 内
 * `mockListen.mockReset()` 之后调用一次。
 */
export function captureListenHandlers(): CapturedListener[] {
  const handlers: CapturedListener[] = []
  mockListen.mockImplementation((_event, handler) => {
    handlers.push(handler)
    return Promise.resolve(() => {})
  })
  return handlers
}

/**
 * 安装单监听器捕获实现（最后注册者胜）；返回读取函数，尚未捕获时为 null。
 * beforeEeach 内 `mockListen.mockReset()` 之后调用一次。
 */
export function captureLastListener(): () => CapturedListener | null {
  let handler: CapturedListener | null = null
  mockListen.mockImplementation((_event, h) => {
    handler = h
    return Promise.resolve(() => {})
  })
  return () => handler
}
