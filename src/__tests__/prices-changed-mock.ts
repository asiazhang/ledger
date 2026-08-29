/**
 * 价格消费方组件级测试的共享 mock（issue #238 / ADR-0031 决策 3）：
 * 替换 `usePricesChanged` 订阅接缝，捕获订阅回调，供测试手动触发模拟
 * 后端 emit。`vi.mock` 注册须落在各测试文件（vitest 按文件 mock 模块），
 * 工厂内经动态 import 引用本模块（mock 工厂引用外部辅助的官方推荐方式）。
 */
const state = {
  handler: undefined as (() => void) | undefined,
}

/** vi.mock 工厂体内调用：捕获订阅回调 */
export function capturePricesChangedHandler(cb: () => void): void {
  state.handler = cb
}

/** 模拟后端 emit：触发价格失效信号（未订阅时报错而非静默） */
export function firePricesChanged(): void {
  if (typeof state.handler !== 'function') {
    throw new Error('usePricesChanged 未被订阅：先在 vi.mock 工厂中接好 capturePricesChangedHandler')
  }
  state.handler()
}

/** 测试间重置捕获的回调 */
export function resetPricesChangedHandler(): void {
  state.handler = undefined
}
