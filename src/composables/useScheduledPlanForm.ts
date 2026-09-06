import { computed, ref } from 'vue'
import type { TreeSelectOption } from 'naive-ui'
import { useMessage } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import { useAppStore } from '@/stores/app'
import { useFormShared } from '@/composables/useFormShared'
import { resolveMerchantRef } from '@/composables/resolve-merchant'
import { api } from '@/api'
import { errorMessage } from '@/utils/errors'
import { t } from '@/i18n'
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

/** submitCreate 提交编排入参（spec #520）：表单校验与形态特化字段组装留页签，
 * 页签传入形态与已校验的金额/特化字段；商户解析由接缝按形态显式决定（无商户面形态跳过）。 */
export interface ScheduledPlanSubmitSpec {
  kind: ScheduledKind
  /** 每期金额（分）：页签已校验（>0、元转分），分期为 floor 均分口径 */
  amountCents: number
  /** 形态特化字段（页签组装，如分期总额/期数、转账转入账户） */
  specific?: ScheduledPlanSpecificFields
}

/** 提交成功后回调（工厂入参）：适配器注入各自原子动作——转账/分期：关窗 +
 * 特化字段重置 + 清单刷新；订阅追加订阅花费刷新。回调时公共草稿已由接缝重置。 */
export type SubmitSuccessCallback = () => void | Promise<void>

/** useScheduledPlanForm 工厂入参。 */
export interface UseScheduledPlanFormOptions {
  /** 提交成功后回调（提交时序编排的最后一步，spec #520） */
  onSubmitted?: SubmitSuccessCallback
}

/**
 * ScheduledPlanForm 计划表单接缝（ADR-0041，计划域私有，仅三个计划页签消费）：
 * 三种新建表单的公共部分单点——公共草稿字段（备注/账户/分类/商户/币种/周期三字段/
 * 开始日）、表单重置、商户解析（「输入即建 + 重名兜底强制重拉按名复用」整套竞态处理；
 * 编辑路径以「编辑中商户 id」参数保留软删兜底分支）与公共 payload 组装（
 * CreateScheduledInput 公共字段部分，新增公共字段只改这里一处）。
 *
 * spec #520 起的提交时序编排（submitCreate）：商户解析（无商户面形态显式跳过）→
 * 公共 payload 组装合并形态特化字段 → 创建命令 → 成功/失败提示 → 公共草稿重置 →
 * 提交成功后回调；表单校验与形态特化字段组装留在各页签。
 *
 * 工厂形态：每次调用返回独立实例（新建弹窗与编辑弹窗各持一份草稿，互不串扰）。
 * 跨域表单共享接缝 useFormShared 在此内部消费，计划页签不再直连（消费面自然收缩）。
 */
export function useScheduledPlanForm(options: UseScheduledPlanFormOptions = {}) {
  const { onSubmitted } = options
  const reference = useReferenceStore()
  const appStore = useAppStore()
  const { accountOptions, currencyOptions } = useFormShared()
  const message = useMessage()

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
   * 商户解析（保存时单点收口，issue #190/#206）：「输入即建 + 重名兜底」交互
   * 收口在共享接缝 `resolveMerchantRef`（保单表单同款消费，issue #360）——
   * 本地仅读草稿字段，解析细则见该接缝注释。
   */
  async function resolveMerchant(editingMerchantId: string | null = null): Promise<string | null> {
    return resolveMerchantRef(merchantRef.value, editingMerchantId)
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
      ...spec.specific,
    }
  }

  /**
   * 成功提示文案键按形态单源持有（spec #520）：逐字维持三个既有文案键。
   * 只作专名出现，不复述文案值（文案见 i18n scheduled.json toasts）。
   */
  const CREATE_SUCCESS_KEY: Record<ScheduledKind, string> = {
    subscription: 'scheduled.toast.subscriptionCreated',
    installment: 'scheduled.toast.installmentCreated',
    scheduled_transfer: 'scheduled.toast.transferCreated',
  }

  /**
   * 新建提交流程编排（spec #520）：
   *   商户解析（scheduled_transfer 形态无商户面，显式跳过；其余形态走 resolveMerchant）
   *   → 公共 payload 组装合并形态特化字段 → 创建命令 → 成功/失败提示
   *   → 公共草稿重置 → 提交成功后回调。
   * 表单校验与形态特化字段组装留页签；创建调用维持现有 try/catch 形态（不迁入 Loadable）。
   * 成功时 resolve；失败时只发错误提示并结束（不重置草稿、不触发回调——弹窗保持打开）。
   */
  async function submitCreate(spec: ScheduledPlanSubmitSpec): Promise<void> {
    try {
      const merchantId =
        spec.kind === 'scheduled_transfer' ? null : await resolveMerchant()
      await api.createScheduledTransaction(
        buildCreateInput({
          kind: spec.kind,
          amountCents: spec.amountCents,
          merchantId,
          specific: spec.specific,
        }),
      )
      message.success(t(CREATE_SUCCESS_KEY[spec.kind]))
      reset()
      await onSubmitted?.()
    } catch (e) {
      message.error(t('scheduled.toast.createFailed', { message: errorMessage(e) }))
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
    submitCreate,
  }
}
