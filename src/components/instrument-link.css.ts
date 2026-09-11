import { globalStyle } from '@vanilla-extract/css'

/**
 * 标的代码链接旁路样式（ADR-0107 / ADR-0093 样式方案）：与组件同目录共置。
 * 视觉与交互对齐 AccountLink / MerchantLink 的 scoped 样式先例（存量文件保留，
 * 新组件按守门走 vanilla-extract），钩子类 + globalStyle 不依赖样式注入顺序。
 * 主题强调色（base/hover）由组件内联注入（--accent-hover 自定义属性）。
 */

/** 链接态钩子类：文本按钮，继承单元格字号。 */
export const INSTRUMENT_LINK_CLASS = 'instrument-link'

globalStyle(`.${INSTRUMENT_LINK_CLASS}`, {
  border: 'none',
  padding: 0,
  background: 'none',
  font: 'inherit',
  cursor: 'pointer',
  borderRadius: '4px',
  maxWidth: '100%',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
})

globalStyle(`.${INSTRUMENT_LINK_CLASS}:hover`, {
  color: 'var(--accent-hover)',
  background: 'rgba(255, 255, 255, 0.06)',
  textDecoration: 'underline',
})

globalStyle(`.${INSTRUMENT_LINK_CLASS}:focus-visible`, {
  outline: '2px solid var(--accent-hover)',
  outlineOffset: '2px',
})

/** 占位钩子类（无代码标的的纯文本「-」）：无强调色、不可点击。 */
export const INSTRUMENT_PLACEHOLDER_CLASS = 'instrument-placeholder'

globalStyle(`.${INSTRUMENT_PLACEHOLDER_CLASS}`, {
  color: 'inherit',
  maxWidth: '100%',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
})
