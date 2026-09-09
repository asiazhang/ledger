import { describe, it, expect } from 'vitest'
import { Chart as ChartJS } from 'chart.js'
// 被测注册缝（issue #926）：导入统一注册模块即执行全量 register（模块级副作用，
// 导入即发生）。注册表是真实 Chart.js registry，不经 vue-chartjs 桩——历史事故
// （组件自持注册子集、桩测覆盖不到）正是本测试要钉死的回归面。
import '@/utils/chart-registration'

describe('Chart.js 统一注册缝（issue #926）', () => {
  it('折线图与柱状图所需 controller / element / scale 全部已注册（防 "point" is not a registered element 回归）', () => {
    // chart.js v4 注册表按类别分表：controller/element/scale 各自的 id 空间
    // 独立（元素 id 是 'point'/'line'/'bar' 而非类名）。缺任何一项都会在图表
    // 构造/动画帧中抛 "not a registered element/controller/scale"（线上实况：
    // "point" is not a registered element → 错误循环冻结界面）。
    expect(() => ChartJS.registry.getController('line'), '缺 line controller').not.toThrow()
    expect(() => ChartJS.registry.getController('bar'), '缺 bar controller').not.toThrow()
    expect(() => ChartJS.registry.getElement('line'), '缺 line element').not.toThrow()
    expect(() => ChartJS.registry.getElement('point'), '缺 point element').not.toThrow()
    expect(() => ChartJS.registry.getElement('bar'), '缺 bar element').not.toThrow()
    expect(() => ChartJS.registry.getScale('category'), '缺 category scale').not.toThrow()
    expect(() => ChartJS.registry.getScale('linear'), '缺 linear scale').not.toThrow()
  })
})
