// 主题包统一入口（barrel）：语义色、Design Tokens、图表样式、主题合同与
// vanilla-extract 主题的集中转出口。跨包一律整引包名 `@ledger/theme`
//（深导入由 check-frontend-structure.ts 守门）；包内测试不经本入口——
// app-theme.css.ts / theme-contract.ts 须在测试装好 vanilla-extract 捕获
// adapter 之后动态导入（见 packages/theme/src/__tests__/theme-contract.test.ts），
// 本入口的静态求值链不保证该时序。

// Appearance 模式类型（issue #1154 下移，stores/app 改从包导入）
export type { Theme } from './appearance'

// 交易类型语义色与盈亏涨跌色（issue #435 / #920 单一来源）
export type { SemanticColor } from './semantic-colors'
export {
  KIND_SEMANTIC_COLORS,
  SEMANTIC_COLOR_KINDS,
  kindSemanticColor,
  PNL_COLORS,
  pnlSemanticColor,
} from './semantic-colors'

// 中性 Design Tokens 单一来源（issue #887 / ADR-0093）
export type { FloatLayerTokens, NeutralTokens } from './design-tokens'
export { DARK_FLOAT_LAYER, NEUTRAL_TOKENS } from './design-tokens'

// 组件库主题覆盖（从 token 派生）
export { darkOverrides, lightOverrides } from './overrides'

// 新方案主题（vanilla-extract 主题合同与合同变量，issue #888 / ADR-0093）
export { appVars, darkThemeClass, lightThemeClass } from './app-theme.css.ts'

// Sprinkles 原子化层（构建期组合，ADR-0093 纪律）
export { sprinkles } from './sprinkles.css.ts'

// 主题合同（模式 → 根主题类 + 组件库覆盖，App.vue 唯一接线点）
export type { AppThemeContract } from './theme-contract'
export { bindRootThemeClass, resolveAppTheme } from './theme-contract'

// 柔和柱状图统一样式与柱尾金额标注插件
export type { BarEndAmountsPluginOptions } from './chart-style'
export {
  SOFT_BAR_RADIUS,
  SOFT_BAR_PERCENTAGE,
  SOFT_CATEGORY_PERCENTAGE,
  SOFT_TOOLTIP,
  SOFT_LEGEND_LABELS,
  softChartColors,
  barEndAmountPlugin,
  softBarFillPlugin,
} from './chart-style'
