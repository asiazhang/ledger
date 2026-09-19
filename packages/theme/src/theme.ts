/**
 * 应用外观模式（Theme）——亮/暗二值闭集的单一来源（issue #1154 / ADR-0093）。
 *
 * 原定义住在应用壳 `src/stores/app.ts`（Appearance 设备偏好的状态源），使 theme
 * 包对它持有 4 条 `import type { Theme }` 边（design-tokens / chart-style /
 * semantic-colors / theme-contract 各一条）。类型随包下移后这层依赖归零：theme
 * 包不依赖 stores / components / views / composables，方向反转为应用壳从包导入。
 */
export type Theme = "dark" | "light";
