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

/**
 * `<Bar>` 桩的带 options 形态（issue #378）：data 与 options 分 span 序列化，
 * 供断言图数据形态与 options（如横向 indexAxis）。两 span 分开，保证各目
 * text() 仍是完整 JSON（与只渲染 data 的 bar-chart 桩互不干扰）。
 */
export const BarChartStubWithOptions: Component = {
  name: 'Bar',
  props: ['data', 'options', 'plugins'],
  template:
    '<div><span data-testid="bar-data">{{ JSON.stringify(data) }}</span>' +
    '<span data-testid="bar-options">{{ JSON.stringify(options) }}</span></div>',
}
