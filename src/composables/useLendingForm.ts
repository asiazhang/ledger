import { computed, ref } from 'vue'
import { useTransferForm } from '@/composables/useTransferForm'
import { useFormShared } from '@/composables/useFormShared'
import { t } from '@/i18n'
import {
  LENDING_DIRECTION_SIDES,
  accountMatchesSide,
  resolveLendingDirection,
  type LendingFormDirection,
} from '@/domain/lending'
import type { AccountType, Transaction } from '@/types'

/**
 * 借贷录入 = 转账表单的借贷变体（issue #374 / ADR-0053）：金额/币种/日期/备注、
 * 语义校验、装配与提交路由全部复用 useTransferForm（提交产物与转账同构：
 * kind=transfer + 双账户 + 金额/币种/日期/备注），仅叠加——
 * - 方向状态（预置方向 + toggle 覆盖借出/收回/借入/还款四方向）；
 * - 账户选择器按方向过滤（借出/收回：资金账户 ↔ receivable；借入/还款：debt ↔ 资金账户）；
 * - 方向切换时的已选账户处置（反向方向交换两端，其余清掉越侧选择）。
 */
export function useLendingForm(options?: {
  /** 创建入口的预置方向（「借出」「借入」两个入口项各预设其一）；编辑模式优先按既有
   * 交易派生（与表单分派同用 domain 层 resolveLendingDirection），派生失败（账户类型
   * 缺失）时以此兑底。 */
  initialDirection?: LendingFormDirection
  onCreated?: () => void
  onUpdated?: () => void
  /** 编辑模式：与 useTransferForm editing 同约定（创建时读一次回填、提交时重读定目标） */
  editing?: () => Transaction | null
}) {
  const { reference } = useFormShared()

  const accountTypeOf = (id: string | null | undefined): AccountType | undefined =>
    id == null ? undefined : reference.accountMap.get(id)?.type

  // 编辑模式先按既有交易派生方向（形态识别与表单分派同一函数）；
  // 派生不出（账户类型缺失等）回退入口预置方向，不误判。
  const editingTx = options?.editing?.() ?? null
  const direction = ref<LendingFormDirection>(
    editingTx
      ? (resolveLendingDirection(editingTx, accountTypeOf) ?? options?.initialDirection ?? 'lend')
      : (options?.initialDirection ?? 'lend'),
  )

  // 创建成功提示按提交时的当前方向给专属文案（getter：方向在表单存续期可切换）
  const transfer = useTransferForm({
    onCreated: options?.onCreated,
    onUpdated: options?.onUpdated,
    editing: options?.editing,
    createdMessage: () =>
      t('transactions.lending.created', {
        dir: t(`transactions.lending.${direction.value}`),
      }),
  })

  /** 某侧（from/to）可选项：当前方向该侧允许侧别内的账户（过滤表见 domain 借贷模块） */
  function accountOptionsFor(sideKey: 'from' | 'to') {
    return computed(() => {
      const side = LENDING_DIRECTION_SIDES[direction.value][sideKey]
      return reference.accounts
        .filter((a) => accountMatchesSide(a.type, side))
        .map((a) => ({ label: a.name, value: a.id }))
    })
  }

  /** 转出侧可选项 */
  const fromAccountOptions = accountOptionsFor('from')

  /** 转入侧可选项 */
  const toAccountOptions = accountOptionsFor('to')

  /** 方向切换：反向方向的既有 Pair 交换两端即贴合新方向（借出↔收回/借入↔还款，不丢
   * 已选账户）；其余情况保留仍属允许侧别的选择、清掉越侧的，不存在中间态残留。 */
  function setDirection(next: LendingFormDirection) {
    if (next === direction.value) return
    const { from, to } = LENDING_DIRECTION_SIDES[next]
    const fromId = transfer.accountId.value
    const toId = transfer.toAccountId.value
    if (
      fromId != null &&
      toId != null &&
      accountMatchesSide(accountTypeOf(toId), from) &&
      accountMatchesSide(accountTypeOf(fromId), to)
    ) {
      transfer.accountId.value = toId
      transfer.toAccountId.value = fromId
    } else {
      if (fromId != null && !accountMatchesSide(accountTypeOf(fromId), from)) {
        transfer.accountId.value = null
      }
      if (toId != null && !accountMatchesSide(accountTypeOf(toId), to)) {
        transfer.toAccountId.value = null
      }
    }
    direction.value = next
  }

  return {
    ...transfer,
    direction,
    fromAccountOptions,
    toAccountOptions,
    setDirection,
  }
}
