import { describe, it, expect } from 'vitest'
import { merchantBars, merchantRankColor } from '@/utils/merchant-chart'
import type { MerchantShare } from '@/types'

// 商户消费排行柱图数据形态纯函数（issue #588）：按后端返回序渲染零口径逻辑、
// 名次梯度色（同色系、第 1 名最深）随名次单调。

/** 从 hsl 字符串提取亮度（percent 数值） */
const lightnessOf = (color: string) => {
  const m = color.match(/hsl\(\d+, \d+%, ([\d.]+)%\)/)
  expect(m).toBeTruthy()
  return Number(m![1])
}

describe('merchantRankColor 名次梯度色（issue #588）', () => {
  it('第 1 名最深（亮度最低档），随名次单调变浅', () => {
    const l0 = lightnessOf(merchantRankColor(0, 5))
    const l1 = lightnessOf(merchantRankColor(1, 5))
    const l4 = lightnessOf(merchantRankColor(4, 5))
    expect(l0).toBeLessThan(l1)
    expect(l1).toBeLessThan(l4)
    expect(l4).toBeLessThanOrEqual(72)
  })

  it('同色系：色相与饱和度恒定，只有亮度随名次变化', () => {
    for (const i of [0, 2, 4]) {
      expect(merchantRankColor(i, 5)).toMatch(/^hsl\(217, 52%, /)
    }
  })

  it('单柱取最深档（第 1 名即末位，不出现除零）', () => {
    expect(merchantRankColor(0, 1)).toBe(merchantRankColor(0, 1))
    expect(lightnessOf(merchantRankColor(0, 1))).toBe(30)
  })
})

describe('merchantBars 图行构建（issue #588）', () => {
  const rows: MerchantShare[] = [
    { merchant_id: 'm-1', merchant_name: '超市', amount_cents: 5000 },
    { merchant_id: 'm-2', merchant_name: '咖啡', amount_cents: 3000 },
    { merchant_id: 'm-3', merchant_name: '书店', amount_cents: 0 },
  ]

  it('后端返回序即柱序：名称、数值逐行对应，零口径逻辑（不重排不过滤）', () => {
    const bars = merchantBars(rows)
    expect(bars.map((b) => b.name)).toEqual(['超市', '咖啡', '书店'])
    expect(bars.map((b) => b.value)).toEqual([5000, 3000, 0])
  })

  it('梯度色随名次单调（柱色与行序一一对应）', () => {
    const bars = merchantBars(rows)
    expect(lightnessOf(bars[0].color)).toBeLessThan(lightnessOf(bars[1].color))
    expect(lightnessOf(bars[1].color)).toBeLessThan(lightnessOf(bars[2].color))
  })

  it('负净额行（退款大于支出）如实渲染，口径归后端', () => {
    const bars = merchantBars([
      { merchant_id: 'm-9', merchant_name: '退款户', amount_cents: -200 },
    ])
    expect(bars).toEqual([
      { name: '退款户', value: -200, color: merchantRankColor(0, 1) },
    ])
  })
})
