import { computed, ref } from 'vue'
import type { TreeSelectOption } from 'naive-ui'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { useAppStore } from '@/stores/app'
import { useFormShared } from '@/composables/useFormShared'
import { todayStr } from '@/utils/date'
import type { CreateScheduledInput, RecurrenceType, ScheduledKind } from '@/types'

/** 形态特化字段（ADR-0041：分期总额/期数、转账转入账户与总期数留页签）：
 * 仅携带该形态真实发送的键——组装结果键集与既有三表单逐字一致，不补空键。 */
export interface ScheduledPlanSpecificFields {
  total_amount_cents?: number | null
  total_occurrences?: number | null
  to_account_id?: string | null
}

/** 公共 payload 组装入参：商户 id 须先经 resolveMerchant 解析（无商户面的形态传 null）。 */
export interface ScheduledPlanCreateSpec {
  kind: ScheduledKind
  /** 每期金额（分）：分期为 floor 均分口径，订阅/转账为每期金额 */
  amountCents: number
  merchantId: string | null
  /** 形态特化字段（页签组装） */
  specific?: ScheduledPlanSpecificFields
}

/**
 * ScheduledPlanForm 计划表单接缝（ADR-0041，计划域私有，仅三个计划页签消费）：
 * 三种新建表单的公共部分单点——公共草稿字段（备注/账户/分类/商户/币种/周期三字段/
 * 开始日）、表单重置、商户解析（「输入即建 + 重名兜底强制重拉按名复用」整套竞态处理；
 * 编辑路径以「编辑中商户 id」参数保留软删兜底分支）与公共 payload 组装（
 * CreateScheduledInput 公共字段部分，新增公共字段只改这里一处）。
 *
 * 工厂形态：每次调用返回独立实例（新建弹窗与编辑弹窗各持一份草稿，互不串扰）。
 * 表单校验与形态特化字段组装（分期总额/期数、转账转入账户、币种跟随）留在各页签；
 * 跨域表单共享接缝 useFormShared 在此内部消费，计划页签不再直连（消费面自然收缩）。
 */
export function useScheduledPlanForm() {
  const reference = useReferenceStore()
  const appStore = useAppStore()
  const { accountOptions, currencyOptions } = useFormShared()

  // ---------------------------------------------------------------------------
  // 公共草稿字段：初始态与 reset() 复位终态共用同一来源语义（模态语义下每次
  // 打开应是全新表单）。周期三字段含 recurrence_day（「每月几号」三形态皆未
  // 暴露 UI，恒为 null）——新增公共字段先落此处，三种形态一次生效。
  // ---------------------------------------------------------------------------

  const note = ref('')
  const accountId = ref<string | null>(null)
  const categoryId = ref<string | null>(null)
  const merchantRef = ref<string | null>(null)
  const currencyCode = ref(appStore.defaultCurrency)
  const recurrenceType = ref<RecurrenceType>('monthly')
  const recurrenceInterval = ref(1)
  const recurrenceDay = ref<number | null>(null)
  const startDate = ref(todayStr())

  /** 重置草稿到初始态。形态特化字段（金额/期数/转入账户等）由页签自行复位。 */
  function reset() {
    note.value = ''
    accountId.value = null
    categoryId.value = null
    merchantRef.value = null
    currencyCode.value = appStore.defaultCurrency
    recurrenceType.value = 'monthly'
    recurrenceInterval.value = 1
    recurrenceDay.value = null
    startDate.value = todayStr()
  }

  // ---------------------------------------------------------------------------
  // 选项面：账户/币种经跨域共享接缝，分类树（分期/订阅扣款为支出）与在用商户
  // 为计划表单口径，全仓一份。
  // ---------------------------------------------------------------------------

  const categoryTreeOptions = computed(
    () => reference.treeCategoryOptions('expense') as unknown as TreeSelectOption[],
  )

  const merchantOptions = computed<{ label: string; value: string }[]>(() =>
    reference.merchants.map((m) => ({ label: m.name, value: m.id })),
  )

  /**
   * 商户解析（保存时单点收口，issue #190/#206）：「输入即建」交互——
   * 1. 空 → null（无商户）；
   * 2. 选中已有商户（value 为 id）→ 原样携带；
   * 3. 编辑未改动原商户（软删且超出会话缓存）→ 原样携带（后端保持历史引用语义），
   *    仅当 `editingMerchantId` 传入时生效（订阅编辑路径专用）；
   * 4. 输入文本精确命中在用商户名 → 按名复用；
   * 5. 未命中 → `create_merchant` 即建；重名错误（store 陈旧竞态）先强制重拉
   *    按名复用，仍失败才向上抛。
   */
  async function resolveMerchant(editingMerchantId: string | null = null): Promise<string | null> {
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
      // 重名兜底（store 陈旧竞态）：强制重拉后按名复用；重拉失败不影响原错误上抛
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

  /**
   * 公共 payload 组装（CreateScheduledInput 公共字段部分）：账户/分类/商户/金额/
   * 币种/周期三字段/开始日/备注（trim，空 → null）单源；形态特化键仅透传
   * `spec.specific` 给出的键。账户校验留页签——调用前 accountId 必已选定。
   */
  function buildCreateInput(spec: ScheduledPlanCreateSpec): CreateScheduledInput {
    return {
      kind: spec.kind,
      account_id: accountId.value!,
      category_id: categoryId.value,
      merchant_id: spec.merchantId,
      amount_cents: spec.amountCents,
      currency_code: currencyCode.value,
      recurrence_type: recurrenceType.value,
      recurrence_interval: recurrenceInterval.value,
      recurrence_day: recurrenceDay.value,
      start_date: startDate.value,
      note: note.value.trim() || null,
      ...(spec.specific ?? {}),
    }
  }

  return {
    // 公共草稿字段
    note,
    accountId,
    categoryId,
    merchantRef,
    currencyCode,
    recurrenceType,
    recurrenceInterval,
    recurrenceDay,
    startDate,
    // 选项面
    accountOptions,
    currencyOptions,
    categoryTreeOptions,
    merchantOptions,
    // 动作
    reset,
    resolveMerchant,
    buildCreateInput,
  }
}
