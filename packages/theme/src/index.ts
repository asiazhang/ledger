/**
 * 主题包类型入口（`@ledger/theme`，issue #1154 / spec #1148）：只出 `Theme`
 * 类型，运行面按模块走子路径导入——不开运行期 barrel，避免把 naive-ui（主题
 * 覆盖）与 vanilla-extract（原子样式层）的重模块面经单一入口扩散给全部消费文件。
 */
export type { Theme } from './theme'
