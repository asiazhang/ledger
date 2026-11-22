export type AmountJudgment =
  | { kind: "ok"; yuan: number }
  | { kind: "empty" }
  | { kind: "parse-error" }
  | { kind: "over-precision" };

export type QuantityJudgment =
  | { kind: "ok"; value: number }
  | { kind: "empty" }
  | { kind: "parse-error" }
  | { kind: "over-precision" };

export type PriceJudgment =
  | { kind: "ok"; yuan: number }
  | { kind: "empty" }
  | { kind: "parse-error" }
  | { kind: "over-precision" };

export type RequiredTextJudgment = { kind: "ok" } | { kind: "empty" };

export type MinLengthJudgment = { kind: "ok" } | { kind: "empty" } | { kind: "too-short" };

export type FieldErrorKind = Exclude<
  AmountJudgment | QuantityJudgment | PriceJudgment | RequiredTextJudgment | MinLengthJudgment,
  { kind: "ok" }
>["kind"];

export interface FieldTiming {
  touched: boolean;
  saveAttempted: boolean;
}

function judgeDecimalText(
  text: string,
  maxDecimals: number,
): { kind: "ok"; value: number } | { kind: "empty" | "parse-error" | "over-precision" } {
  const trimmed = text.trim();
  if (!trimmed) return { kind: "empty" };
  if (!/^-?(?:\d+(?:\.\d*)?|\.\d+)$/.test(trimmed)) return { kind: "parse-error" };
  const dot = trimmed.indexOf(".");
  if (dot !== -1 && trimmed.length - dot - 1 > maxDecimals) return { kind: "over-precision" };
  return { kind: "ok", value: Number(trimmed) };
}

export function judgeAmountText(text: string): AmountJudgment {
  const judgment = judgeDecimalText(text, 2);
  return judgment.kind === "ok" ? { kind: "ok", yuan: judgment.value } : judgment;
}

export function judgeQuantityText(text: string): QuantityJudgment {
  const judgment = judgeDecimalText(text, 4);
  return judgment.kind === "ok" ? { kind: "ok", value: judgment.value } : judgment;
}

export function judgePriceText(text: string): PriceJudgment {
  const judgment = judgeDecimalText(text, 4);
  return judgment.kind === "ok" ? { kind: "ok", yuan: judgment.value } : judgment;
}

export function judgeRequiredText(value: string | null | undefined): RequiredTextJudgment {
  return value != null && value.trim() ? { kind: "ok" } : { kind: "empty" };
}

export function judgeMinLengthText(text: string, min: number): MinLengthJudgment {
  if (!text) return { kind: "empty" };
  return text.length >= min ? { kind: "ok" } : { kind: "too-short" };
}

export function fieldErrorKind(
  judgment: { kind: "ok" | FieldErrorKind },
  timing: FieldTiming,
): FieldErrorKind | null {
  if (judgment.kind === "ok") return null;
  if (judgment.kind === "empty") return timing.touched || timing.saveAttempted ? "empty" : null;
  return judgment.kind;
}
