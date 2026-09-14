/**
 * 分期每期金额预览（issue #204）。
 *
 * 口径唯一来源是后端分期期次生成（src-tauri/src/scheduled_transactions/engine.rs
 * expand_occurrences 的 installment 分支）：floor 均分、尾差进最后一期。
 * 本函数只是把同一口径搬到前端做实时预览，禁止在此之外再写第二套分期金额表达式。
 */
export interface InstallmentSchedule {
  /** 每期金额（floor 均分，单位：分） */
  perPeriodCents: number
  /** 最后一期金额 = 每期金额 + 尾差（单位：分）；整除时与 perPeriodCents 相等 */
  lastPeriodCents: number
}

/** 校验正整数（金额分 / 期数共用）。 */
function isPositiveInt(v: number): boolean {
  return Number.isInteger(v) && v > 0
}

/**
 * 按总额与期数计算每期金额与最后一期（含尾差）。
 * @param totalCents 分期总金额（分，正整数）
 * @param totalOccurrences 总期数（正整数）
 * @throws 非法输入（非正整数）时抛错
 */
export function installmentSchedule(
  totalCents: number,
  totalOccurrences: number,
): InstallmentSchedule {
  if (!isPositiveInt(totalCents)) {
    throw new Error(`分期总金额必须是正整数（分），收到：${totalCents}`)
  }
  if (!isPositiveInt(totalOccurrences)) {
    throw new Error(`分期期数必须是正整数，收到：${totalOccurrences}`)
  }
  const perPeriodCents = Math.floor(totalCents / totalOccurrences)
  const lastPeriodCents = totalCents - perPeriodCents * (totalOccurrences - 1)
  return { perPeriodCents, lastPeriodCents }
}
