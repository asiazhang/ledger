import { invoke } from '@tauri-apps/api/core'
import { expect, vi, type Mock } from 'vitest'

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

/**
 * 最近一次指定命令调用的 args（单一事实源：`.mock.calls` 元组的 args 位可选，
 * 测试体直取会得到 `Record | undefined`）。本应用全部命令均携带对象 args，
 * 缺失即用例缺陷——经 expect 守卫前置暴露，不在测试体散布非空断言。
 */
export function lastInvokeArgs(cmd: string): Record<string, unknown> {
  const call = mockInvoke.mock.calls.filter(([c]) => c === cmd).at(-1)
  expect(call, `应已调用 ${cmd}`).toBeTruthy()
  expect(call![1], `调用 ${cmd} 应携带对象 args`).toBeDefined()
  return call![1] as Record<string, unknown>
}
