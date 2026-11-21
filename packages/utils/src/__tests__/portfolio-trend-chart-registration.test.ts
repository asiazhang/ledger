import { describe, it, expect } from "vitest";
import { Chart as ChartJS } from "chart.js";
import "../chart-registration";

describe("Chart.js 统一注册缝（issue #926）", () => {
  it('折线图与柱状图所需 controller / element / scale 全部已注册（防 "point" is not a registered element 回归）', () => {
    expect(() => ChartJS.registry.getController("line"), "缺 line controller").not.toThrow();
    expect(() => ChartJS.registry.getController("bar"), "缺 bar controller").not.toThrow();
    expect(() => ChartJS.registry.getElement("line"), "缺 line element").not.toThrow();
    expect(() => ChartJS.registry.getElement("point"), "缺 point element").not.toThrow();
    expect(() => ChartJS.registry.getElement("bar"), "缺 bar element").not.toThrow();
    expect(() => ChartJS.registry.getScale("category"), "缺 category scale").not.toThrow();
    expect(() => ChartJS.registry.getScale("linear"), "缺 linear scale").not.toThrow();
  });
});
