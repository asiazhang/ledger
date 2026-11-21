import { h, type VNode } from "vue";
import type { TransactionSourceKind } from "@ledger/types";

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

function strokeRect(attrs: Record<string, string>): VNode {
  return h("rect", attrs);
}

export function copyIcon(): VNode {
  return svgIcon([
    strokeRect({ x: "128", y: "128", width: "336", height: "336", rx: "57" }),
    strokePath(
      "M383.5 128l.5-24a56.16 56.16 0 0 0-56-56H112a64.19 64.19 0 0 0-64 64v216a56.16 56.16 0 0 0 56 56h24",
    ),
  ]);
}

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

export function sourceKindIcon(kind: TransactionSourceKind): VNode {
  return SOURCE_KIND_SHAPES[kind]();
}
