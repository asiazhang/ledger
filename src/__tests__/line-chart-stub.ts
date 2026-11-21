import type { Component } from "vue";

export const LineChartStub: Component = {
  name: "Line",
  props: ["data", "options"],
  template: '<div data-testid="line-chart">{{ JSON.stringify(data) }}</div>',
};

export const BarChartStub: Component = {
  name: "Bar",
  props: ["data", "options"],
  template: '<div data-testid="bar-chart">{{ JSON.stringify(data) }}</div>',
};

export const BarChartStubWithOptions: Component = {
  name: "Bar",
  props: ["data", "options", "plugins"],
  template:
    '<div><span data-testid="bar-data">{{ JSON.stringify(data) }}</span>' +
    '<span data-testid="bar-options">{{ JSON.stringify(options) }}</span>' +
    '<button v-for="(_, i) in data.labels" :key="i" data-testid="bar-click"' +
    ' @click="options.onClick && options.onClick(null, [{ index: i }])">bar-{{ i }}</button></div>',
};
