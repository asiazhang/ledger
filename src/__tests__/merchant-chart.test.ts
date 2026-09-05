import { describe, it, expect } from 'vitest'
import { merchantBars } from '@/utils/merchant-chart'
import { paletteColor } from '@/utils/category-chart'
import type { MerchantShare } from '@/types'

// 商户消费排行柱图数据形态纯函数（issue #588）：按后端返回序渲染零口径逻辑；
// 柱色与支出分类构成同源——分类色板按名次序取色（多颜色），hex 实色由
// softBarFillPlugin 绘制期呈现「基线淡出 → 柱端实色」渐变。

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

  it('柱色与分类构成同源：色板按名次序取色（多颜色，第 1 名 = 色板首位深蓝）', () => {
    const bars = merchantBars(rows)
    expect(bars.map((b) => b.color)).toEqual([
      paletteColor(0),
      paletteColor(1),
      paletteColor(2),
    ])
    // 多颜色：相邻名次颜色互不相同
    expect(bars[0].color).not.toBe(bars[1].color)
    expect(bars[1].color).not.toBe(bars[2].color)
    // hex 实色：满足 softBarFillPlugin 渐变换装前提（非 hex 会被插件跳过、丢失渐变）
    for (const color of bars.map((b) => b.color)) {
      expect(color).toMatch(/^#[0-9a-f]{6}$/)
    }
  })

  it('负净额行（退款大于支出）如实渲染，口径归后端', () => {
    const bars = merchantBars([
      { merchant_id: 'm-9', merchant_name: '退款户', amount_cents: -200 },
    ])
    expect(bars).toEqual([{ name: '退款户', value: -200, color: paletteColor(0) }])
  })
})
