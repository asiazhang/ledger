import { globalStyle } from '@vanilla-extract/css'
import { appVars } from '@/theme/app-theme.css.ts'

/**
 * 移动档交易卡片列表旁路样式（issue #846 / ADR-0088 决策 9，ADR-0093 样式方案）：
 * 与组件同目录共置。组件只在移动档渲染（<840 断点双渲染），桌面档 DOM 不存在、
 * 规则天然零渗透；类名为稳定钩子类（测试与样式同键），不覆盖任何 naive 内部类。
 *
 * 卡片字段按票⑥范围：日期、类型、分类/商户、账户、金额（收支语义色），来源行
 * 保留链接语义；中性视觉取值一律经主题合同变量（appVars），亮暗随根主题类换装。
 */

/** 列表容器钩子类。 */
export const TRANSACTION_CARD_LIST_CLASS = 'transaction-card-list'

/** 卡片根元素钩子类。 */
export const TRANSACTION_CARD_CLASS = 'transaction-card'

/** 卡片「⋯」按钮钩子类（与桌面表格操作列共用 row-actions-btn/touch-hit-area 全局类）。 */
export const CARD_MENU_CLASS = 'transaction-card-menu'

/** 列表容器：卡片纵向堆叠。 */
globalStyle(`.${TRANSACTION_CARD_LIST_CLASS}`, {
  display: 'flex',
  flexDirection: 'column',
  gap: '8px',
})

/** 卡片根：卡片底色 + 中性边框 + 圆角，内部 flex 列布局。 */
globalStyle(`.${TRANSACTION_CARD_CLASS}`, {
  display: 'flex',
  flexDirection: 'column',
  gap: '6px',
  padding: '12px',
  borderRadius: appVars.radius.base,
  background: appVars.background.card,
  border: `1px solid ${appVars.border.border}`,
})

/** 头部行：日期 + 类型标签靠左、「⋯」靠右。 */
globalStyle(`.${TRANSACTION_CARD_CLASS} .transaction-card-head`, {
  display: 'flex',
  alignItems: 'center',
  gap: '8px',
})

globalStyle(`.${TRANSACTION_CARD_CLASS} .transaction-card-date`, {
  fontSize: '12px',
  color: appVars.text.secondary,
  fontVariantNumeric: 'tabular-nums',
  flex: 'none',
})

/** 「⋯」推到头部行右缘；热区外扩由全局 touch-hit-area 类承担。 */
globalStyle(`.${TRANSACTION_CARD_CLASS} .${CARD_MENU_CLASS}`, {
  marginLeft: 'auto',
})

/** 信息行（分类/商户、账户、来源）：单行省略，长内容不撑破卡片。 */
globalStyle(`.${TRANSACTION_CARD_CLASS} .transaction-card-row`, {
  display: 'flex',
  alignItems: 'center',
  gap: '6px',
  minWidth: 0,
  fontSize: '13px',
  whiteSpace: 'nowrap',
  overflow: 'hidden',
})

globalStyle(`.${TRANSACTION_CARD_CLASS} .transaction-card-row > *`, {
  flexShrink: 1,
  minWidth: 0,
})

/** 分类/商户缺省的「-」与来源行的弱化呈现。 */
globalStyle(`.${TRANSACTION_CARD_CLASS} .transaction-card-empty`, {
  color: appVars.text.tertiary,
})

/** 分类与商户之间的间隔点。 */
globalStyle(`.${TRANSACTION_CARD_CLASS} .transaction-card-join`, {
  color: appVars.text.tertiary,
  flex: 'none',
})

/** 金额行：右对齐、加大字号——扫一眼即读金额与方向（语义色由内联 style 承担）。 */
globalStyle(`.${TRANSACTION_CARD_CLASS} .transaction-card-amount`, {
  display: 'flex',
  justifyContent: 'flex-end',
  fontSize: '16px',
  fontWeight: 600,
  fontVariantNumeric: 'tabular-nums',
  paddingTop: '2px',
})
