import type { GlobalThemeOverrides } from 'naive-ui'

/**
 * 主题定制（Design Tokens）——Raycast 精致工具感：近黑底、微分层、细边框、
 * 克制圆角（8/12px）、琥珀暖橙强调色。经 App.vue 的 NConfigProvider `theme-overrides` 接入。
 *
 * 约定：
 * - 强调色（品牌色）与语义色（收入绿/支出红/退款蓝）相互独立：语义色不在这里，
 *   硬编码于 `src/components/transaction-columns.ts` 与报表图表，不随主题变化。
 * - 暗色主题的按钮文字色由 Naive 自动取深色（baseColor=#000），亮琥珀直接可用；
 *   亮色主题按钮文字为白色，亮琥珀对比度不足（约 2:1），故亮色用同色相加深版
 *   （amber-700，白字约 4.4:1）保证可读性。
 * - 改动后跑 `npx vue-tsc --noEmit`，`GlobalThemeOverrides` 会校验变量名。
 */

// 暗色（默认主题，主战场）
export const darkOverrides: GlobalThemeOverrides = {
  common: {
    // 强调色：琥珀暖橙
    primaryColor: '#F59E0B',
    primaryColorHover: '#FBBF24',
    primaryColorPressed: '#D97706',
    primaryColorSuppl: '#F59E0B',
    // 圆角阶梯：基础 8（组件级：卡片/弹窗 12）
    borderRadius: '8px',
    borderRadiusSmall: '6px',
    // 背景分层：近黑 body → 略浅卡片（侧边栏随 cardColor）→ 弹窗
    bodyColor: '#0E0E10',
    cardColor: '#161618',
    popoverColor: '#1C1C1E',
    modalColor: '#1C1C1E',
    // 细边框代替重阴影
    borderColor: 'rgba(255, 255, 255, 0.08)',
    dividerColor: 'rgba(255, 255, 255, 0.06)',
    // 文本层级（克制的中灰）
    textColor1: '#ECECEC',
    textColor2: '#A0A0A0',
    textColor3: '#6E6E6E',
  },
  Dropdown: {
    // 浮层菜单（行右键菜单、下拉菜单）：表格/卡片底色（#161618）与 popoverColor
    // （#1C1C1E）几乎同阶，菜单浮在行上会淹没。Raycast 式分层「浮层比底层更亮」：
    // 背景直接抬到 #343438，并由 peers.Popover 把阴影换成「细边框 ring + 柔和投影」，
    // 仅作用于 Dropdown 自身，不动全局 popoverColor / boxShadow2。
    color: '#343438',
    peers: {
      Popover: {
        boxShadow:
          '0 0 0 1px rgba(255, 255, 255, 0.12), 0 8px 24px rgba(0, 0, 0, 0.5)',
      },
    },
  },
  Card: {
    borderRadius: '12px',
  },
  Button: {
    borderRadius: '8px',
  },
  Input: {
    borderRadius: '8px',
  },
  Select: {
    borderRadius: '8px',
  },
  DatePicker: {
    borderRadius: '8px',
  },
  Menu: {
    borderRadius: '6px',
    itemColorActive: 'rgba(255, 255, 255, 0.06)',
    itemColorActiveHover: 'rgba(255, 255, 255, 0.08)',
    itemTextColorActive: '#F59E0B',
    itemTextColorActiveHover: '#F59E0B',
    itemIconColorActive: '#F59E0B',
    itemIconColorActiveHover: '#F59E0B',
  },
}

// 亮色（次要主题）：仅共享强调色（同色相加深版），其余保持 Naive 出厂默认（能用即可）
export const lightOverrides: GlobalThemeOverrides = {
  common: {
    primaryColor: '#B45309',
    primaryColorHover: '#92400E',
    primaryColorPressed: '#78350F',
    primaryColorSuppl: '#B45309',
  },
}
