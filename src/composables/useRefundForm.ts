import { computed, ref } from 'vue'
import { useMessage } from 'naive-ui'
import { api } from '@/api'
import { centsToYuan, formatAmount } from '@/types'
import { buildRefundInput } from '@/domain/transaction-input'
import { judgeAmountText, fieldErrorKind } from '@/utils/field-error'
import { useFormShared } from '@/composables/useFormShared'
import { t } from '@/i18n'
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

  // 金额字段错误态（ADR-0058 / issue #415）：同支出/收入形态（#414 先例）与转账形态
  // ——金额以原始文本承载输入（不拦截、不静默丢弃，非法文本原样保留），判定口径走
  // 共享单点 judgeAmountText；错误态装配（输入中即时红 / 空值红在失焦或保存尝试后）
  // 由本薄层声明时机。
  const amountText = ref('')
  const amountBlurred = ref(false)
  const saveAttempted = ref(false)
  const amountJudgment = computed(() => judgeAmountText(amountText.value))
  const amountError = computed(() =>
    fieldErrorKind(amountJudgment.value, {
      touched: amountBlurred.value,
      saveAttempted: saveAttempted.value,
    }),
  )
  /** 任一字段处于错误态（本期仅金额），保存按钮随之禁用 */
  const hasFieldError = computed(() => amountError.value != null)

  /** 金额失焦：空值红时机输入（touched） */
  function markAmountBlurred() {
    amountBlurred.value = true
  }

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
    // 金额回填：分 → 元（不手写 /100）后以文本形态回填；整数分的合法回填至多
    // 两位小数（币种小数位 ≤ 2），判定必为 ok，不显红态
    amountText.value = String(centsToYuan(fixedTx.amount_cents, reference.getCurrency(fixedTx.currency_code)))
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
    // 保存尝试即触发空值兜底红态（fieldErrorKind 的 saveAttempted 输入）
    saveAttempted.value = true
    // 格式类错误（解析失败 / 超精度 / 必填为空）由「红框＋提交禁用」取代旧格式
    // toast（ADR-0058 决策 1/3）：错误态下静默中止提交（先于关联交易 toast，
    // 同转账形态守卫序），红框已在字段上呈现
    if (amountError.value != null) return
    // 行内模式原交易固定；搜索模式取下拉选择
    const targetId = fixedTarget?.()?.id ?? refundTargetId.value
    if (!targetId) {
      message.warning(t('transactions.refund.warnNoTarget'))
      return
    }
    const judgment = amountJudgment.value
    if (judgment.kind !== 'ok') return // 不可达（错误态已被上方守卫拦截），仅为类型收窄
    // 业务类校验（纯零/负数）保留既有提交 toast 通道，不动（ADR-0058：业务不成立不属字段错误态）
    if (judgment.yuan <= 0) {
      message.warning(t('transactions.refund.warnNoAmount'))
      return
    }
    try {
      // wire 字段拼装收口 TransactionInput 装配器（issue #216）。账户随原交易：
      // 后端强制继承原支出账户/币种，行内模式打开即回填账户，搜索模式取所选
      // 原交易的账户（表单账户 ref 无独立录入入口）；装配器对缺失 fail fast 兜底
      const input = buildRefundInput({
        amount: judgment.yuan,
        currencyCode: currencyCode.value,
        accountId: refundTarget.value?.account_id ?? accountId.value,
        refundOfTransactionId: targetId,
        note: note.value,
        date: date.value,
      })
      await api.createTransaction(input)
      message.success(t('transactions.refund.created'))
      // 行内模式跳过全量交易重载：列表刷新由 onCreated 回调承担
      if (!fixedTarget) await loadTransactions()
      amountText.value = ''
      // 时机标志同清：弹窗关窗销毁实例前不留潜伏红态（初始为空不红，ADR-0058 决策 2）
      amountBlurred.value = false
      saveAttempted.value = false
      note.value = ''
      refundTargetId.value = null
      options?.onCreated?.()
    } catch (e) {
      message.error(t('transactions.refund.failed', { msg: errorMessage(e) }))
    }
  }

  function resetForm() {
    amountText.value = ''
    amountBlurred.value = false
    saveAttempted.value = false
    currencyCode.value = 'CNY'
    accountId.value = null
    refundTargetId.value = null
    note.value = ''
    date.value = Date.now()
  }

  return {
    amountText, markAmountBlurred, amountError, hasFieldError,
    currencyCode, accountId, refundTargetId, note, date,
    accountOptions, currencyOptions,
    expenseTransactions, refundTargetOptions, refundTarget,
    submit, resetForm,
  }
}
