export interface InstallmentSchedule {
  perPeriodCents: number;
  lastPeriodCents: number;
}

function isPositiveInt(v: number): boolean {
  return Number.isInteger(v) && v > 0;
}

export function installmentSchedule(
  totalCents: number,
  totalOccurrences: number,
): InstallmentSchedule {
  if (!isPositiveInt(totalCents)) {
    throw new Error(`分期总金额必须是正整数（分），收到：${totalCents}`);
  }
  if (!isPositiveInt(totalOccurrences)) {
    throw new Error(`分期期数必须是正整数，收到：${totalOccurrences}`);
  }
  const perPeriodCents = Math.floor(totalCents / totalOccurrences);
  const lastPeriodCents = totalCents - perPeriodCents * (totalOccurrences - 1);
  return { perPeriodCents, lastPeriodCents };
}
