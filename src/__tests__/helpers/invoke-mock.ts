import { invoke } from '@tauri-apps/api/core'
import { vi, type Mock } from 'vitest'

/**
 * 测试侧 invoke mock 的统一入口（单一事实源）。
 *
 * 为什么不用 `vi.mocked(invoke)`：tauri 的 `InvokeArgs = Record<string, unknown> |
 * number[]`，其中 `number[]` 是 IPC 缓冲区保留形态，本应用全部命令的 args 均为对象。
 * 测试 handler 若按 `InvokeArgs` 书写，每个函数体都得先窄化联合（`args as {…}`
 * 断言散布到全部测试）；经本助手在单点收窄为对象形态，测试体直接按 `Record` 访问、
 * 零断言。若未来真的出现非对象 args 的命令，只放宽这一处——失败面收敛在单点。
 */
export type AppInvokeHandler = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>

export const mockInvoke = vi.mocked(invoke) as unknown as Mock<AppInvokeHandler>
