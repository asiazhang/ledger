import { h, type VNode } from "vue";
import type { TransactionSourceKind } from "@ledger/types";

/**
 * 静态矢量图标（issue #2189 交易表格行内高频组件瘦身）：交易表格单元格内的高频
 * 图标不再经 NIcon + @vicons 组件逐格实例化组件实例，改由本模块以纯 VNode
 * （原生 svg/path 元素）静态渲染——渲染函数直接产出元素节点，零组件实例开销。
 *
 * 形状数据逐元素转录自 @vicons/ionicons5 同名 outline 图标（fill=none、
 * stroke=currentColor、stroke-width=32 的既有呈现，与 NIcon 渲染逐像素一致）；
 * viewBox 与 @vicons 一致（512）。与 SourceLink 的 KIND_ICONS 同闭集同源惯例：
 * 新增来源类型时两处同步补行（编译器 Record 强制穷尽）。
 *
 * 图标以工厂函数产出（每次调用新建 VNode）：同一图标形状会出现在多行多个
 * 单元格，跨单元格复用同一 VNode 实例是 Vue 未定义行为，工厂保证逐格独立。
 */

/** 静态图标 svg 包裹（512 viewBox + currentColor 描边继承，尺寸由调用方 class/样式控制）。 */
function svgIcon(children: VNode[]): VNode {
  return h(
    "svg",
    {
      xmlns: "http://www.w3.org/2000/svg",
      viewBox: "0 0 512 512",
      fill: "none",
      stroke: "currentColor",
      "stroke-width": "32",
      "stroke-linecap": "round",
      "stroke-linejoin": "round",
    },
    children,
  );
}

function strokePath(d: string): VNode {
  return h("path", { d });
}

/** 描边矩形（CalendarClear 的外框；闭合图形 linecap 无渲染影响，基准同 svg 根）。 */
function strokeRect(attrs: Record<string, string>): VNode {
  return h("rect", attrs);
}

/** 复制图标（CopyOutline）：备注复制按钮的共享静态图标。 */
export function copyIcon(): VNode {
  return svgIcon([
    strokeRect({ x: "128", y: "128", width: "336", height: "336", rx: "57" }),
    strokePath(
      "M383.5 128l.5-24a56.16 56.16 0 0 0-56-56H112a64.19 64.19 0 0 0-64 64v216a56.16 56.16 0 0 0 56 56h24",
    ),
  ]);
}

/** 来源类型 → 静态图标形状（六类闭集穷尽映射，与 SourceLink KIND_ICONS 同源惯例：
 * 保单=盾、物品=立方、标的=走势、订阅=循环、转账=双向、分期=日历）。 */
const SOURCE_KIND_SHAPES: Record<TransactionSourceKind, () => VNode> = {
  installmentPlan: () =>
    svgIcon([
      strokeRect({ x: "48", y: "80", width: "416", height: "384", rx: "48" }),
      strokePath("M128 48v32"),
      strokePath("M384 48v32"),
      strokePath("M464 160H48"),
    ]),
  subscription: () =>
    svgIcon([
      strokePath("M320 120l48 48l-48 48"),
      strokePath("M352 168H144a80.24 80.24 0 0 0-80 80v16"),
      strokePath("M192 392l-48-48l48-48"),
      strokePath("M160 344h208a80.24 80.24 0 0 0 80-80v-16"),
    ]),
  scheduledTransfer: () =>
    svgIcon([
      strokePath("M304 48l112 112l-112 112"),
      strokePath("M398.87 160H96"),
      strokePath("M208 464L96 352l112-112"),
      strokePath("M114 352h302"),
    ]),
  policy: () =>
    svgIcon([
      strokePath("M336 176L225.2 304L176 255.8"),
      strokePath(
        "M463.1 112.37C373.68 96.33 336.71 84.45 256 48c-80.71 36.45-117.68 48.33-207.1 64.37C32.7 369.13 240.58 457.79 256 464c15.42-6.21 223.3-94.87 207.1-351.63z",
      ),
    ]),
  item: () =>
    svgIcon([
      strokePath(
        "M448 341.37V170.61A32 32 0 0 0 432.11 143l-152-88.46a47.94 47.94 0 0 0-48.24 0L79.89 143A32 32 0 0 0 64 170.61v170.76A32 32 0 0 0 79.89 369l152 88.46a48 48 0 0 0 48.24 0l152-88.46A32 32 0 0 0 448 341.37z",
      ),
      strokePath("M69 153.99l187 110l187-110"),
      strokePath("M256 463.99v-200"),
    ]),
  instrument: () =>
    svgIcon([
      strokePath("M352 144h112v112"),
      strokePath(
        "M48 368l121.37-121.37a32 32 0 0 1 45.26 0l50.74 50.74a32 32 0 0 0 45.26 0L448 160",
      ),
    ]),
};

/** 来源类型静态图标（每次调用产出独立 VNode，逐格安全）。 */
export function sourceKindIcon(kind: TransactionSourceKind): VNode {
  return SOURCE_KIND_SHAPES[kind]();
}
