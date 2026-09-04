import { useMessage } from 'naive-ui'
import type { Ref } from 'vue'
import { useModalIntent } from '@/composables/useModalIntent'
import { api } from '@/api'
import { errorMessage } from '@/utils/errors'
import { t } from '@/i18n'
import type { CreateFormKind, Transaction, TransactionTrade } from '@/types'

/**
 * TransactionModalState 交易弹窗编排深模块（ADR-0045，词汇表「TransactionModalState（交易弹窗编排）」）：
 * 交易列表四个弹窗（记一笔 / 退款 / 编辑 / 加入物品）共享的「开启 / 目标 / 关闭」编排——
 * 意图闭集四的单一判别联合是唯一事实源，显示开关由「意图非空」派生（无独立 show 布尔，
 * 不存在「目标非空但已关闭」的中间态）；回调序号随 open 递增内化（作表单 key 强制重建）；
 * 编辑 buy/sell 的「先取买卖明细再开窗、失败不开窗」异步时序与慢取竞态守卫（last-open-wins）
 * 内化其中，慢取/失败行为一处定义。
 *
 * 已迁为弹窗意图编排通用工厂 ModalIntent（useModalIntent，ADR-0072）之上的首个适配器：
 * 意图与序号两面由工厂持有（意图落位即递增序号、关闭不重置），本模块只补交易弹窗族
 * 特有的异步时序——「先取买卖明细再开窗、失败不开窗、最后一次开启胜出」守卫留适配器层，
 * 不上浮通用工厂。适配器代数计数器与工厂序号是两个计数器：代数随每次开启尝试递增
 * （含取数失败），序号只在意图真正落位递增，语义不同、不合并。
 *
 * 依赖 direct-import（api 与 useMessage），不做注入（先例 useScheduledPlanList；
 * getTransactionTrade 只有一个实现，注入是 YAGNI）。只内化「关闭」——列表刷新
 * （翻回第一页或保持当前页）仍归视图，弹窗编排与 TransactionFilter 两个深模块正交。
 */

// ---------------------------------------------------------------------------
// 意图模型（闭集四：单一判别联合）
// ---------------------------------------------------------------------------

/**
 * 意图状态（单一判别联合，弹窗编排的唯一事实源）：
 * - create：无目标行，携带表单形态子类型（issue #374 起 CreateFormKind：可创建 kind +
 *   借贷两个呈现变体；refund 不在可创建集，入口由交易条目右键承接）；
 * - refund / add-item：携带目标交易行；
 * - edit：另携带买卖明细（非买卖行为 null；buy/sell 的明细由模块先取再开窗，调用方不经手）。
 * 视图以 `intent?.type` 判别渲染，payload 在各分支内被类型系统收窄。
 */
export type TransactionModalIntent =
  | { type: 'create'; kind: CreateFormKind }
  | { type: 'refund'; row: Transaction }
  | { type: 'edit'; row: Transaction; trade: TransactionTrade | null }
  | { type: 'add-item'; row: Transaction }

/**
 * open 入参（开启请求）：与意图状态同构，唯 edit 只携目标行——明细由模块内化取数，
 * 不出现在调用方面上。
 */
export type TransactionModalOpenRequest =
  | { type: 'create'; kind: CreateFormKind }
  | { type: 'refund'; row: Transaction }
  | { type: 'edit'; row: Transaction }
  | { type: 'add-item'; row: Transaction }

// ---------------------------------------------------------------------------
// 工厂
// ---------------------------------------------------------------------------

export interface UseTransactionModalStateReturn {
  /** 当前意图（只读）：null = 关闭终态（弹窗不显示）；非空即「弹窗显示」。 */
  readonly intent: Readonly<Ref<TransactionModalIntent | null>>
  /** 回调序号：随每次成功 open 递增（作表单 key 强制重建实例）；被竞态丢弃的 open 不递增。 */
  readonly seq: Readonly<Ref<number>>
  /** 开启意图：统一异步签名（仅 edit 真正 await——先取明细再开窗，失败报错不开窗）。 */
  open(request: TransactionModalOpenRequest): Promise<void>
  /** 关闭：意图清回 null 终态（列表刷新等关闭后副作用仍归视图）。 */
  close(): void
}

/**
 * 交易弹窗编排工厂：每次调用返回独立实例（意图与序号不串扰）。
 * 须在组件 setup 内调用（错误提示经 useMessage，与仓库既有 composable 形态一致）。
 */
export function useTransactionModalState(): UseTransactionModalStateReturn {
  const message = useMessage()

  // 意图与序号归 ModalIntent 工厂所有（ADR-0072）：意图非空即显示、落位即递增序号、
  // 关闭清回 null 终态；本适配器经工厂 open/close 驱动，不再直写这两面状态。
  const { intent, seq, open: landIntent, close: clearIntent } = useModalIntent<TransactionModalIntent>()

  /**
   * 代数守卫（last-open-wins）：每次 open 递增一代；异步取数返回后，代数已过期
   * （期间又有新的 open 或 close）则整体丢弃——不落意图（工厂序号不递增）、不开窗，
   * 迟到的失败也不报错。消灭「慢 A 覆盖快 B」竞态；close 一并推进代数，
   * 使「关闭清回空终态」成为接口保证——取数在途时关闭，迟到的成功不再重开弹窗。
   *
   * 代数与工厂序号是两个计数器（ADR-0072）：代数随每次开启尝试递增（含取数失败，
   * 失败路径只推进代数、序号不动）；序号只在意图真正落位时经工厂递增。
   */
  let generation = 0

  /** 结算一次开启：代数仍最新才经工厂落位意图并递增序号（同步意图即时结算，edit 待取数后结算）。 */
  function settle(gen: number, next: TransactionModalIntent) {
    if (gen !== generation) return
    landIntent(next)
  }

  async function open(request: TransactionModalOpenRequest): Promise<void> {
    const gen = ++generation
    if (request.type === 'create') {
      settle(gen, { type: 'create', kind: request.kind })
      return
    }
    if (request.type === 'refund' || request.type === 'add-item') {
      settle(gen, { type: request.type, row: request.row })
      return
    }
    // edit：先取买卖明细再开窗（时序内化）。非买卖行无明细面，开窗即开。
    const { row } = request
    if (row.kind !== 'buy' && row.kind !== 'sell') {
      settle(gen, { type: 'edit', row, trade: null })
      return
    }
    try {
      const trade = await api.getTransactionTrade(row.id)
      settle(gen, { type: 'edit', row, trade })
    } catch (e) {
      if (gen !== generation) return // 迟到的失败整体丢弃
      message.error(t('transactions.modal.editFailed', { msg: errorMessage(e) }))
    }
  }

  function close() {
    generation += 1 // 关闭推进代数：取数在途时关闭，迟到的成功不再重开弹窗
    clearIntent()
  }

  return {
    intent,
    seq,
    open,
    close,
  }
}
