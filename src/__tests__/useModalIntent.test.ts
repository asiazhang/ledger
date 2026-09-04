import { describe, expect, it } from 'vitest'
import { nextTick, watch } from 'vue'
import { useModalIntent } from '@/composables/useModalIntent'

/**
 * ModalIntent（弹窗意图编排）模块测试（ADR-0072，词汇表「ModalIntent」词条）：
 * 工厂零外部依赖（无 store、无 api、无组件），直接实例化直打接口——
 * 意图（开/重开/关/带载荷开）→ 可观察状态终态与序号递增，不断言内部实现拓扑。
 */

/** 测试用意图闭集：带载荷判别联合（形态照消费方声明）。 */
type TestIntent =
  | { type: 'create' }
  | { type: 'edit'; rowId: string }

describe('useModalIntent 初始状态', () => {
  it('意图为 null（关闭终态，显示由「意图非空」派生）、序号 0', () => {
    const modal = useModalIntent<TestIntent>()
    expect(modal.intent.value).toBeNull()
    expect(modal.seq.value).toBe(0)
  })
})

describe('useModalIntent open（纯同步）', () => {
  it('open：意图落位、序号递增；返回后立即观察得到（无异步、无中间态）', () => {
    const modal = useModalIntent<TestIntent>()
    const payload = { type: 'edit', rowId: 'r1' } as const
    modal.open(payload)
    expect(modal.intent.value).toEqual({ type: 'edit', rowId: 'r1' })
    expect(modal.intent.value!.type).toBe('edit')
    expect(modal.seq.value).toBe(1)
  })

  it('同载荷重开：意图为全新对象（重触发消费方响应）、序号再递增', () => {
    const modal = useModalIntent<TestIntent>()
    // 复用同一引用重开：全新对象由工厂机制保证（不依赖调用方每次传新字面量）
    const samePayload = { type: 'edit', rowId: 'r1' } as const
    modal.open(samePayload)
    const first = modal.intent.value
    modal.open(samePayload)
    expect(modal.intent.value).not.toBe(first)
    expect(modal.intent.value).not.toBe(samePayload)
    expect(modal.intent.value).toEqual({ type: 'edit', rowId: 'r1' })
    expect(modal.seq.value).toBe(2)
  })

  it('同载荷重开：意图 watch 消费方重触发（全新对象落位）', async () => {
    const modal = useModalIntent<TestIntent>()
    const samePayload = { type: 'edit', rowId: 'r1' } as const
    modal.open(samePayload)
    let fires = 0
    watch(modal.intent, () => {
      fires += 1
    })
    modal.open(samePayload)
    await nextTick() // watch 回调随 pre 队列异步刷新
    expect(fires).toBe(1)
  })

  it('换目标重开：意图更新为最新、序号继续递增', () => {
    const modal = useModalIntent<TestIntent>()
    modal.open({ type: 'edit', rowId: 'r1' })
    modal.open({ type: 'edit', rowId: 'r2' })
    expect(modal.intent.value).toEqual({ type: 'edit', rowId: 'r2' })
    expect(modal.seq.value).toBe(2)
  })
})

describe('useModalIntent close', () => {
  it('close：意图清回 null 终态、序号不重置；返回后立即观察得到', () => {
    const modal = useModalIntent<TestIntent>()
    modal.open({ type: 'create' })
    modal.close()
    expect(modal.intent.value).toBeNull()
    expect(modal.seq.value).toBe(1)
  })

  it('关闭后可重开：序号在关闭序号之上继续递增', () => {
    const modal = useModalIntent<TestIntent>()
    modal.open({ type: 'create' })
    modal.close()
    modal.open({ type: 'create' })
    expect(modal.intent.value).toEqual({ type: 'create' })
    expect(modal.seq.value).toBe(2)
  })

  it('未开启时 close 幂等：意图保持 null', () => {
    const modal = useModalIntent<TestIntent>()
    modal.close()
    expect(modal.intent.value).toBeNull()
  })
})

describe('useModalIntent 工厂形态', () => {
  it('每次调用返回独立实例：意图与序号互不串扰', () => {
    const first = useModalIntent<TestIntent>()
    const second = useModalIntent<TestIntent>()
    first.open({ type: 'create' })
    expect(first.intent.value).toEqual({ type: 'create' })
    expect(first.seq.value).toBe(1)
    expect(second.intent.value).toBeNull()
    expect(second.seq.value).toBe(0)
  })
})
