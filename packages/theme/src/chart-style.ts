import { Chart as ChartJS, type Chart } from "chart.js";
import type { Theme } from "./theme";
import { formatAmount } from "@ledger/money";

export const SOFT_BAR_RADIUS = 4;

export const SOFT_BAR_PERCENTAGE = 0.7;
export const SOFT_CATEGORY_PERCENTAGE = 0.85;

const BASE_ALPHA = 0.4;

export function softChartColors(theme: Theme): { grid: string; ticks: string } {
  return theme === "dark"
    ? { grid: "rgba(255, 255, 255, 0.10)", ticks: "rgba(255, 255, 255, 0.55)" }
    : { grid: "rgba(0, 0, 0, 0.08)", ticks: "rgba(0, 0, 0, 0.55)" };
}

export const SOFT_TOOLTIP = {
  backgroundColor: "rgba(0, 0, 0, 0.85)",
  cornerRadius: 8,
  padding: 10,
  boxPadding: 6,
} as const;

export const SOFT_LEGEND_LABELS = {
  usePointStyle: true,
  pointStyle: "circle",
  boxWidth: 8,
  boxHeight: 8,
  padding: 16,
} as const;

function withAlpha(hex: string, alpha: number): string {
  const n = hex.replace("#", "");
  const r = Number.parseInt(n.slice(0, 2), 16);
  const g = Number.parseInt(n.slice(2, 4), 16);
  const b = Number.parseInt(n.slice(4, 6), 16);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

export interface BarEndAmountsPluginOptions {
  labels?: string[];
}

export const barEndAmountPlugin = {
  id: "barEndAmounts",
  afterDatasetsDraw(chart: Chart<"bar">) {
    const data = chart.data.datasets[0]?.data as number[] | undefined;
    if (!data?.length) return;
    const labelOverrides = (
      chart.options.plugins as Record<string, BarEndAmountsPluginOptions | undefined> | undefined
    )?.barEndAmounts?.labels;
    const ctx = chart.ctx;
    ctx.save();
    ctx.fillStyle = typeof chart.options.color === "string" ? chart.options.color : "#666";
    const f = ChartJS.defaults.font;
    ctx.font = `${f.size ?? 12}px ${f.family ?? "sans-serif"}`;
    ctx.textBaseline = "middle";
    chart.getDatasetMeta(0).data.forEach((el, i) => {
      const value = data[i];
      const label = labelOverrides?.[i] ?? formatAmount(value);
      const { x, y } = el.getProps(["x", "y"], true);
      ctx.textAlign = value >= 0 ? "left" : "right";
      ctx.fillText(label, value >= 0 ? x + 6 : x - 6, y);
    });
    ctx.restore();
  },
};

export const softBarFillPlugin = {
  id: "softBarFill",
  beforeDatasetsDraw(chart: Chart<"bar">) {
    const horizontal = chart.options.indexAxis === "y";
    const { ctx } = chart;
    chart.data.datasets.forEach((_, datasetIndex) => {
      for (const el of chart.getDatasetMeta(datasetIndex).data) {
        const color = el.options.backgroundColor;
        if (typeof color !== "string" || color.length !== 7 || !color.startsWith("#")) continue;
        const { x, y, base } = el.getProps(["x", "y", "base"], true);
        const tip = horizontal ? x : y;
        if (!Number.isFinite(tip) || !Number.isFinite(base) || Math.abs(tip - base) < 1) continue;
        const gradient = horizontal
          ? ctx.createLinearGradient(base, 0, tip, 0)
          : ctx.createLinearGradient(0, base, 0, tip);
        gradient.addColorStop(0, withAlpha(color, BASE_ALPHA));
        gradient.addColorStop(1, color);
        el.options.backgroundColor = gradient;
      }
    });
  },
};
