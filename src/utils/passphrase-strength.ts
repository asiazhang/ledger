/**
 * 口令强度评估收口单点（词汇表「口令强度」，备份与数据文件域；issue #685）。
 *
 * 收口「zxcvbn 调用 + score→档位映射」两件事：组件（EncryptionSettings）只消费
 * 本模块出口，不直接触碰 zxcvbn；不并入 field-error.ts——那是字段错误态闭集
 * （ADR-0058），强度是纯信息反馈，不拦截提交、不改提交可用性。
 *
 * 口径（issue #685 定稿）：
 * - zxcvbn score 0–1 弱 / 2 中 / 3 强 / 4 极强四档闭集；
 * - 空输入不评估（初始为空不显示，沿字段错误态「不惩罚尚未输入」精神）；
 * - 不显示破解时间估算等伪精确数值，只出档位与色条填充刻度；
 * - zxcvbn 字典包体积较大，经动态 import 惰性加载：首次评估时加载一次并缓存，
 *   之后逐键同步计算（毫秒级，无需防抖）。
 *
 * 测试：src/__tests__/passphrase-strength.test.ts 钉死闭集映射与典型样例。
 */

/** 口令强度档位闭集（zxcvbn score 0–1 弱 / 2 中 / 3 强 / 4 极强） */
export type PassphraseStrengthTier = 'weak' | 'medium' | 'strong' | 'very-strong'

/** 单次评估结果：zxcvbn 原始 score + 档位 + 色条填充百分比（纯展示刻度） */
export interface PassphraseStrengthAssessment {
  score: 0 | 1 | 2 | 3 | 4
  tier: PassphraseStrengthTier
  /** 色条填充百分比（(score+1)×20%），同档内保留刻度差，仅展示用 */
  percent: number
}

type ZxcvbnScore = PassphraseStrengthAssessment['score']

/**
 * score→档位闭集映射（同步纯函数）：0–1 归弱、2/3/4 各自成档。
 * 映射口径全仓唯此一处，改档位边界只碰本函数。
 */
export function strengthForScore(score: ZxcvbnScore): PassphraseStrengthAssessment {
  const tier: PassphraseStrengthTier = score <= 1 ? 'weak' : score === 2 ? 'medium' : score === 3 ? 'strong' : 'very-strong'
  return { score, tier, percent: (score + 1) * 20 }
}

/** 惰性加载的 zxcvbn 打分器（首次评估时加载字典包并缓存实例） */
let checkerPromise: Promise<(password: string) => ZxcvbnScore> | null = null

function loadChecker(): Promise<(password: string) => ZxcvbnScore> {
  checkerPromise ??= Promise.all([
    import('@zxcvbn-ts/core'),
    import('@zxcvbn-ts/language-common'),
  ]).then(([{ ZxcvbnFactory }, { dictionary, adjacencyGraphs }]) => {
    const factory = new ZxcvbnFactory({
      dictionary: { ...dictionary },
      graphs: adjacencyGraphs,
    })
    return (password: string) => factory.check(password).score
  })
  return checkerPromise
}

/**
 * 口令文本 → 评估结果：空输入 → null（不评估）；非空经 zxcvbn 打分后映射档位。
 * 返回 Promise 只承载首次字典包加载，评分本身同步毫秒级；调用方以「最后一次
 * 胜出」守卫消费即可保证逐键刷新不串档。
 */
export async function assessPassphraseStrength(input: string): Promise<PassphraseStrengthAssessment | null> {
  if (!input) return null
  const checker = await loadChecker()
  return strengthForScore(checker(input))
}
