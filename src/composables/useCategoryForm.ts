import { computed, ref } from 'vue'
import { useMessage } from 'naive-ui'
import type { TreeSelectOption } from 'naive-ui'
import { api } from '@/api'
import { centsToYuan } from '@/types'
import { buildExpenseIncomeInput } from '@/domain/transaction-input'
import { useReferenceStore } from '@/stores/reference'
import { useFormShared, utcMidnightTimestamp } from '@/composables/useFormShared'
import type { Transaction } from '@/types'
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
  const merchantRef = ref<string | null>(null)
  const note = ref('')
  const date = ref(Date.now())

  // 编辑模式（issue #178）：打开即回填该笔交易全部业务字段（编辑态声明提前：
  // 下方的商户选项/回填均依赖 editingTx）。金额经 centsToYuan 按币种小数位
  // 换算（不手写 /100）；日期以 UTC 午夜时间戳承载回填，提交端日期转换
  // 收口装配器 toLocalDateISO（issue #216）。
  const editingTx = options?.editing?.() ?? null

  const treeOptions = computed<TreeSelectOption[]>(() => reference.treeCategoryOptions(kind) as unknown as TreeSelectOption[])

  // 商户下拉选项（issue #189）：在用商户；编辑时若原商户已不在字典（软删且超出
  // 会话显示缓存），追加兜底选项承载原 id——裸 uuid 不可读，提交时按「未改动」
  // 语义原样保留（后端 existing_merchant_id unchanged 语义跳过校验）。
  const editingMerchantId = editingTx?.merchant_id ?? null
  const merchantOptions = computed<{ label: string; value: string }[]>(() => {
    const base = reference.merchants.map((m) => ({ label: m.name, value: m.id }))
    if (editingMerchantId && !reference.merchantMap.has(editingMerchantId)) {
      base.unshift({ label: '（已删除商户）', value: editingMerchantId })
    }
    return base
  })

  /**
   * 商户解析（保存时单点收口，issue #189）：「输入即建」交互——
   * 1. 空 → null（无商户）；
   * 2. 选中已有商户（value 为 id，含编辑回填的软删商户 id，会话缓存内可见）→ 原样携带；
   * 3. 编辑未改动原商户（软删且超出缓存）→ 原样携带（后端 unchanged 语义跳过校验）；
   * 4. 输入文本精确命中在用商户名 → 按名复用；
   * 5. 未命中 → `create_merchant` 即建；重名错误（store 陈旧竞态）先强制重拉
   *    按名复用，仍失败才向上抛。
   */
  async function resolveMerchantId(): Promise<string | null> {
    const selected = merchantRef.value
    if (!selected) return null
    if (reference.merchantMap.has(selected)) return selected
    if (editingMerchantId && selected === editingMerchantId) return selected
    const name = selected.trim()
    if (!name) return null
    const existing = reference.merchantByName.get(name)
    if (existing) return existing.id
    try {
      return await api.createMerchant({ name })
    } catch (e) {
      // 重名兑底（store 陈旧竞态）：强制重拉后按名复用；重拉失败不影响原错误上抛
      try {
        await reference.refresh()
      } catch {
        /* 保留原 create 错误 */
      }
      const retry = reference.merchantByName.get(name)
      if (retry) return retry.id
      throw e
    }
  }

  if (editingTx) {
    amount.value = centsToYuan(editingTx.amount_cents, reference.getCurrency(editingTx.currency_code))
    currencyCode.value = editingTx.currency_code
    accountId.value = editingTx.account_id
    categoryId.value = editingTx.category_id
    merchantRef.value = editingTx.merchant_id
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
    // 商户解析留表单层（异步 + 即建/重拉副作用，issue #189）：装配器收已解析的 id
    const merchantId = await resolveMerchantId()
    // 编辑目标提交时重读（getter 约定见 options.editing 注释）
    const editing = options?.editing?.() ?? null
    try {
      // wire 字段拼装收口 TransactionInput 装配器（issue #216）：创建/编辑共用
      // 同一装配结果（UpdateTransactionInput 与 TransactionInput 字段同构，
      // 幂等键不可编辑）；金额元转分与本地日期转换均为装配器实现细节
      const input = buildExpenseIncomeInput({
        kind,
        amount: amount.value,
        currencyCode: currencyCode.value,
        accountId: accountId.value,
        categoryId: categoryId.value,
        merchantId,
        note: note.value,
        date: date.value,
      })
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
    merchantRef.value = null
    note.value = ''
    date.value = Date.now()
  }

  return {
    amount, currencyCode, accountId, categoryId, merchantRef, note, date,
    accountOptions, currencyOptions, treeOptions, merchantOptions,
    submit, resetForm,
  }
}
