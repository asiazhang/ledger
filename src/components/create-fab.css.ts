import { globalStyle } from '@vanilla-extract/css'
import { appVars } from '@/theme/app-theme.css.ts'

/**
 * 记一笔悬浮按钮旁路样式（issue #846 / ADR-0088 决策 5，ADR-0093 样式方案）：
 * 与组件同目录共置。组件只在移动档交易页渲染（桌面档 DOM 不存在，零渗透）。
 *
 * 覆盖 naive 按钮内部类的规则提一级特异性（钩子类 + .n-button 并列，先例
 * app-modal.css.ts），不依赖样式注入顺序。触控目标 ≥48px 为全局验收基线
 * （ADR-0088 决策 11）：FAB 本体 56px（Material FAB 惯例），类型选项 minHeight 48px。
 */

/** FAB 触发按钮钩子类：右下常驻，避开手势条安全区。 */
export const CREATE_FAB_CLASS = 'create-fab'

globalStyle(`.${CREATE_FAB_CLASS}.n-button`, {
  position: 'fixed',
  right: '16px',
  bottom: 'calc(16px + env(safe-area-inset-bottom))',
  width: '56px',
  height: '56px',
  fontSize: '28px',
  borderRadius: '50%',
})

/** 类型选择轻弹层内容容器：五枚大号选项纵向排布。 */
export const CREATE_FAB_SHEET_CLASS = 'create-fab-sheet'

globalStyle(`.${CREATE_FAB_SHEET_CLASS}`, {
  display: 'flex',
  flexDirection: 'column',
  gap: '8px',
  minWidth: '180px',
})

/** 类型选项钩子类：大号触控目标（≥48px），随主题合同换装。 */
export const CREATE_FAB_OPTION_CLASS = 'create-fab-option'

globalStyle(`.${CREATE_FAB_OPTION_CLASS}.n-button`, {
  width: '100%',
  minHeight: '48px',
  justifyContent: 'center',
  background: appVars.background.card,
})
