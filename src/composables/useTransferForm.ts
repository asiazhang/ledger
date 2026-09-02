import { computed, ref } from 'vue'
import { useMessage } from 'naive-ui'
import { api } from '@/api'
import { centsToYuan } from '@/types'
import { buildTransferInput } from '@/domain/transaction-input'
import { judgeAmountText, fieldErrorKind } from '@/utils/field-error'
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
  /** 创建成功提示（可选 getter，缺省用转账通用文案）：借贷变体按提交时的当前
   * 方向给专属文案（issue #374），故取 getter 而非静态串。 */
  createdMessage?: () => string
}) {
  const { reference, accountOptions, currencyOptions } = useFormShared()
  const message = useMessage()

  // 金额字段错误态（ADR-0058 / issue #415）：同支出/收入形态（#414 先例）——金额以
  // 原始文本承载输入（不拦截、不静默丢弃，非法文本原样保留），判定口径走共享单点
  // judgeAmountText；错误态装配（输入中即时红 / 空值红在失焦或保存尝试后）由本薄层
  // 声明时机。借贷变体（useLendingForm）展开复用同一状态，红态行为随接缝天然一致。
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
  const toAccountId = ref<string | null>(null)
  const note = ref('')
  const date = ref(Date.now())

  // 编辑模式（issue #178）：打开即回填该笔交易全部业务字段。日期以 UTC 午夜
  // 时间戳承载回填，提交端日期转换收口装配器 toLocalDateISO（issue #216）。
  const editingTx = options?.editing?.() ?? null
  if (editingTx) {
    // 金额回填：分 → 元（不手写 /100）后以文本形态回填；整数分的合法回填至多
    // 两位小数（币种小数位 ≤ 2），判定必为 ok，不显红态
    amountText.value = String(centsToYuan(editingTx.amount_cents, reference.getCurrency(editingTx.currency_code)))
    currencyCode.value = editingTx.currency_code
    accountId.value = editingTx.account_id
    toAccountId.value = editingTx.to_account_id
    note.value = editingTx.note ?? ''
    date.value = utcMidnightTimestamp(editingTx.date)
  }

  async function submit() {
    // 保存尝试即触发空值兜底红态（fieldErrorKind 的 saveAttempted 输入）
    saveAttempted.value = true
    // 格式类错误（解析失败 / 超精度 / 必填为空）由「红框＋提交禁用」取代旧格式
    // toast（ADR-0058 决策 1/3）：错误态下静默中止提交（先于账户 toast：红态是
    // 本次点击的全部反馈，账户提示延后到格式修正后的下次尝试），红框已在字段上呈现
    if (amountError.value != null) return
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
    const judgment = amountJudgment.value
    if (judgment.kind !== 'ok') return // 不可达（错误态已被上方守卫拦截），仅为类型收窄
    // 业务类校验（纯零/负数）保留既有提交 toast 通道，不动（ADR-0058：业务不成立不属字段错误态）
    if (judgment.yuan <= 0) {
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
        amount: judgment.yuan,
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
        message.success(options?.createdMessage?.() ?? t('transactions.form.transferCreated'))
        amountText.value = ''
        // 时机标志同清：弹窗关窗销毁实例前不留潜伏红态（初始为空不红，ADR-0058 决策 2）
        amountBlurred.value = false
        saveAttempted.value = false
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
    amountText.value = ''
    amountBlurred.value = false
    saveAttempted.value = false
    currencyCode.value = 'CNY'
    accountId.value = null
    toAccountId.value = null
    note.value = ''
    date.value = Date.now()
  }

  return {
    amountText, markAmountBlurred, amountError, hasFieldError,
    currencyCode, accountId, toAccountId, note, date,
    accountOptions, currencyOptions,
    submit, resetForm,
  }
}
