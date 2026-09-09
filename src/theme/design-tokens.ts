import { lightTheme } from 'naive-ui'
import type { Theme } from '@/stores/app'

/**
 * 中性 Design Tokens 单一来源（issue #887 / ADR-0093）——圆角阶梯、背景分层、
 * 边框、文字灰阶四类中性设计常量的唯一来源，亮/暗两套取值。
 *
 * 约定：
 * - 派生方向单向：组件库主题覆盖（`overrides.ts`）与新样式方案主题均从本模块
 *   派生，覆盖模块不持独立副本——token 变更波及两处视觉是单一来源的本意。
 * - 只收中性设计常量：强调色（品牌琥珀）不是中性常量，留在覆盖模块；业务语义
 *   色不并入，维持 `semantic-colors.ts` 的语义色接缝。
 * - 暗色取值维持现视觉语言（近黑底、微分层、细边框、克制圆角）。
 * - 亮色当前视觉 = 组件库出厂默认（issue #887 边界：亮色覆盖仅共享强调色，
 *   不借机补全），故亮色取值运行时取自 naive-ui 出厂亮色主题（`lightTheme`），
 *   与现状零漂移；待新样式方案主题派生落地时再收敛为手工定案取值。
 * - 是开发期常量，不是运行时状态：亮暗两套取值随主题切换整体生效，不逐值变化。
 */
export interface NeutralTokens {
  /** 圆角阶梯：small 细小组件 / base 基础组件 / large 卡片、弹窗级 */
  radius: { small: string; base: string; large: string }
  /** 背景分层：近黑 body → 略浅卡片 → 弹窗/浮层 */
  background: { body: string; card: string; popover: string; modal: string }
  /** 细边框代替重阴影 */
  border: { border: string; divider: string }
  /** 文字灰阶（克制的中灰，三级层级） */
  text: { primary: string; secondary: string; tertiary: string }
}

export const NEUTRAL_TOKENS: Record<Theme, NeutralTokens> = {
  dark: {
    radius: { small: '6px', base: '8px', large: '12px' },
    background: {
      body: '#0E0E10',
      card: '#161618',
      popover: '#1C1C1E',
      modal: '#1C1C1E',
    },
    border: {
      border: 'rgba(255, 255, 255, 0.08)',
      divider: 'rgba(255, 255, 255, 0.06)',
    },
    text: { primary: '#ECECEC', secondary: '#A0A0A0', tertiary: '#6E6E6E' },
  },
  light: {
    radius: {
      small: lightTheme.common.borderRadiusSmall,
      base: lightTheme.common.borderRadius,
      large: lightTheme.common.borderRadius,
    },
    background: {
      body: lightTheme.common.bodyColor,
      card: lightTheme.common.cardColor,
      popover: lightTheme.common.popoverColor,
      modal: lightTheme.common.modalColor,
    },
    border: {
      border: lightTheme.common.borderColor,
      divider: lightTheme.common.dividerColor,
    },
    text: {
      primary: lightTheme.common.textColor1,
      secondary: lightTheme.common.textColor2,
      tertiary: lightTheme.common.textColor3,
    },
  },
}

/**
 * 暗色浮层语言（Raycast 式「浮层比底层更亮」）：下拉菜单底色抬到比卡片更亮一阶，
 * 菜单激活项中性底色，以及「细边框 ring + 柔和投影」复合阴影。
 *
 * 当前仅暗色主题持有该取值——亮色主题的浮层/菜单激活底色为组件库出厂行为，
 * 无对应中性常量（不发明取值）；新样式方案亮色浮层落地时并入 token。
 */
export interface FloatLayerTokens {
  /** 浮层（下拉菜单）底色 */
  color: string
  /** 菜单/下拉激活项中性底色 */
  activeOverlay: string
  /** 菜单/下拉激活项悬停中性底色 */
  activeHoverOverlay: string
  /** 浮层「细边框 ring + 柔和投影」复合 box-shadow */
  ringShadow: string
}

export const DARK_FLOAT_LAYER: FloatLayerTokens = {
  color: '#343438',
  activeOverlay: 'rgba(255, 255, 255, 0.06)',
  activeHoverOverlay: 'rgba(255, 255, 255, 0.08)',
  ringShadow:
    '0 0 0 1px rgba(255, 255, 255, 0.12), 0 8px 24px rgba(0, 0, 0, 0.5)',
}
