import { describe, it, expect } from 'vitest'
import { merchantTableRows } from '@/utils/merchant-chart'
import { paletteColor } from '@/utils/category-chart'
import type { MerchantShare } from '@/types'

// 商户消费排行表格行构建纯函数（issue #618 表格化）：排序与 topN 截断已收口后端
// `merchant_shares`，前端按返回序渲染、零口径逻辑。行色与支出分类构成同源——
// 分类色板按名次序取色（多颜色）；内嵌条比例分母 = 显示区最大正金额（TopN 下即
// 第一名），负净额不画条（比例归 0）但金额与占比照实；占比分母 = 后端载荷全量
// 合计（与 topN 截断无关，沿用既有 tooltip 同一取整口径）；笔数从 MerchantShare
// 透传；merchant_id 透传为下钻跳转载荷用（issue #589）。

describe('merchantTableRows 表格行构建（issue #618）', () => {
  const rows: MerchantShare[] = [
    { merchant_id: 'm-1', merchant_name: '超市', amount_cents: 5000, transaction_count: 3 },
    { merchant_id: 'm-2', merchant_name: '咖啡', amount_cents: 2500, transaction_count: 2 },
    { merchant_id: 'm-3', merchant_name: '书店', amount_cents: 0, transaction_count: 1 },
  ]

  it('后端返回序即行序：名称、金额、笔数逐行透传，零口径逻辑（不重排不过滤）', () => {
    const table = merchantTableRows(rows, 15000)
    expect(table.map((r) => r.name)).toEqual(['超市', '咖啡', '书店'])
    expect(table.map((r) => r.amount_cents)).toEqual([5000, 2500, 0])
    expect(table.map((r) => r.transactionCount)).toEqual([3, 2, 1])
  })

  it('merchant_id 同源透传（下钻跳转载荷用，issue #589）', () => {
    const table = merchantTableRows(rows, 15000)
    expect(table.map((r) => r.merchant_id)).toEqual(['m-1', 'm-2', 'm-3'])
  })

  it('行色与分类构成同源：色板按名次序取色（多颜色，第 1 名 = 色板首位深蓝）', () => {
    const table = merchantTableRows(rows, 15000)
    expect(table.map((r) => r.color)).toEqual([
      paletteColor(0),
      paletteColor(1),
      paletteColor(2),
    ])
    // 多颜色：相邻名次颜色互不相同
    expect(table[0].color).not.toBe(table[1].color)
    expect(table[1].color).not.toBe(table[2].color)
  })

  it('内嵌条 = 金额 ÷ 显示区最大正金额（百分数，两位小数内）：最大行 = 100、其余按比例、零金额行 = 0', () => {
    const table = merchantTableRows(rows, 15000)
    expect(table.map((r) => r.barPct)).toEqual([100, 50, 0])
  })

  it('非整除比例收敛到两位小数内：9900/50000 → barPct 19.8', () => {
    const table = merchantTableRows([
      { merchant_id: 'm-a', merchant_name: '甲', amount_cents: 50000, transaction_count: 1 },
      { merchant_id: 'm-b', merchant_name: '乙', amount_cents: 9900, transaction_count: 1 },
    ], 150000)
    expect(table[0].barPct).toBe(100)
    expect(table[1].barPct).toBe(19.8)
  })

  it('占比% 分母 = 载荷全量合计（与 topN 截断无关），取整口径与既有 tooltip 同源', () => {
    // 全量 15000 刻意 ≠ 展示行合计 7500：咖啡 2500/15000 = 17%（误用行合计会得 33%）
    const table = merchantTableRows(rows, 15000)
    expect(table.map((r) => r.sharePct)).toEqual([33, 17, 0])
  })

  it('负净额行（退款大于支出）：不画条（条宽归 0），金额与占比照实显示', () => {
    const table = merchantTableRows([
      { merchant_id: 'm-9', merchant_name: '退款户', amount_cents: -200, transaction_count: 1 },
    ], 15000)
    expect(table[0].amount_cents).toBe(-200)
    expect(table[0].barPct).toBe(0)
    expect(table[0].sharePct).toBe(-1) // round(-200/15000*100) = round(-1.33) = -1
  })

  it('负净额行不影响其他行的条长分母：分母 = 最大正金额，非绝对值最大', () => {
    const table = merchantTableRows([
      { merchant_id: 'm-1', merchant_name: '超市', amount_cents: 5000, transaction_count: 2 },
      { merchant_id: 'm-9', merchant_name: '退款户', amount_cents: -8000, transaction_count: 1 },
    ], 15000)
    expect(table.map((r) => r.barPct)).toEqual([100, 0])
  })

  it('全零 / 全负行集：最大正金额缺位时条长全体归 0（除零防护），金额占比照实', () => {
    const table = merchantTableRows([
      { merchant_id: 'm-4', merchant_name: '甲', amount_cents: -1000, transaction_count: 1 },
      { merchant_id: 'm-5', merchant_name: '乙', amount_cents: 0, transaction_count: 2 },
    ], 15000)
    expect(table.map((r) => r.barPct)).toEqual([0, 0])
    expect(table.map((r) => r.amount_cents)).toEqual([-1000, 0])
  })

  it('全量合计为 0：占比全体归 0（分母缺位防护），与 tooltip 分母缺位同语义', () => {
    const table = merchantTableRows(rows, 0)
    expect(table.map((r) => r.sharePct)).toEqual([0, 0, 0])
  })
})
