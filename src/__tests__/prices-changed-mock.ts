/**
 * 价格消费方组件级测试的共享 mock（issue #238 / ADR-0031 决策 3）：
 * 替换 `usePricesChanged` 订阅接缝，捕获订阅回调，供测试手动触发模拟
 * 后端 emit。`vi.mock` 注册须落在各测试文件（vitest 按文件 mock 模块），
 * 工厂内经动态 import 引用本模块（mock 工厂引用外部辅助的官方推荐方式）。
 *
 * 捕获形态为**订阅列表**：真实接缝是 tauri `listen` 的多订阅者语义（消费方
 * 自选订阅、各自重拉，issue #1195 起同视图多消费方并存），mock 与之对齐。
 */
const state = {
  handlers: [] as (() => void)[],
}

/** vi.mock 工厂体内调用：捕获订阅回调 */
export function capturePricesChangedHandler(cb: () => void): void {
  state.handlers.push(cb)
}

/** 模拟后端 emit：逐订阅者触发（未订阅时报错而非静默） */
export function firePricesChanged(): void {
  if (state.handlers.length === 0) {
    throw new Error('usePricesChanged 未被订阅：先在 vi.mock 工厂中接好 capturePricesChangedHandler')
  }
  for (const handler of state.handlers) {
    handler()
  }
}

/** 测试间重置捕获的回调 */
export function resetPricesChangedHandler(): void {
  state.handlers = []
}
