// 表格通用工具：列宽与横向滚动下限的单一收口

import type { DataTableColumn } from 'naive-ui'

/** 固定列宽总和（不设 `width` 的弹性列不计入）。
 * 作为 `scroll-x` 的窄窗口横向滚动下限：窄窗口时弹性列先收缩到不动，
 * 固定列宽总和即表格铺满内容所需的最小宽度。 */
export function sumFixedColumnWidths<T>(columns: DataTableColumn<T>[]): number {
  return columns.reduce(
    (sum, col) => sum + (typeof col.width === 'number' ? col.width : 0),
    0,
  )
}
