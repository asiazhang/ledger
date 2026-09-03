/**
 * 字段错误态共享判定单点（词汇表「字段错误态」，界面状态与交互域；ADR-0058 / issue #414）。
 *
 * 全仓弹窗表单格式判定的唯一口径收口处：金额格式、必填判定与错误态装配均为
 * 同步纯函数，无 Vue / store / api 依赖。各表单消费同一口径，改判定只碰本文件；
 * 判定**时机**（输入中 / 失焦 / 保存尝试）由消费方按字段性质声明并以
 * FieldTiming 传入，本模块只收口径不代判时机。
 *
 * 口径（ADR-0058 决策 1）：
 * - 只覆盖格式类错误——解析失败、超出表示精度、必填为空；纯业务规则不成立
 *   （如金额纯零、负数）不在其列，走既有提交时校验通道；
 * - 即时判定：格式错误（解析失败 / 超精度）输入中即红；空值红在失焦或
 *   保存尝试时出现（初始为空不红，不惩罚尚未输入）；
 * - 不拦截、不静默丢弃是输入行为前提（消费方保证），本模块只对已保留的
 *   原始文本作判定。
 *
 * 测试：src/__tests__/field-error.test.ts 穷举格式错误闭集与装配时机表。
 */

/** 金额文本判定闭集 */
export type AmountJudgment =
  /** 合法，yuan 为元的数值（供装配器元转分；负数属业务类校验，不在此拦） */
  | { kind: 'ok'; yuan: number }
  /** 必填为空（空串或纯空白） */
  | { kind: 'empty' }
  /** 解析失败（非数字字符、多个小数点、科学计数法等，如 `4.30发`） */
  | { kind: 'parse-error' }
  /** 超出表示精度（金额以整数分表达，至多两位小数，如 `4.305`） */
  | { kind: 'over-precision' }

/** 数量文本判定闭集（交易数量：股数/份额，输入粒度至多四位小数，issue #416） */
export type QuantityJudgment =
  /** 合法，value 为数量的数值（负数属业务类校验，不在此拦） */
  | { kind: 'ok'; value: number }
  | { kind: 'empty' }
  | { kind: 'parse-error' }
  /** 超出输入粒度（多于四位小数，如 `1.23456`） */
  | { kind: 'over-precision' }

/** 价格文本判定闭集（单价：万分之一元刻度见价格刻度 ADR-0038，至多四位小数，issue #416） */
export type PriceJudgment =
  /** 合法，yuan 为元的数值（供装配器 yuanToPrice；负数属业务类校验，不在此拦） */
  | { kind: 'ok'; yuan: number }
  | { kind: 'empty' }
  | { kind: 'parse-error' }
  /** 超出表示精度（多于四位小数，如 `1.23456`） */
  | { kind: 'over-precision' }

/** 必填文本判定闭集（名称类自由文本字段用；推广期消费） */
export type RequiredTextJudgment = { kind: 'ok' } | { kind: 'empty' }

/** 字段错误类别（不含 ok） */
export type FieldErrorKind = Exclude<
  AmountJudgment | QuantityJudgment | PriceJudgment | RequiredTextJudgment,
  { kind: 'ok' }
>['kind']

/** 判定时机输入（消费方声明）：touched = 失焦过；saveAttempted = 发生过保存尝试 */
export interface FieldTiming {
  touched: boolean
  saveAttempted: boolean
}

/**
 * 小数文本判定内部共用体：形状校验（可选负号 + 数字 + 至多一个小数点，尾随
 * 小数点视为合法中间态避免逐键误红）+ 按字段粒度的超精度判定。金额/数量/价格
 * 三规则变体各自命名出口，改粒度只碰对应一行。
 */
function judgeDecimalText(
  text: string,
  maxDecimals: number,
): { kind: 'ok'; value: number } | { kind: 'empty' | 'parse-error' | 'over-precision' } {
  const trimmed = text.trim()
  if (!trimmed) return { kind: 'empty' }
  // 形状：可选负号，（整数部分 + 可选小数点及小数）或（纯小数）——『12.』合法、『1.2.3』非法
  if (!/^-?(?:\d+(?:\.\d*)?|\.\d+)$/.test(trimmed)) return { kind: 'parse-error' }
  const dot = trimmed.indexOf('.')
  if (dot !== -1 && trimmed.length - dot - 1 > maxDecimals) return { kind: 'over-precision' }
  return { kind: 'ok', value: Number(trimmed) }
}

/**
 * 金额文本 → 判定。输入是输入框中**原样保留**的原始文本（不拦截口径）。
 *
 * - 空串 / 纯空白 → empty（必填为空是否红由装配按时机判定）；
 * - 形状合法且至多两位小数（金额以整数分表达）→ ok（携带元的数值）；
 * - 形状非法 → parse-error；形状合法但超两位小数 → over-precision。
 *
 * 与既有口径的一致性：科学计数法（`1e3`）拒绝同 yuanToCents；两端空白容差
 * （trim 后判定）同 yuanToCents；负数可解析（业务类校验留提交通道，同旧
 * NInputNumber min=0 时代「提交时业务 toast」的净效果）。
 * 超安全整数范围的极端输入判 ok，由装配层 requireAmountCents 既有 fail fast 兜底。
 */
export function judgeAmountText(text: string): AmountJudgment {
  const judgment = judgeDecimalText(text, 2)
  return judgment.kind === 'ok' ? { kind: 'ok', yuan: judgment.value } : judgment
}

/**
 * 数量文本 → 判定（issue #416）。输入是输入框中**原样保留**的原始文本。
 * 输入粒度至多四位小数——与既有数量输入组件 precision=4 的约束一致；形状与
 * 容差口径（trim、尾随小数点、科学计数法拒绝、负数可解析）同金额。
 */
export function judgeQuantityText(text: string): QuantityJudgment {
  const judgment = judgeDecimalText(text, 4)
  return judgment.kind === 'ok' ? { kind: 'ok', value: judgment.value } : judgment
}

/**
 * 价格文本 → 判定（issue #416）。输入是输入框中**原样保留**的原始文本。
 * 至多四位小数——价格刻度以万分之一元为最小单位（价格刻度 ADR-0038，全投资域
 * 价格列统一口径）；形状与容差口径同金额。
 */
export function judgePriceText(text: string): PriceJudgment {
  const judgment = judgeDecimalText(text, 4)
  return judgment.kind === 'ok' ? { kind: 'ok', yuan: judgment.value } : judgment
}

/**
 * 必填文本 → 判定：null / undefined / 空串 / 纯空白为 empty，其余 ok。
 * 名称类自由文本字段（账户、分类、商户名等）推广期消费。
 */
export function judgeRequiredText(value: string | null | undefined): RequiredTextJudgment {
  return value != null && value.trim() ? { kind: 'ok' } : { kind: 'empty' }
}

/**
 * 错误态装配：字段判定 + 时机 → 当前错误类别（null = 无错误态）。
 *
 * - ok → 恒 null（合法即解除红态，「红态持续到修正」的解除侧）；
 * - parse-error / over-precision → 即时红，不读时机（输入中即判定即红）；
 * - empty → touched 或 saveAttempted 才红（初始为空不红；失焦或保存尝试触发）。
 */
export function fieldErrorKind(
  judgment: { kind: 'ok' | FieldErrorKind },
  timing: FieldTiming,
): FieldErrorKind | null {
  if (judgment.kind === 'ok') return null
  if (judgment.kind === 'empty') return timing.touched || timing.saveAttempted ? 'empty' : null
  return judgment.kind
}
