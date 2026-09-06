import { describe, expect, it, vi } from 'vitest'
import { useFocusParam } from '@/composables/useFocusParam'
import type { LocationQuery } from 'vue-router'

/**
 * focus 消费助手模块测试（spec #704 / issue #705，词汇表「实体定位参数
 * （focus 参数）」）：工厂零外部依赖（不接 router、不接 store、不接组件），
 * 以普通 query 对象直打——读一次语义（消费即失效、空转封闸、迟到丢弃、
 * 刷新经新实例重定位）全部落在 consume() 的可观察行为上。
 */

/** 快捷构造：以给定 query 对象建厂（getter 包一层，工厂只经它读取）。 */
function make(query: LocationQuery, onFocus: (id: string) => void = vi.fn()) {
  return { helper: useFocusParam({ query: () => query, onFocus }), onFocus }
}

describe('useFocusParam 无 focus：安全空转', () => {
  it('query 无 focus 参数：consume 不回调、不报错', () => {
    const { helper, onFocus } = make({})
    helper.consume()
    expect(onFocus).not.toHaveBeenCalled()
  })

  it('focus 为空串 / null（?focus）：同为空转，不回调', () => {
    const empty = make({ focus: '' })
    empty.helper.consume()
    expect(empty.onFocus).not.toHaveBeenCalled()

    const bare = make({ focus: null })
    bare.helper.consume()
    expect(bare.onFocus).not.toHaveBeenCalled()
  })
})

describe('useFocusParam 消费一次', () => {
  it('focus 在场：consume 回调一次并携带实体 id', () => {
    const { helper, onFocus } = make({ focus: 'pol1' })
    helper.consume()
    expect(onFocus).toHaveBeenCalledTimes(1)
    expect(onFocus).toHaveBeenCalledWith('pol1')
  })

  it('读一次后失效：同一实例重复 consume 不再回调（URL 残留不反复打扰）', () => {
    const { helper, onFocus } = make({ focus: 'pol1' })
    helper.consume()
    helper.consume()
    helper.consume()
    expect(onFocus).toHaveBeenCalledTimes(1)
  })

  it('迟到消费丢弃：已消费后 query 换了新 focus 值也不回调', () => {
    const query: LocationQuery = { focus: 'pol1' }
    const { helper, onFocus } = make(query)
    helper.consume()
    query.focus = 'pol2' // 同实例后续到达的新定位意图
    helper.consume()
    expect(onFocus).toHaveBeenCalledTimes(1)
    expect(onFocus).toHaveBeenCalledWith('pol1')
  })

  it('空转即封闸：首次 consume 无 focus，此后 focus 在场也不再消费（挂载时消费一次即失效）', () => {
    const query: LocationQuery = {}
    const { helper, onFocus } = make(query)
    helper.consume() // 挂载（无 focus）：安全空转，同时耗尽本实例唯一一次读取
    query.focus = 'pol1'
    helper.consume()
    expect(onFocus).not.toHaveBeenCalled()
  })
})

describe('useFocusParam query 形态防御', () => {
  it('重复键 ?focus=a&focus=b：取第一个值', () => {
    const { helper, onFocus } = make({ focus: ['a', 'b'] })
    helper.consume()
    expect(onFocus).toHaveBeenCalledWith('a')
  })

  it('数组首元为 null：按空转处理，不回调', () => {
    const { helper, onFocus } = make({ focus: [null, 'a'] })
    helper.consume()
    expect(onFocus).not.toHaveBeenCalled()
  })
})

describe('useFocusParam 不写回 URL 与实例独立', () => {
  it('消费不改写 query 对象（不写回 URL 的工厂侧形态：无 router 依赖、只读）', () => {
    const query: LocationQuery = { focus: 'pol1', tab: 'policies' }
    const snapshot = { ...query }
    const { helper } = make(query)
    helper.consume()
    expect(query).toEqual(snapshot)
  })

  it('多实例互不串扰：各自消费一次（页面刷新 = 新实例 = 重定位的机制基础）', () => {
    const query: LocationQuery = { focus: 'pol1' }
    const a = make(query)
    const b = make(query)
    a.helper.consume()
    b.helper.consume()
    expect(a.onFocus).toHaveBeenCalledTimes(1)
    expect(b.onFocus).toHaveBeenCalledTimes(1)
  })
})
