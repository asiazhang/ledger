import { computed, ref } from 'vue'
import { useMessage } from 'naive-ui'
import { api } from '@/api'
import { centsToYuan, formatAmount } from '@/types'
import { buildRefundInput } from '@/domain/transaction-input'
import { useFormShared } from '@/composables/useFormShared'
import type { Transaction } from '@/types'
import { errorMessage } from "@/utils/errors";

export function useRefundForm(options?: {
  onCreated?: () => void
  /** 行内退款（issue #151）：原交易由调用方所在行固定给定。注意：getter 仅在
   * composable 创建时读取一次做初始化、提交时重读一次，并非响应式依赖——
   * 换目标交易必须由父层强制重建组件实例（如 TransactionsView 经 TransactionModalState
   * 的回调序号作 :key="modalSeq"），否则展示/提交仍指向旧交易。打开即锁定继承的账户/币种展示，金额默认原交易金额
   * （原始币种）；提交跳过全量交易重载（列表刷新由 onCreated 回调承担）。 */
  fixedTarget?: () => Transaction | null
}) {
  const { reference, accountOptions, currencyOptions } = useFormShared()
  const message = useMessage()

  const amount = ref<number | null>(null)
  const currencyCode = ref('CNY')
  const accountId = ref<string | null>(null)
  const refundTargetId = ref<string | null>(null)
  const note = ref('')
  const date = ref(Date.now())

  // 行内退款模式：原交易由所在行固定，打开即按原交易初始化金额与锁定字段展示
  // （账户/币种/分类后端强制继承原支出，此处仅为展示；amount_cents 按原币种解释）
  const fixedTarget = options?.fixedTarget
  const fixedTx = fixedTarget?.() ?? null
  if (fixedTx) {
    amount.value = centsToYuan(fixedTx.amount_cents, reference.getCurrency(fixedTx.currency_code))
    currencyCode.value = fixedTx.currency_code
    accountId.value = fixedTx.account_id
  }

  const transactions = ref<Transaction[]>([])

  const expenseTransactions = computed(() =>
    transactions.value.filter((t) => t.kind === 'expense'),
  )

  const refundTargetOptions = computed(() =>
    expenseTransactions.value.map((t) => {
      const cat = reference.categoryPath(t.category_id) || '-'
      const cur = reference.getCurrency(t.currency_code)
      const amt = formatAmount(t.amount_native_cents, cur)
      const noteStr = t.note ? ` · ${t.note}` : ''
      return {
        label: `${t.date}  ${amt}  ${cat}${noteStr}`,
        value: t.id,
      }
    }),
  )

  const refundTarget = computed<Transaction | null>(() => {
    const fixed = fixedTarget?.()
    if (fixed) return fixed
    return refundTargetId.value == null
      ? null
      : (expenseTransactions.value.find((t) => t.id === refundTargetId.value) ?? null)
  })

  async function loadTransactions() {
    try {
      transactions.value = (await api.listTransactions()).items
    } catch {
      // 加载失败忽略，退款关联可后续重试
    }
  }

  async function submit() {
    // 行内模式原交易固定；搜索模式取下拉选择
    const targetId = fixedTarget?.()?.id ?? refundTargetId.value
    if (!targetId) {
      message.warning('请选择要退款的原始支出交易')
      return
    }
    if (amount.value == null || amount.value <= 0) {
      message.warning('请输入退款金额')
      return
    }
    try {
      // wire 字段拼装收口 TransactionInput 装配器（issue #216）。账户随原交易：
      // 后端强制继承原支出账户/币种，行内模式打开即回填账户，搜索模式取所选
      // 原交易的账户（表单账户 ref 无独立录入入口）；装配器对缺失 fail fast 兜底
      const input = buildRefundInput({
        amount: amount.value,
        currencyCode: currencyCode.value,
        accountId: refundTarget.value?.account_id ?? accountId.value,
        refundOfTransactionId: targetId,
        note: note.value,
        date: date.value,
      })
      await api.createTransaction(input)
      message.success('已记退款')
      // 行内模式跳过全量交易重载：列表刷新由 onCreated 回调承担
      if (!fixedTarget) await loadTransactions()
      amount.value = null
      note.value = ''
      refundTargetId.value = null
      options?.onCreated?.()
    } catch (e) {
      message.error(`退款失败: ${errorMessage(e)}`)
    }
  }

  function resetForm() {
    amount.value = null
    currencyCode.value = 'CNY'
    accountId.value = null
    refundTargetId.value = null
    note.value = ''
    date.value = Date.now()
  }

  return {
    amount, currencyCode, accountId, refundTargetId, note, date,
    accountOptions, currencyOptions,
    expenseTransactions, refundTargetOptions, refundTarget,
    submit, resetForm,
  }
}
