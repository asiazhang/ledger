import type { GlobalThemeOverrides } from 'naive-ui'
import { DARK_FLOAT_LAYER, NEUTRAL_TOKENS } from './design-tokens'

/**
 * 主题定制（组件库主题覆盖）——Raycast 精致工具感：近黑底、微分层、细边框、
 * 克制圆角（8/12px）、琥珀暖橙强调色。经 App.vue 的 NConfigProvider `theme-overrides` 接入。
 *
 * 约定：
 * - **中性常量从 token 派生，本模块不持独立副本**（issue #887）：圆角阶梯、
 *   背景分层、边框、文字灰阶的唯一来源是同层 `src/theme/design-tokens.ts`，
 *   一致性由派生保证而非人工同步；派生一致性由 Vitest 守门
 *   （`src/__tests__/design-tokens.test.ts`）。
 * - 强调色（品牌色）与语义色（业务色）相互独立，且均不是中性常量：强调色留在
 *   本模块；语义色不在这里，收口于同层单一来源模块 `src/theme/semantic-colors.ts`
 *   （六种交易类型、亮/暗两套色值），交易列表/搜索金额列与报表月度收支图同源
 *   消费、随主题切换即时换色（issue #435）。
 * - 亮色主题保持现状等效：仅共享强调色（同色相加深版），其余保持 Naive 出厂
 *   默认，不借机补全（issue #887 边界）。
 * - 暗色主题的按钮文字色由 Naive 自动取深色（baseColor=#000），亮琥珀直接可用；
 *   亮色主题按钮文字为白色，亮琥珀对比度不足（约 2:1），故亮色用同色相加深版
 *   （amber-700，白字约 4.4:1）保证可读性。
 * - 改动后跑 `pnpm exec vue-tsc --noEmit`，`GlobalThemeOverrides` 会校验变量名。
 */

// 暗色（默认主题，主战场）——中性常量取自 token
const token = NEUTRAL_TOKENS.dark
const floatLayer = DARK_FLOAT_LAYER

export const darkOverrides: GlobalThemeOverrides = {
  common: {
    // 强调色：琥珀暖橙
    primaryColor: '#F59E0B',
    primaryColorHover: '#FBBF24',
    primaryColorPressed: '#D97706',
    primaryColorSuppl: '#F59E0B',
    // 圆角阶梯：基础 8（组件级：卡片 12、菜单 6）。按钮/输入/下拉等基础组件
    // 的圆角经 common.borderRadius 传播，无需组件级重复声明（naive 的 Button
    // 主题变量是按尺寸的 borderRadius*，Select/DatePicker 自身无 borderRadius
    // 变量——旧覆盖里这三个组件级 borderRadius 是被 naive 忽略的无效键，随本
    // 次派生化清理，视觉零变化，issue #887）。
    borderRadius: token.radius.base,
    borderRadiusSmall: token.radius.small,
    // 背景分层：近黑 body → 略浅卡片（侧边栏随 cardColor）→ 弹窗
    bodyColor: token.background.body,
    cardColor: token.background.card,
    popoverColor: token.background.popover,
    modalColor: token.background.modal,
    // 细边框代替重阴影
    borderColor: token.border.border,
    dividerColor: token.border.divider,
    // 文本层级（克制的中灰）
    textColor1: token.text.primary,
    textColor2: token.text.secondary,
    textColor3: token.text.tertiary,
  },
  Dropdown: {
    // 浮层菜单（行右键菜单、下拉菜单）：表格/卡片底色与 popoverColor 几乎同阶，
    // 菜单浮在行上会淹没。Raycast 式分层「浮层比底层更亮」：背景抬到比卡片更亮
    // 一阶，并由 peers.Popover 把阴影换成「细边框 ring + 柔和投影」，仅作用于
    // Dropdown 自身，不动全局 popoverColor / boxShadow2（取值见 token 暗色浮层语言）。
    color: floatLayer.color,
    peers: {
      Popover: {
        boxShadow: floatLayer.ringShadow,
      },
    },
  },
  Card: {
    borderRadius: token.radius.large,
  },
  Input: {
    borderRadius: token.radius.base,
  },
  Menu: {
    borderRadius: token.radius.small,
    itemColorActive: floatLayer.activeOverlay,
    itemColorActiveHover: floatLayer.activeHoverOverlay,
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
