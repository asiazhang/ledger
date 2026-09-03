import type { Chart } from 'chart.js'
import type { Theme } from '@/stores/app'

/**
 * 柔和柱状图统一样式（报表页两张图共用，2026-09 视觉柔化；投资趋势图未来可复用）：
 * - 柱端 4px 圆角：borderSkipped 默认 start，基线端不圆，柱端读数位置不糊；
 * - 柱体沿数值轴渐隐：柱端实色 → 基线 40% 透明度，由 softBarFillPlugin 在绘制期
 *   逐柱换装——data 侧 backgroundColor 仍传实色字符串（同 id 稳定配色的数据契约、
 *   图桩 JSON 序列化断言均不受影响），渐变纯绘制期呈现；
 * - 网格/刻度主题感知弱化 + tooltip/图例柔和预设，中性色按外观取值。
 */

/** 柱端圆角（px） */
export const SOFT_BAR_RADIUS = 4

/** 月度图柱宽收窄：柱体占类目宽 70%×85%，柱间留呼吸感 */
export const SOFT_BAR_PERCENTAGE = 0.7
export const SOFT_CATEGORY_PERCENTAGE = 0.85

/** 基线端透明度（柱端实色 → 基线淡出） */
const BASE_ALPHA = 0.4

/** 主题感知中性色：网格线与刻度文字（暗色白基、亮色黑基，同灰阶） */
export function softChartColors(theme: Theme): { grid: string; ticks: string } {
  return theme === 'dark'
    ? { grid: 'rgba(255, 255, 255, 0.10)', ticks: 'rgba(255, 255, 255, 0.55)' }
    : { grid: 'rgba(0, 0, 0, 0.08)', ticks: 'rgba(0, 0, 0, 0.55)' }
}

/** tooltip 柔和预设：深色气泡两主题一致，圆角 + 加大内边距去工程感 */
export const SOFT_TOOLTIP = {
  backgroundColor: 'rgba(0, 0, 0, 0.85)',
  cornerRadius: 8,
  padding: 10,
  boxPadding: 6,
} as const

/** 图例小圆点预设：弱化图例存在感 */
export const SOFT_LEGEND_LABELS = {
  usePointStyle: true,
  pointStyle: 'circle',
  boxWidth: 8,
  boxHeight: 8,
  padding: 16,
} as const

function withAlpha(hex: string, alpha: number): string {
  const n = hex.replace('#', '')
  const r = Number.parseInt(n.slice(0, 2), 16)
  const g = Number.parseInt(n.slice(2, 4), 16)
  const b = Number.parseInt(n.slice(4, 6), 16)
  return `rgba(${r}, ${g}, ${b}, ${alpha})`
}

/**
 * 柱体渐隐插件（挂进 Bar 的 plugins 即生效，与柱尾标签插件同先例）：
 * beforeDatasetsDraw 阶段元素几何已定，逐柱把已解析的实色 backgroundColor
 * 换成「基线 BASE_ALPHA → 柱端实色」的 canvas 渐变；方向随 indexAxis 自动
 * 适配（横向图数值端实色、基线淡出，负值柱天然正确）。退化柱（高/宽近 0）
 * 与非 6 位 hex 实色原样跳过，保持实色。
 */
export const softBarFillPlugin = {
  id: 'softBarFill',
  beforeDatasetsDraw(chart: Chart<'bar'>) {
    const horizontal = chart.options.indexAxis === 'y'
    const { ctx } = chart
    chart.data.datasets.forEach((_, datasetIndex) => {
      for (const el of chart.getDatasetMeta(datasetIndex).data) {
        const color = el.options.backgroundColor
        if (typeof color !== 'string' || color.length !== 7 || !color.startsWith('#')) continue
        const { x, y, base } = el.getProps(['x', 'y', 'base'], true)
        const tip = horizontal ? x : y
        if (!Number.isFinite(tip) || !Number.isFinite(base) || Math.abs(tip - base) < 1) continue
        const gradient = horizontal
          ? ctx.createLinearGradient(base, 0, tip, 0)
          : ctx.createLinearGradient(0, base, 0, tip)
        gradient.addColorStop(0, withAlpha(color, BASE_ALPHA))
        gradient.addColorStop(1, color)
        el.options.backgroundColor = gradient
      }
    })
  },
}
