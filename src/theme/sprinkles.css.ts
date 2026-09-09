import { createSprinkles, defineProperties } from '@vanilla-extract/sprinkles'
import { appVars } from './app-theme.css.ts'

/**
 * Sprinkles 原子化层（issue #888 / ADR-0093）：theme 驱动的原子类工具面。
 *
 * 约定：
 * - **原子类组合只在构建期发生**（ADR-0093 纪律）：组件在 `.css.ts` 旁路样式
 *   文件中以字面量键组合 sprinkles，运行时不动态拼装——动态拼装会把全量原子
 *   类样式带进产物，属被禁止的越线行为。
 * - **theme 轴消费合同变量**（borderRadius/background/borderColor/color）：
 *   原子类不烘焙取值，随根元素主题类整体换装；中性 token 尚未覆盖的轴
 *   （间距、字号等）不入 sprinkles，待 token 化后再扩，避免第二套常量来源。
 * - **语义色不入合同**（issue #888 / Design Tokens 词条边界）：业务语义色
 *   （六种交易类型）维持 `semantic-colors.ts` 既有接缝，新方案需要时直接消费
 *   该接缝，不并入合同变量、不在此重复定义。
 * - 布尔轴（display/对齐族）是无条件原子，值集按需增补，不预设全量。
 */
const properties = defineProperties({
  properties: {
    display: ['block', 'inline-block', 'flex', 'inline-flex', 'none'],
    flexDirection: ['row', 'column'],
    alignItems: ['stretch', 'flex-start', 'center', 'flex-end', 'baseline'],
    justifyContent: ['flex-start', 'center', 'flex-end', 'space-between', 'space-around'],
    borderRadius: {
      small: appVars.radius.small,
      base: appVars.radius.base,
      large: appVars.radius.large,
    },
    background: {
      body: appVars.background.body,
      card: appVars.background.card,
      popover: appVars.background.popover,
      modal: appVars.background.modal,
    },
    borderColor: {
      border: appVars.border.border,
      divider: appVars.border.divider,
    },
    color: {
      textPrimary: appVars.text.primary,
      textSecondary: appVars.text.secondary,
      textTertiary: appVars.text.tertiary,
    },
  },
})

export const sprinkles = createSprinkles(properties)
