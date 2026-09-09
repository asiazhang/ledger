import { style } from '@vanilla-extract/css'
import { sprinkles } from '@/theme/sprinkles.css.ts'

/**
 * 同步进度条旁路样式（issue #897 / ADR-0095，ADR-0093 样式方案）：scoped 样式
 * 块迁出为组件旁的 vanilla-extract 样式文件，与组件同目录共置。
 *
 * 蓝色是同步进度专属色（品牌强调色为琥珀，该蓝亮暗主题同色相可读），中性
 * token 与合同变量不覆盖这一语义色，按先例以一次性字面量收在本文件、不入
 * sprinkles（不造第二套常量来源）；胶囊圆角与灰色轨道面同为该组件的形态
 * 字面量。减弱动态偏好（prefers-reduced-motion）下宽度变化即时生效，不做
 * 过渡动画。
 */
export const root = style([
  sprinkles({
    display: 'flex',
    alignItems: 'center',
  }),
  {
    gap: '8px',
    minWidth: 0,
    pointerEvents: 'none',
  },
])

export const track = style({
  flex: 1,
  height: '2px',
  borderRadius: '999px',
  background: 'rgba(127, 127, 127, 0.2)',
  overflow: 'hidden',
})

export const bar = style({
  height: '100%',
  borderRadius: '999px',
  background: '#2080f0',
  transition: 'width 0.2s ease',
  '@media': {
    '(prefers-reduced-motion: reduce)': {
      transition: 'none',
    },
  },
})

export const text = style({
  fontSize: '12px',
  lineHeight: 1.4,
  opacity: 0.75,
  whiteSpace: 'nowrap',
})
