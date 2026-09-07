import { invoke } from '@tauri-apps/api/core'
import { expect, vi, type Mock } from 'vitest'
import { useReferenceStore } from '@/stores/reference'
// 注：与 reference-stubs.ts 构成模块环（本文件引 REFERENCE_DEFAULTS，对岸薄别名
// 引回 wireInvokeSeam）——守门脚本把 REFERENCE_DEFAULTS 登记处钉在
// reference-stubs.ts、既有使用者从该文件导入别名，环为迁移窗口期的结构必然；
// 双方绑定都只在函数体内使用，无模块求值期取值，收尾票（#750）删除别名后即消解。
import { REFERENCE_DEFAULTS } from './reference-stubs'

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

// —— invoke 测试接缝（issue #746，ADR-0085 决策 1；术语见测试基础设施域词汇表） ——

/**
 * 未命中报错基座：按命令名拒绝。全局壳层在每测桩复位后重挂本实现，
 * 拼错或漏布线的命令立即让测试变红并报出命令名（而非 undefined 引发的
 * 产品代码 TypeError）；走接缝的测试以自己的应答表覆盖之。
 */
export function unexpectedInvoke(cmd: string): Promise<never> {
  return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
}

/**
 * 接缝求值序的静态兜底层：defaults 表 → 参考数据预热 → 未命中报错。
 * 参考字典的五个 list 命令由桩层规范夹具（REFERENCE_DEFAULTS，含软删行）兜底
 * 应答，测试不得在 defaults 表重复枚举。
 */
export function resolveSeamFallback(
  cmd: string,
  defaults: Record<string, unknown> = {},
): Promise<unknown> {
  if (cmd in defaults) return Promise.resolve(defaults[cmd])
  if (cmd in REFERENCE_DEFAULTS) return Promise.resolve(REFERENCE_DEFAULTS[cmd])
  return unexpectedInvoke(cmd)
}

/**
 * defaults 表成员：命令契约快照，只收静态值；函数归 overrides 表。
 * 约束以文档约定与评审守门（TS 无法从 `object` 分支排除函数形态）：把函数
 * 写进 defaults 属用例缺陷——会被 `Promise.resolve` 当静态值兑底而非调用。
 */
export type InvokeSeamStaticValue =
  | string
  | number
  | boolean
  | null
  | undefined
  | object

/** overrides 表成员：函数（计数、按参分支、一次性失败）或静态值，优先级高于 defaults。 */
export type InvokeSeamOverride =
  | ((args?: Record<string, unknown>) => unknown)
  | InvokeSeamStaticValue

export interface InvokeSeamOptions {
  /** defaults 表：本场景下命令契约的静态快照；参考字典命令不得在此重复枚举。 */
  defaults?: Record<string, InvokeSeamStaticValue>
  /** overrides 表：用例级覆盖，可含函数；求值时函数以 args 调用，非 thenable 返回值包装为 resolved promise。 */
  overrides?: Record<string, InvokeSeamOverride>
  /**
   * store 层参考数据预热（opt-in，默认关）：接线后代做参考 store 的预载刷新，
   * 就绪信号经分发器的 `ready` 发放。刷新时序与断言耦合，默认开启会破坏
   * 「迁移纯机械替换」判据，故仅需要参考 store 预载的用例显式开启。
   */
  refreshReferenceStores?: boolean
}

/**
 * 接缝分发器：既是挂在全局替身上的应答函数（供 `mockImplementationOnce` 委托），
 * 也是调用事实断言的观察点（配合 `mockInvoke.mock.calls` / `lastInvokeArgs`）。
 * `ready` 仅在 `refreshReferenceStores: true` 时存在，为 store 层预热的就绪信号。
 */
export type InvokeSeamDispatcher = ((
  cmd: string,
  args?: Record<string, unknown>,
) => Promise<unknown>) & {
  ready?: Promise<void>
}

function isThenable(value: unknown): value is Promise<unknown> {
  return typeof (value as Promise<unknown> | undefined)?.then === 'function'
}

/**
 * invoke 测试接缝的唯一入口：组装接线一体（issue #746，ADR-0085 决策 1）。
 *
 * 一次调用内部完成：应答表挂上全局替身（`mockInvoke.mockImplementation`）
 * + 每测清理注册（接线替身只活当前测试——全局壳层每测桩复位后重挂未命中
 * 报错基座，本文件与壳层同批落地，接线不外泄由壳层保证）。返回分发器供
 * `mockImplementationOnce` 委托与调用事实断言。
 *
 * 求值序（既有语义，使用者零迁移成本）：overrides 表 → defaults 表 →
 * 参考数据兜底 → 未命中 reject。defaults 表只收静态值（类型层排除函数），
 * overrides 表可含函数的两表分工是接口语义的一部分。
 *
 * 典型用法（beforeEach 内）：
 * ```ts
 * const base = wireInvokeSeam({ defaults: { get_snapshot: snapshot } })
 * // 一次性桩：领域命令自己接，其余委托回接缝
 * mockInvoke.mockImplementationOnce((cmd, args) =>
 *   cmd === 'create_transaction' ? Promise.resolve('new-id') : base(cmd, args))
 * ```
 */
export function wireInvokeSeam(options: InvokeSeamOptions = {}): InvokeSeamDispatcher {
  const defaults = options.defaults ?? {}
  const overrides = options.overrides ?? {}
  const dispatch = ((cmd: string, args?: Record<string, unknown>): Promise<unknown> => {
    if (cmd in overrides) {
      const handler = overrides[cmd]
      if (typeof handler === 'function') {
        const out = (handler as (a?: Record<string, unknown>) => unknown)(args)
        return isThenable(out) ? out : Promise.resolve(out)
      }
      return Promise.resolve(handler)
    }
    return resolveSeamFallback(cmd, defaults)
  }) as InvokeSeamDispatcher
  mockInvoke.mockImplementation(dispatch as typeof invoke)
  if (options.refreshReferenceStores) {
    dispatch.ready = useReferenceStore().refresh().then(() => undefined)
  }
  return dispatch
}
