import type { Component } from 'vue'

/**
 * vue-chartjs `<Line>` 的测试桩：组件层测试只验证数据联动与文案渲染，
 * 不验证 canvas 绘制（jsdom 无 2D context）。用法：
 * `vi.mock('vue-chartjs', () => ({ Line: LineChartStub }))`。
 * 桩把收到的 `data` prop 序列化渲染到 DOM，供断言图表输入。
 */
export const LineChartStub: Component = {
  name: 'Line',
  props: ['data', 'options'],
  template: '<div data-testid="line-chart">{{ JSON.stringify(data) }}</div>',
}

/**
 * vue-chartjs `<Bar>` 的测试桩：与 Line 桩同约定（issue #160 订阅花费趋势图）。
 * `vi.mock('vue-chartjs', async () => ({ Bar: BarChartStub }))`。
 */
export const BarChartStub: Component = {
  name: 'Bar',
  props: ['data', 'options'],
  template: '<div data-testid="bar-chart">{{ JSON.stringify(data) }}</div>',
}
