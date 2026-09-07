import { vi } from 'vitest'

/**
 * naive-ui 消息接口的稳定替身实例（issue #746，ADR-0085 决策 4）。
 *
 * 机制仍是全局替身统一替换（setup.ts 的 `vi.mock('naive-ui')` 经本出口发放），
 * 只变发放策略：从「每次调用返回新实例」改为「每测发放同一稳定实例 + 全局
 * 每测自动清零」。需要断言消息的测试经本出口读取实例，不再自建并行实例
 * （历史 `vi.hoisted` 自建样板随迁移批次删除）。
 */
export const messageApi = {
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
  info: vi.fn(),
  loading: vi.fn(),
  destroyAll: vi.fn(),
}

/** 消息接口清零（清理四件套之一，由全局壳层每测执行）。 */
export function resetMessageApi(): void {
  messageApi.success.mockReset()
  messageApi.warning.mockReset()
  messageApi.error.mockReset()
  messageApi.info.mockReset()
  messageApi.loading.mockReset()
  messageApi.destroyAll.mockReset()
}
