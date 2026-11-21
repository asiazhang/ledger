import type { Theme } from "./theme";
import type { TransactionKind } from "@ledger/types";

export interface SemanticColor {
  light: string;
  dark: string;
}

export const KIND_SEMANTIC_COLORS: Record<TransactionKind, SemanticColor> = {
  expense: { light: "#d03050", dark: "#e88080" },
  income: { light: "#18a058", dark: "#63e2b7" },
  refund: { light: "#2080f0", dark: "#63a8f2" },
  transfer: { light: "#722ed1", dark: "#b37feb" },
  buy: { light: "#eb2f96", dark: "#ff85c0" },
  sell: { light: "#13c2c2", dark: "#5cdbd3" },
  convert: { light: "#d48806", dark: "#f0c060" },
  split: { light: "#2f54eb", dark: "#85a5ff" },
  dividend: { light: "#0d9488", dark: "#5eead4" },
};

export const SEMANTIC_COLOR_KINDS = Object.keys(KIND_SEMANTIC_COLORS) as TransactionKind[];

export function kindSemanticColor(kind: TransactionKind, theme: Theme): string {
  return KIND_SEMANTIC_COLORS[kind][theme];
}

export const PNL_COLORS = {
  gain: { light: "#d03050", dark: "#e88080" },
  loss: { light: "#18a058", dark: "#63e2b7" },
} as const;

export function pnlSemanticColor(cents: number, theme: Theme): string {
  return cents >= 0 ? PNL_COLORS.gain[theme] : PNL_COLORS.loss[theme];
}
