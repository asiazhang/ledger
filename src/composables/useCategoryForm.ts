import { computed, ref } from 'vue'
import { useMessage } from 'naive-ui'
import type { TreeSelectOption } from 'naive-ui'
import { api } from '@/api'
import { centsToYuan } from '@/types'
import { useReferenceStore } from '@/stores/reference'
import { useFormShared, utcMidnightTimestamp } from '@/composables/useFormShared'
import type { Transaction, UpdateTransactionInput } from '@/types'
import { errorMessage } from "@/utils/errors";

export function useCategoryForm(
  kind: 'expense' | 'income',
  options?: {
    onCreated?: () => void
    /** 编辑模式（issue #178）：更新成功回调。编辑路径与创建路径共用 submit，
     * 按是否存在 editing 目标分派命令；成功后不重置表单（弹窗由父层关闭）。 */
    onUpdated?: () => void
    /** 编辑模式：待编辑交易 getter。与 useRefundForm fixedTarget 同约定：仅在
     * composable 创建时读一次做回填、提交时重读一次定目标，换目标交易必须由
     * 父层强制重建组件实例（:key 序号重建），否则回填/提交仍指向旧交易。 */
    editing?: () => Transaction | null
  },
) {
  const { accountOptions, currencyOptions } = useFormShared()
  const reference = useReferenceStore()
  const message = useMessage()

  const amount = ref<number | null>(null)
  const currencyCode = ref('CNY')
  const accountId = ref<string | null>(null)
  const categoryId = ref<string | null>(null)
  const note = ref('')
  const date = ref(Date.now())

  const treeOptions = computed<TreeSelectOption[]>(() => reference.treeCategoryOptions(kind) as unknown as TreeSelectOption[])

  // 编辑模式（issue #178）：打开即回填该笔交易全部业务字段。金额经 centsToYuan
  // 按币种小数位换算（不手写 /100）；日期以 UTC 午夜回填，与提交端
  // toISOString 切片同一口径，不改往返无损。
  const editingTx = options?.editing?.() ?? null
  if (editingTx) {
    amount.value = centsToYuan(editingTx.amount_cents, reference.getCurrency(editingTx.currency_code))
    currencyCode.value = editingTx.currency_code
    accountId.value = editingTx.account_id
    categoryId.value = editingTx.category_id
    note.value = editingTx.note ?? ''
    date.value = utcMidnightTimestamp(editingTx.date)
  }

  async function submit() {
    if (!accountId.value) {
      message.warning('请选择账户')
      return
    }
    if (amount.value == null || amount.value <= 0) {
      message.warning('请输入金额')
      return
    }
    // 同一入参对象形状（issue #178）：创建/编辑共用 UpdateTransactionInput 形状
    // （幂等键不可编辑，TransactionInput 的其余字段均被覆盖）
    const input: UpdateTransactionInput = {
      kind,
      amount_cents: Math.round(amount.value * 100),
      currency_code: currencyCode.value,
      account_id: accountId.value,
      category_id: categoryId.value,
      note: note.value || null,
      date: new Date(date.value).toISOString().slice(0, 10),
    }
    // 编辑目标提交时重读（getter 约定见 options.editing 注释）
    const editing = options?.editing?.() ?? null
    try {
      if (editing) {
        await api.updateTransaction(editing.id, input)
        message.success('已保存修改')
        // 编辑路径不重置表单：成功即关窗（onUpdated），实例整体销毁
        options?.onUpdated?.()
      } else {
        await api.createTransaction(input)
        message.success(kind === 'expense' ? '已记支出' : '已记收入')
        amount.value = null
        note.value = ''
        options?.onCreated?.()
      }
    } catch (e) {
      message.error(editing ? `保存失败: ${errorMessage(e)}` : `记账失败: ${errorMessage(e)}`)
    }
  }

  function resetForm() {
    amount.value = null
    currencyCode.value = 'CNY'
    accountId.value = null
    categoryId.value = null
    note.value = ''
    date.value = Date.now()
  }

  return {
    amount, currencyCode, accountId, categoryId, note, date,
    accountOptions, currencyOptions, treeOptions,
    submit, resetForm,
  }
}
