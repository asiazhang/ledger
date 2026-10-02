import { useBareKeyShortcuts } from "@/composables/useBareKeyShortcuts";

/**
 * 翻页快捷键（issue #1902 / ADR-0140）：服务端分页列表裸键 `[` / `]` 步进上一页/下一页。
 * 键位闭集单一来源：keydown 匹配与分页条旁键位提示共用同一对键。
 *
 * 抑制三闸门与监听面由 useBareKeyShortcuts 内核承载（与周期步进快捷键共用同一内核，
 * Overlay Suppression ADR-0035 / 输入轴 ADR-0088 / CreateShortcut 家族纪律）。
 * 步进语义由调用方声明：以最新页码为基准走既有翻页出口、首/末页越界幂等无操作，
 * 本模块不持页码状态。
 */
export const PAGINATION_KEYS = {
  prev: "[",
  next: "]",
} as const;

export type PaginationStep = "prev" | "next";

/**
 * 纯函数：裸键命中则返回步进方向，否则 null。
 * 仅接受无修饰键裸键——任何 Ctrl/Cmd/Alt/Shift 修饰均不命中，
 * 把组合键让给系统与其他快捷键（如 Cmd+1..9 视图切换）。
 */
export function matchPaginationShortcut(e: KeyboardEvent): PaginationStep | null {
  if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return null;
  if (e.key === PAGINATION_KEYS.prev) return "prev";
  if (e.key === PAGINATION_KEYS.next) return "next";
  return null;
}

/**
 * 服务端分页列表翻页快捷键：裸键 `[` 上一页 / `]` 下一页（语义等同点击分页条，ADR-0140）。
 * 随消费视图装卸；抑制条件与监听面由 useBareKeyShortcuts 内核唯一事实源驱动，
 * 纯函数 matchPaginationShortcut 与轴无关（可独立测试）。
 */
export function usePaginationShortcuts(step: (dir: PaginationStep) => void): void {
  useBareKeyShortcuts(matchPaginationShortcut, step);
}
