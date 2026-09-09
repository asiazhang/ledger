import { globalStyle } from '@vanilla-extract/css'

/**
 * AppModal 移动档全屏化分支的旁路样式（issue #844 / ADR-0088 决策 8，ADR-0093
 * 样式方案）：与组件同目录共置。桌面档零渗透——全部规则以移动钩子类打头，钩子
 * 类仅由 AppModal 在窗口分级落移动档（<840，ADR-0088 决策 2）时挂上卡片根元素。
 *
 * 声明分两类：
 * - **近全屏尺寸不走本文件**：naive 运行时注入的 `.n-card { width: 100% }` 晚于
 *   构建期 CSS 入册，类规则同特异性时后来者胜——宽度/高度走 AppModal 的内联
 *   style（与桌面 cardSize 同一机制，恒胜类规则），本文件只承载内联表达不了的
 *   结构性布局。
 * - **覆盖 naive 内部类的规则一律提高一级特异性**（钩子类 + naive 修饰类并列），
 *   压过 naive 自身嵌套选择器（如 `.n-form-item .n-form-item-label`），不依赖
 *   注入顺序；只覆盖 naive 未声明或需翻转的属性。
 *
 * 词汇表「对话框排版」移动档条款：近全屏卡片、标签上置、按钮行底部固定；三档
 * 宽度（cardSize）是桌面档口径，移动档以全屏化为唯一形态。
 */

/** 移动档卡片钩子类：AppModal 按窗口分级挂到卡片根元素（.n-card 同元素）。 */
export const MOBILE_CARD_CLASS = 'app-modal-mobile-card'

/** 卡片本体：圆角裁切内部滚动内容（✕ 标题栏与卡片边缘不露内容）。 */
globalStyle(`.${MOBILE_CARD_CLASS}`, {
  overflow: 'hidden',
})

/**
 * 内容区滚动方案（lg 表格详情类全屏化后的可读性，issue #844 范围项）：内容区
 * 自身纵向滚动，子项不压缩（表格等天然高度内容随滚动可达，而非被 flex 压扁）；
 * naive 已声明的 flex: 1（内容区在卡片 flex 列中撑满 ✕ 头部以下空间）不动。
 */
globalStyle(`.${MOBILE_CARD_CLASS} .n-card-content`, {
  display: 'flex',
  flexDirection: 'column',
  minHeight: 0,
  overflowY: 'auto',
})

globalStyle(`.${MOBILE_CARD_CLASS} .n-card-content > *`, {
  flexShrink: 0,
})

/**
 * 标签上置：左置标签表单（ADR-0079 表单约定 label-placement="left"）在移动档
 * 翻为上下堆叠（居左标签在窄屏挤压输入宽度，词汇表移动档条款）——grid 区域
 * 翻转复刻 naive 自身 top-labelled 布局，标签文字改居左（left 档 naive 默认
 * 右对齐）；align-items 复位 stretch（left-labelled 的 flex-start 会让堆叠后
 * 的标签/控件行收缩不铺满）。top-labelled 表单天然满足，无需处理。
 */
globalStyle(
  `.${MOBILE_CARD_CLASS} .n-form-item.n-form-item--left-labelled`,
  {
    gridTemplateAreas: '"label" "blank" "feedback"',
    gridTemplateColumns: 'minmax(0, 100%)',
    gridTemplateRows: 'minmax(var(--n-label-height), auto) 1fr',
    alignItems: 'stretch',
  },
)

globalStyle(`.${MOBILE_CARD_CLASS} .n-form-item--left-labelled .n-form-item-label`, {
  textAlign: 'left',
})

/**
 * 按钮行底部固定：节奏容器（ADR-0079 决策 4 的 12px NSpace vertical）撑满
 * 剩余高度，其末块（「取消 + 主操作」按钮行，调用点统一编排在此）以 auto
 * 外边距推至卡片底部——主操作拇指可及。内容超高时滚动、auto 边距归零，
 * 按钮行随内容流（遮罩不关原则不动，表单不因固定按钮遮挡丢内容）。两个
 * 作用面：表单弹窗的节奏容器（表单自身先撑满剩余高度）；无表单内容形态
 * （确认框族：说明段 + 按钮行的裸节奏容器直挂内容区）。inline 表单（页面
 * 级筛选形态）不是弹窗表单约定对象，豁免。
 */
globalStyle(`.${MOBILE_CARD_CLASS} form.n-form:not(.n-form--inline)`, {
  display: 'flex',
  flexDirection: 'column',
  flexGrow: 1,
})

globalStyle(
  `.${MOBILE_CARD_CLASS} form.n-form:not(.n-form--inline) > .n-space, .${MOBILE_CARD_CLASS} .n-card-content > .n-space`,
  {
    flexGrow: 1,
  },
)

globalStyle(
  `.${MOBILE_CARD_CLASS} form.n-form:not(.n-form--inline) > .n-space > :last-child, .${MOBILE_CARD_CLASS} .n-card-content > .n-space > :last-child`,
  {
    marginTop: 'auto',
  },
)
