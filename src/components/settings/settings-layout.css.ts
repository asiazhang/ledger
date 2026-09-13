import { globalStyle } from '@vanilla-extract/css'

/**
 * 设置页卡片布局（issue #651 修订，ADR-0093 样式方案）：与设置组件同目录共置。
 *
 * 原「内容列固定 720px、左对齐」在宽屏下把剩余空间整块空着；改为单列动态铺满
 * 内容区可用宽度——不新增窗口断点，也不分栏；窗口缩放、侧栏随语言切换
 * （160 / 200px）时卡片与宽表（备份文件列表）实时跟随。
 */

/** 设置列钩子类：SettingsView 内容列根元素。 */
export const SETTINGS_COLUMN_CLASS = 'settings-column'

/** 卡片堆叠钩子类：多卡容器的单列等距堆叠。 */
export const SETTINGS_CARD_STACK_CLASS = 'settings-card-stack'

/** 设置列：吃满内容区可用宽度（原 720px 上限移除）。 */
globalStyle(`.${SETTINGS_COLUMN_CLASS}`, {
  width: '100%',
})

/** 卡片堆叠：单列铺满、16px 行距（与既有 NSpace vertical size=16 节奏一致）。 */
globalStyle(`.${SETTINGS_CARD_STACK_CLASS}`, {
  display: 'grid',
  gridTemplateColumns: 'minmax(0, 1fr)',
  gap: '16px',
  width: '100%',
})
