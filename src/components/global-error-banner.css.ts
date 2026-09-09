import { style } from '@vanilla-extract/css'
import { sprinkles } from '@/theme/sprinkles.css.ts'

/**
 * 全局渲染错误提示条旁路样式（issue #926 / ADR-0093 样式方案）：与组件同目录
 * 共置。位置沿用 GlobalBusyBar 的窗口顶部环境指示位；错误带高度小、不遮内容，
 * 本体可点（关闭按钮），属环境指示惯例（忙碌条同位先例）。
 */
export const banner = style([
  sprinkles({
    display: 'flex',
    alignItems: 'center',
  }),
  {
    position: 'fixed',
    top: 'env(safe-area-inset-top, 0px)',
    left: 0,
    right: 0,
    zIndex: 3000,
    gap: '8px',
    padding: '4px 12px',
    fontSize: '12px',
    lineHeight: 1.4,
    background: 'var(--error-color)',
    color: '#fff',
  },
])

export const text = style({
  minWidth: 0,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
  userSelect: 'none',
})

export const closeButton = style({
  flexShrink: 0,
})
