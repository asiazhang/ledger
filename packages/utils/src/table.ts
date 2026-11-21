import type { DataTableColumn } from "naive-ui";

export function sumFixedColumnWidths<T>(columns: DataTableColumn<T>[]): number {
  return columns.reduce((sum, col) => sum + (typeof col.width === "number" ? col.width : 0), 0);
}
