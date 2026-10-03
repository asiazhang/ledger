import { useBareKeyShortcuts } from "@/composables/useBareKeyShortcuts";

/**
 * 周期步进快捷键（issue #1904 / ADR-0141）：裸键 `,` / `.` 步进上一个/下一个自然周期，
 * 语义等同点击 QuickTimeRange 的期间步进器 `<` / `>`。
 * 键位闭集单一来源：keydown 匹配与键位提示共用同一对键。
 *
 * 键位论证：`,` `.` 恰是步进按钮图标 `<` `>` 的同一物理键位的无 Shift 形态，
 * 方向空间映射直觉直接；裸键仓库零占用（与 ViewShortcut 的 ⌘, 设置键同键不同修饰，
 * 裸键匹配本就拒绝一切修饰键）；组合键让给系统与视图快捷键（ADR-0140 家族纪律）。
 *
 * 抑制闸门与监听面复用 useBareKeyShortcuts 内核（Overlay Suppression ADR-0035 /
 * 输入轴 ADR-0088）；纯函数 matchPeriodStepShortcut 与轴无关（可独立测试）。
 */
export const PERIOD_STEP_KEYS = {
  prev: ",",
  next: ".",
} as const;

export type PeriodStep = "prev" | "next";

/**
 * 纯函数：裸键命中则返回步进方向，否则 null。
 * 仅接受无修饰键裸键——任何 Ctrl/Cmd/Alt/Shift 修饰均不命中。
 */
export function matchPeriodStepShortcut(e: KeyboardEvent): PeriodStep | null {
  if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return null;
  if (e.key === PERIOD_STEP_KEYS.prev) return "prev";
  if (e.key === PERIOD_STEP_KEYS.next) return "next";
  return null;
}

/**
 * 周期步进快捷键：裸键 `,` 上一个周期 / `.` 下一个周期（语义等同点击期间步进器）。
 * 步进语义由调用方声明：越界（「全部」无游标、数据期间边界末端）幂等无操作，
 * 本模块不持游标、不派生边界。
 */
export function useTimePeriodShortcuts(step: (dir: PeriodStep) => void): void {
  useBareKeyShortcuts(matchPeriodStepShortcut, step);
}
