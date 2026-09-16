import { vi, type Mock } from 'vitest'

/** 测试支持包只依赖被测包提供的最小注册接口，避免反向依赖造成 workspace 环。 */
export interface ToastSink {
  error: (content: string) => void
}

/**
 * toast sink 假件（issue #1354：双源上收共享测试支持包）：
 * 壳侧 src/__tests__/factories.ts 与 @ledger/loadable、@ledger/scheduled-plan-list
 * 包内测试的局部替身同实现收拢此处——唯一定义点，ToastSink 结构演化时只改这一处
 * （副本防回潮由 check-test-stubs 规则 4 名单 + 本文件白名单守门，issue #1364）。
 * 消费形态：包名深导入 `@ledger/test-support/toast-sink`（exports `./*`
 * 通配）。替身引用被替对象：复位通过调用方传入的产品注册接口完成；测试支持包不反向依赖
 * @ledger/loadable，避免 workspace 依赖环。
 */

/** 假 toast sink：记录 error toast 调用（Loadable 默认策略经 sink 弹出，断言只看 sink 面） */
export function makeFakeSink(): ToastSink & { error: Mock<(content: string) => void> } {
  return { error: vi.fn<(content: string) => void>() }
}

/** 每用例复位 sink 为 no-op，模拟「注册前」默认态，防模块级 sink 状态串扰。 */
export function resetToastSink(register: (sink: ToastSink) => void): void {
  register({ error: () => {} })
}
