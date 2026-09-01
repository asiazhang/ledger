import { ref } from 'vue'
import { useMessage } from 'naive-ui'
import { api } from '@/api'
import { centsToYuan } from '@/types'
import { buildTransferInput } from '@/domain/transaction-input'
import { useFormShared, utcMidnightTimestamp } from '@/composables/useFormShared'
import { t } from '@/i18n'
import type { Transaction } from '@/types'
import { errorMessage } from "@/utils/errors";

export function useTransferForm(options?: {
  onCreated?: () => void
  /** 编辑模式（issue #178）：更新成功回调。编辑路径与创建路径共用 submit，
   * 按是否存在 editing 目标分派命令；成功后不重置表单（弹窗由父层关闭）。 */
  onUpdated?: () => void
  /** 编辑模式：待编辑交易 getter。与 useRefundForm fixedTarget 同约定：仅在
   * composable 创建时读一次做回填、提交时重读一次定目标，换目标交易必须由
   * 父层强制重建组件实例（:key 序号重建），否则回填/提交仍指向旧交易。 */
  editing?: () => Transaction | null
}) {
  const { reference, accountOptions, currencyOptions } = useFormShared()
  const message = useMessage()

  const amount = ref<number | null>(null)
  const currencyCode = ref('CNY')
  const accountId = ref<string | null>(null)
  const toAccountId = ref<string | null>(null)
  const note = ref('')
  const date = ref(Date.now())

  // 编辑模式（issue #178）：打开即回填该笔交易全部业务字段。金额经 centsToYuan
  // 按币种小数位换算（不手写 /100）；日期以 UTC 午夜时间戳承载回填，提交端
  // 日期转换收口装配器 toLocalDateISO（issue #216）。
  const editingTx = options?.editing?.() ?? null
  if (editingTx) {
    amount.value = centsToYuan(editingTx.amount_cents, reference.getCurrency(editingTx.currency_code))
    currencyCode.value = editingTx.currency_code
    accountId.value = editingTx.account_id
    toAccountId.value = editingTx.to_account_id
    note.value = editingTx.note ?? ''
    date.value = utcMidnightTimestamp(editingTx.date)
  }

  async function submit() {
    if (!accountId.value) {
      message.warning(t('transactions.form.warnSelectFromAccount'))
      return
    }
    if (!toAccountId.value) {
      message.warning(t('transactions.form.warnSelectToAccount'))
      return
    }
    if (accountId.value === toAccountId.value) {
      message.warning(t('transactions.form.warnSameAccount'))
      return
    }
    if (amount.value == null || amount.value <= 0) {
      message.warning(t('transactions.form.warnAmount'))
      return
    }
    // 编辑目标提交时重读（getter 约定见 options.editing 注释）
    const editing = options?.editing?.() ?? null
    try {
      // wire 字段拼装收口 TransactionInput 装配器（issue #216）：创建/编辑共用
      // 同一装配结果（UpdateTransactionInput 与 TransactionInput 字段同构，
      // 幂等键不可编辑）；金额元转分与本地日期转换均为装配器实现细节
      const input = buildTransferInput({
        amount: amount.value,
        currencyCode: currencyCode.value,
        accountId: accountId.value,
        toAccountId: toAccountId.value,
        note: note.value,
        date: date.value,
      })
      if (editing) {
        await api.updateTransaction(editing.id, input)
        message.success(t('transactions.form.changesSaved'))
        // 编辑路径不重置表单：成功即关窗（onUpdated），实例整体销毁
        options?.onUpdated?.()
      } else {
        await api.createTransaction(input)
        message.success(t('transactions.form.transferCreated'))
        amount.value = null
        note.value = ''
        options?.onCreated?.()
      }
    } catch (e) {
      message.error(
        editing
          ? t('transactions.form.saveFailed', { msg: errorMessage(e) })
          : t('transactions.form.createFailed', { msg: errorMessage(e) }),
      )
    }
  }

  function resetForm() {
    amount.value = null
    currencyCode.value = 'CNY'
    accountId.value = null
    toAccountId.value = null
    note.value = ''
    date.value = Date.now()
  }

  return {
    amount, currencyCode, accountId, toAccountId, note, date,
    accountOptions, currencyOptions,
    submit, resetForm,
  }
}
