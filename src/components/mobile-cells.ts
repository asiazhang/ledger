export const MOBILE_CELL_STYLE = "display: flex; flex-direction: column; gap: 2px; min-width: 0;";

export const MOBILE_SUB_STYLE = "font-size: 12px; opacity: 0.65;";

export const MOBILE_TOUCH_TARGET_STYLE = { minWidth: "48px", minHeight: "48px" };

export function mobileSubLine(...parts: Array<string | null | undefined>): string {
  return parts.filter((p): p is string => p !== null && p !== undefined && p !== "").join(" · ");
}
