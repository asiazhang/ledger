import { style } from '@vanilla-extract/css'
import { sprinkles } from '@/theme/sprinkles.css.ts'
import { appVars } from '@/theme/app-theme.css.ts'

/**
 * AI 提示词页旁路样式（issue #888 样式方案试点 / ADR-0093）：原 scoped 样式块
 * 迁出为组件旁的 vanilla-extract 样式文件，与组件同目录共置。
 *
 * 验证「书写 → 构建期提取 → 主题类切换」全链路：圆角与底面消费中性 token 的
 * 合同变量（构建期从 `design-tokens.ts` 派生），随根元素主题类亮暗整体换装；
 * 边框经同一合同变量补细边框线（视觉语言「细边框代替重阴影」，亮色下为卡片
 * 提供与 body 的分界）。字体/字号/行高等无中性 token 的轴以一次性字面量收在
 * 本文件，不入 sprinkles（不造第二套常量来源）。
 */
export const promptBody = style([
  sprinkles({
    borderRadius: 'small',
    background: 'card',
  }),
  {
    margin: 0,
    padding: '12px 16px',
    border: `1px solid ${appVars.border.border}`,
    fontFamily: "ui-monospace, 'SF Mono', Menlo, Consolas, monospace",
    fontSize: '13px',
    lineHeight: 1.7,
    whiteSpace: 'pre-wrap',
    wordBreak: 'break-word',
    maxHeight: '60vh',
    overflow: 'auto',
  },
])
