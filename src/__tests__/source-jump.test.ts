import { describe, expect, it, vi } from 'vitest'
import { resolveSourceJumpTarget } from '@/components/source-jump'
import type { TransactionSourceKind } from '@/components/source-jump'

/**
 * 来源跳转目标计算模块测试（spec #704 / issue #705，词汇表「来源列」
 * 「实体定位参数（focus 参数）」）：纯函数直打——六类来源 × 主项/收纳两态
 * → 路由目标（视图名 + query），断言输出对象形态，不接组件、不接 router。
 */

/** 全主项态收纳谓词：一切目标视图都不在收纳清单。 */
const allMain = () => false
/** 全收纳态收纳谓词：一切目标视图都在收纳清单。 */
const allContained = () => true

const SIX_KINDS: readonly TransactionSourceKind[] = [
  'installmentPlan',
  'subscription',
  'scheduledTransfer',
  'policy',
  'item',
  'instrument',
]

describe('resolveSourceJumpTarget 主项态（独立路由直达）', () => {
  it('分期计划：落定时视图分期页签 + focus', () => {
    expect(resolveSourceJumpTarget('installmentPlan', 'p1', allMain)).toEqual({
      name: 'scheduled',
      query: { tab: 'installments', focus: 'p1' },
    })
  })

  it('订阅计划：落定时视图订阅页签 + focus', () => {
    expect(resolveSourceJumpTarget('subscription', 'p2', allMain)).toEqual({
      name: 'scheduled',
      query: { tab: 'subscriptions', focus: 'p2' },
    })
  })

  it('定时转账计划：落定时视图定时转账页签 + focus', () => {
    expect(resolveSourceJumpTarget('scheduledTransfer', 'p3', allMain)).toEqual({
      name: 'scheduled',
      query: { tab: 'transfers', focus: 'p3' },
    })
  })

  it('保单：落保单独立路由 + focus（收纳分流由路由守卫既有重定向兜底）', () => {
    expect(resolveSourceJumpTarget('policy', 'pol1', allMain)).toEqual({
      name: 'policies',
      query: { focus: 'pol1' },
    })
  })

  it('物品：落物品主项路由 + focus', () => {
    expect(resolveSourceJumpTarget('item', 'it1', allMain)).toEqual({
      name: 'items',
      query: { focus: 'it1' },
    })
  })

  it('标的：落投资主项路由 + focus（走势页签选中归落点侧 focus 消费）', () => {
    expect(resolveSourceJumpTarget('instrument', 'ins1', allMain)).toEqual({
      name: 'investments',
      query: { focus: 'ins1' },
    })
  })
})

describe('resolveSourceJumpTarget 收纳态（组「更多」页签落点）', () => {
  it('计划三形态：落记账「更多」定时页签，形态页签以 scheduledTab 叠加 + focus', () => {
    expect(resolveSourceJumpTarget('installmentPlan', 'p1', allContained)).toEqual({
      name: 'bookkeeping-more',
      query: { tab: 'scheduled', scheduledTab: 'installments', focus: 'p1' },
    })
    expect(resolveSourceJumpTarget('subscription', 'p2', allContained)).toEqual({
      name: 'bookkeeping-more',
      query: { tab: 'scheduled', scheduledTab: 'subscriptions', focus: 'p2' },
    })
    expect(resolveSourceJumpTarget('scheduledTransfer', 'p3', allContained)).toEqual({
      name: 'bookkeeping-more',
      query: { tab: 'scheduled', scheduledTab: 'transfers', focus: 'p3' },
    })
  })

  it('保单收纳态：直达资产「更多」保单页签 + focus（语义先行；路由守卫透传由消费票接线）', () => {
    expect(resolveSourceJumpTarget('policy', 'pol1', allContained)).toEqual({
      name: 'assets-more',
      query: { tab: 'policies', focus: 'pol1' },
    })
  })

  it('物品收纳态：落资产「更多」物品页签 + focus（#474 移入后的用户布局）', () => {
    expect(resolveSourceJumpTarget('item', 'it1', allContained)).toEqual({
      name: 'assets-more',
      query: { tab: 'items', focus: 'it1' },
    })
  })

  it('标的收纳态：落资产「更多」投资页签 + focus', () => {
    expect(resolveSourceJumpTarget('instrument', 'ins1', allContained)).toEqual({
      name: 'assets-more',
      query: { tab: 'investments', focus: 'ins1' },
    })
  })
})

describe('resolveSourceJumpTarget 收纳谓词按目标视图逐一定问', () => {
  it('计划形态只问 scheduled、保单只问 policies（互不误伤）', () => {
    const asked = vi.fn(() => false)
    resolveSourceJumpTarget('installmentPlan', 'p1', asked)
    expect(asked).toHaveBeenCalledTimes(1)
    expect(asked).toHaveBeenCalledWith('scheduled')

    resolveSourceJumpTarget('policy', 'pol1', asked)
    expect(asked).toHaveBeenLastCalledWith('policies')
  })

  it('混合收纳态：定时收纳而保单主项态，各自落对落点', () => {
    const contained = (view: string) => view === 'scheduled'
    expect(resolveSourceJumpTarget('subscription', 'p2', contained)).toEqual({
      name: 'bookkeeping-more',
      query: { tab: 'scheduled', scheduledTab: 'subscriptions', focus: 'p2' },
    })
    expect(resolveSourceJumpTarget('policy', 'pol1', contained)).toEqual({
      name: 'policies',
      query: { focus: 'pol1' },
    })
  })
})

describe('resolveSourceJumpTarget focus 参数统一装配', () => {
  it('六类来源 × 两态共 12 个目标全部携带 focus=<实体 id>', () => {
    for (const kind of SIX_KINDS) {
      for (const contained of [allMain, allContained]) {
        const target = resolveSourceJumpTarget(kind, 'entity-42', contained)
        expect(target.query.focus).toBe('entity-42')
      }
    }
  })
})
