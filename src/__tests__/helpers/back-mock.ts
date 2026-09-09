import { onBackButtonPress } from '@tauri-apps/api/app'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { vi, type Mock } from 'vitest'

/**
 * 测试侧系统返回桥接 mock 的统一入口（单一事实源，与 listen-mock 同纪律）。
 *
 * 布线（setup.ts）：vi.mock 替换 '@tauri-apps/api/app' 与 '@tauri-apps/api/window'
 * 两模块；beforeEach 复位后装默认实现（注册即成功、unregister 为 no-op）。
 * 需要驱动返回键或断言注册/撤销/销毁的用例经本文件助手安装捕获实现。
 *
 * - captureBackHandler：捕获 useSystemBack 注册的处理器，返回触发函数——
 *   模拟 Android 返回键到达（payload.canGoBack 可省，默认 false = 栈底）。
 * - captureBackRegistration：捕获注册并暴露 unregister spy——断言换档/卸载撤销。
 * - mockWindowDestroy：装配 getCurrentWindow().destroy 替身——断言「栈底交还系统」。
 */
export type BackPressHandler = (payload?: { canGoBack?: boolean }) => void

type RegisterBackHandler = (
  handler: BackPressHandler,
) => Promise<{ unregister: () => Promise<void> }>

export const mockOnBackButtonPress = vi.mocked(onBackButtonPress) as unknown as Mock<RegisterBackHandler>

/** 安装捕获实现：返回触发函数（模拟返回键到达；未注册时触发为 no-op） */
export function captureBackHandler(): (payload?: { canGoBack?: boolean }) => void {
  let handler: BackPressHandler | null = null
  mockOnBackButtonPress.mockImplementation((h) => {
    handler = h
    return Promise.resolve({ unregister: () => Promise.resolve() })
  })
  return (payload) => handler?.({ canGoBack: false, ...payload })
}

/** 安装带撤销 spy 的捕获实现：返回读取函数（尚未注册时为 null） */
export function captureBackRegistration(): () => { handler: BackPressHandler; unregister: Mock } | null {
  let captured: { handler: BackPressHandler; unregister: Mock } | null = null
  mockOnBackButtonPress.mockImplementation((h) => {
    const unregister = vi.fn().mockResolvedValue(undefined)
    captured = { handler: h, unregister }
    return Promise.resolve({ unregister: () => unregister() })
  })
  return () => captured
}

/** 装配并读取 getCurrentWindow().destroy 替身（栈底交还系统通道） */
export function mockWindowDestroy(): Mock<() => Promise<void>> {
  const destroy = vi.fn().mockResolvedValue(undefined)
  vi.mocked(getCurrentWindow).mockReturnValue({ destroy } as never)
  return destroy
}
