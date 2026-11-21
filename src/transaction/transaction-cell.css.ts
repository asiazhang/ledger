import { globalStyle } from "@vanilla-extract/css";

/**
 * 交易表格单元格轻量元素旁路样式（issue #2189 行内高频组件瘦身，ADR-0093
 * 样式方案与组件同目录共置）：类型标签、账户/商户/来源链接、备注复制按钮与
 * 操作列「⋯」按钮改用轻量原生元素（列渲染函数产出，零组件实例），样式收口
 * 本文件。类名为稳定钩子类（测试与样式同键），不覆盖任何 naive 内部类。
 *
 * 视觉等价锚点：链接形态与 AccountLink / MerchantLink / SourceLink（组件版，
 * 供移动档卡片列表与报表下钻等表格外消费方继续使用）逐条一致——强调色内联
 * 注入（列级按当前主题解析一次），hover/focus 态走 `--tx-accent-hover` 自定义
 * 属性；类型标签与状态标注色型经 @ledger/theme tag-colors 按主题解析（与 NTag
 * 公式对齐，见该模块）内联注入。
 */

/** —— 钩子类常量（渲染函数与测试同键消费）—— */

/** 单元格内链接式原生按钮钩子类（账户/商户/来源下钻）。 */
export const TX_CELL_LINK_CLASS = "tx-cell-link";
/** 不可点击占位文本钩子类。 */
export const TX_CELL_PLACEHOLDER_CLASS = "tx-cell-placeholder";
/** 类型标签钩子类。 */
export const TX_KIND_TAG_CLASS = "tx-kind-tag";
/** 来源单元格容器钩子类。 */
export const TX_SOURCE_CELL_CLASS = "tx-source-cell";
/** 来源类型图标钩子类。 */
export const TX_SOURCE_ICON_CLASS = "tx-source-icon";
/** 触控轴常驻类型全称小字钩子类。 */
export const TX_SOURCE_TYPE_LABEL_CLASS = "tx-source-type-label";
/** 来源状态标注钩子类。 */
export const TX_SOURCE_STATUS_TAG_CLASS = "tx-source-status-tag";
/** 备注列单元格容器钩子类。 */
export const TX_NOTE_CELL_CLASS = "tx-note-cell";

/** 单元格内链接式原生按钮（账户/商户/来源下钻）：文本按钮形态，与组件版一致。 */
globalStyle(`.tx-cell-link`, {
  border: "none",
  padding: "0",
  background: "none",
  font: "inherit",
  cursor: "pointer",
  borderRadius: "4px",
  minWidth: "0",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  textAlign: "left",
});

globalStyle(`.tx-cell-link:hover`, {
  color: "var(--tx-accent-hover)",
  background: "rgba(255, 255, 255, 0.06)",
  textDecoration: "underline",
});

/** 焦点可见（键盘可达性）：强调色焦点环，色值随列级内联注入的 --tx-accent-hover。 */
globalStyle(`.tx-cell-link:focus-visible`, {
  outline: "2px solid var(--tx-accent-hover)",
  outlineOffset: "2px",
});

/** 不可点击占位文本（黑洞/隐藏账户、未知商户、软删保单名称）：继承色纯文本省略。 */
globalStyle(`.tx-cell-placeholder`, {
  maxWidth: "100%",
  minWidth: "0",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
});

/** 类型标签（轻量语义样式元素，色型内联注入）：NTag medium 度量等价
 * （height 28px / font 14px / padding 0 7px / line-height 1，naive Tag self）。 */
globalStyle(`.tx-kind-tag`, {
  display: "inline-flex",
  alignItems: "center",
  boxSizing: "border-box",
  height: "28px",
  padding: "0 7px",
  fontSize: "14px",
  lineHeight: "1",
  whiteSpace: "nowrap",
});

/** 来源单元格容器（图标 + 名称 + 可空标注并排）。 */
globalStyle(`.tx-source-cell`, {
  display: "inline-flex",
  alignItems: "center",
  gap: "4px",
  maxWidth: "100%",
  /* 图标随内容着色后降低存在感：类型是扫读线索不是主信息（SourceLink 同值） */
  color: "inherit",
  opacity: "0.92",
});

globalStyle(`.tx-source-icon`, {
  flex: "none",
  width: "1em",
  height: "1em",
  opacity: "0.65",
});

/** 触控轴常驻类型全称小字（title 不可达的一击可达替代，SourceLink 同值）。 */
globalStyle(`.tx-source-type-label`, {
  flex: "none",
  fontSize: "11px",
  opacity: "0.55",
});

/** 来源状态标注（轻量中性标签：色型内联注入）。NTag small 度量等价
 * （height 22px / font 12px / padding 0 7px，naive Tag small self）。 */
globalStyle(`.tx-source-status-tag`, {
  display: "inline-flex",
  alignItems: "center",
  boxSizing: "border-box",
  flex: "none",
  height: "22px",
  padding: "0 7px",
  fontSize: "12px",
  lineHeight: "1",
  whiteSpace: "nowrap",
});

/** 表格内轻量原生按钮（备注复制、操作列「⋯」）：替代 NButton tiny quaternary
 * ——移除波纹层/加载态/主题混入实例开销，原生按钮基座与焦点环收口此处；
 * 悬停显现/触控常驻规则仍在 global.css（.note-copy-btn 既有规则，元素换形态
 * 后语义不变）。仅作用于表格内（.n-data-table 前缀），移动档卡片的 NButton
 * 「⋯」（同类名）不受影响。 */
globalStyle(`.n-data-table .note-copy-btn, .n-data-table .row-actions-btn`, {
  border: "none",
  background: "none",
  padding: "0",
  cursor: "pointer",
  color: "inherit",
  font: "inherit",
  borderRadius: "4px",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
});

globalStyle(
  `.n-data-table .note-copy-btn:focus-visible, .n-data-table .row-actions-btn:focus-visible`,
  {
    outline: "2px solid var(--tx-accent-hover, currentColor)",
    outlineOffset: "1px",
  },
);

/** 复制图标尺寸（共享静态 svg，14px 与 NIcon size=14 等价）。 */
globalStyle(`.n-data-table .note-copy-btn svg`, {
  width: "14px",
  height: "14px",
});

/** 备注列单元格内布局（文本占满剩余宽度、复制按钮固定宽度靠右）与既有
 * NOTE_CELL_STYLE 同值；收敛为类（同款声明在列渲染函数外单点）。 */
globalStyle(`.tx-note-cell`, {
  display: "flex",
  alignItems: "center",
  gap: "2px",
  width: "100%",
  maxWidth: "100%",
});
