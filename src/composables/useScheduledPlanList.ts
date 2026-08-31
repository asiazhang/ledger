import { computed, readonly, ref } from 'vue'
import type { ComputedRef, Ref } from 'vue'
import { useMessage } from 'naive-ui'
import { api } from '@/api'
import { errorMessage } from '@/utils/errors'
import { t } from '@/i18n'
import { scheduledStatusLabel } from '@/utils/scheduled'
import type {
  RecurrenceType,
  ScheduledKind,
  ScheduledStatus,
  ScheduledTransactionDetail,
  ScheduledTransactionOccurrence,
  ScheduledTransactionWithExt,
} from '@/types'

/**
 * ScheduledPlanList 计划清单深模块（ADR-0041，词汇表「ScheduledPlanList（计划清单）」）：
 * 定时计划三业务形态（分期/订阅/定时转账）共享的清单编排——以计划形态为参数，
 * 内化清单加载与刷新、状态过滤、Plan Lifecycle 操作（暂停/恢复/取消，含成功/失败提示）、
 * 行操作描述符构建与周期选项/周期标签，产出可观察的清单状态与动作；
 * 「定时」页三个页签是它的薄适配器，只留各自形态真差异。
 *
 * 纪律门槛（防上帝模块）：模块内形态分支只表达「能力有无」（如状态过滤选项集按形态
 * 有无「已完成」项）；形态特化计算体（期数预览、币种跟随、转入账户过滤）一律留适配器。
 * 弹层纯度（ADR-0035）：不引用任何组件、不接弹层注册表；行操作描述符由适配器渲染，
 * 确认弹层按弹层封装纪律留在适配器。
 *
 * 文案（ADR-0048）：标签/提示一律渲染或调用时经 t() 现取（选项表为工厂函数、
 * 注入文案为 getter），不在模块加载/工厂调用时定格，语言切换即时生效。
 */

// ---------------------------------------------------------------------------
// 周期选项表与周期标签（全仓单源）
// ---------------------------------------------------------------------------

/** 周期选项（标签现取）：三个页签新建表单下拉共用。 */
export interface ScheduledRecurrenceOption {
  label: string
  value: RecurrenceType
}

/**
 * 单源周期选项表工厂：三个页签新建表单下拉共用。
 * 「定时」转账页签下拉原为「天/周/月/年」，本单源落地即 #309 显式可见变化之一：
 * 统一为「每天/每周/每月/每年」（列表列口径三页签本就一致，不受影响）。
 * 每次调用现取标签（t()），随界面语言即时切换。
 */
export function scheduledRecurrenceOptions(): ScheduledRecurrenceOption[] {
  const labelKeys: Record<RecurrenceType, string> = {
    daily: 'scheduled.recurrence.daily',
    weekly: 'scheduled.recurrence.weekly',
    monthly: 'scheduled.recurrence.monthly',
    yearly: 'scheduled.recurrence.yearly',
  }
  return (Object.keys(labelKeys) as RecurrenceType[]).map((value) => ({
    value,
    label: t(labelKeys[value]),
  }))
}

const RECURRENCE_UNIT_KEY: Record<RecurrenceType, string> = {
  daily: 'scheduled.recurrence.unitDaily',
  weekly: 'scheduled.recurrence.unitWeekly',
  monthly: 'scheduled.recurrence.unitMonthly',
  yearly: 'scheduled.recurrence.unitYearly',
}

/**
 * 周期标签单源（清单「周期」列）：`interval = 1` →「每X」，否则「每N X」；
 * 未知类型兜底显示原值（与既有三 Pane 实现同形）。
 */
export function scheduledRecurrenceLabel(
  recurrenceType: RecurrenceType | string,
  interval: number,
): string {
  const unitKey = RECURRENCE_UNIT_KEY[recurrenceType as RecurrenceType]
  const unit = unitKey ? t(unitKey) : recurrenceType
  return interval > 1
    ? t('scheduled.recurrence.everyN', { n: interval, unit })
    : t('scheduled.recurrence.every', { unit })
}

// ---------------------------------------------------------------------------
// 行模型与行操作描述符
// ---------------------------------------------------------------------------

/** 一行 = 计划 + 形态扩展器产出的详情扩展。 */
export interface ScheduledPlanRow<E> {
  plan: ScheduledTransactionWithExt
  /** 详情命令失败：与「无数据」区分，不静默；渲染方式（下期「加载失败」/进度「加载失败」）由适配器决定。 */
  detailFailed: boolean
  /** 形态扩展：转账/订阅取最早 pending 期次，分期取完成期数/金额（由适配器扩展器定义）。 */
  ext: E
}

/** 行操作描述符：适配器据此渲染操作列（纯数据，无组件引用）。 */
export interface ScheduledPlanRowAction {
  /** 稳定键（适配器可作测试锚点前缀：op-detail / op-pause / op-resume / op-cancel）。 */
  key: 'detail' | 'pause' | 'resume' | 'cancel'
  label: string
  /** 按 Plan Lifecycle 状态的可用性（后端语义见 ADR-0024，模块只消费不重定义）。 */
  available: boolean
  /** 非空 = 该动作需二次确认，此文案交适配器渲染确认弹层（各形态文案不同）。 */
  confirm: string | null
  run(): void
}

/** 状态过滤选项（标签经全仓单源的 scheduledStatusLabel 词汇，不另造第二处映射）。 */
export interface ScheduledPlanStatusOption {
  key: ScheduledStatus
  label: string
}

/**
 * 按形态的状态过滤选项键集（能力有无）：定时转账支持「已完成」（一次性转账执行后
 * completed，issue 历史），订阅/分期暂无——维持各页签现状，不在此越权补齐。
 * 标签经全仓单源的 scheduledStatusLabel 生成，不另造第二处映射（ADR-0041 决策 4）。
 */
const STATUS_FILTER_KEYS: Record<ScheduledKind, ReadonlyArray<ScheduledStatus>> = {
  scheduled_transfer: ['active', 'paused', 'cancelled', 'completed'],
  subscription: ['active', 'paused', 'cancelled'],
  installment: ['active', 'paused', 'cancelled'],
}

function buildStatusFilterOptions(kind: ScheduledKind): ScheduledPlanStatusOption[] {
  return STATUS_FILTER_KEYS[kind].map((key) => ({ key, label: scheduledStatusLabel(key) }))
}

// ---------------------------------------------------------------------------
// 工厂入参与返回面
// ---------------------------------------------------------------------------

export interface UseScheduledPlanListOptions<E> {
  /** 计划形态（闭集：installment | subscription | scheduled_transfer）。 */
  kind: ScheduledKind
  /**
   * 详情 → 行模型的形态扩展器：`detail = null` 表示详情命令失败，扩展器须返回
   * 该形态的「空值」扩展（转账/订阅：next = null；分期：计数与金额归 0）。
   */
  expandDetail(
    plan: ScheduledTransactionWithExt,
    detail: ScheduledTransactionDetail | null,
  ): E
  /** 清单加载失败的提示文案 getter（形态命名，如「加载定时转账失败」；getter 保证切语言即时生效）。 */
  loadErrorText(): string
  /** 取消确认文案 getter（三形态措辞各异，注入而非写死；确认弹层由适配器渲染）。 */
  cancelConfirmText(): string
  /** 生命周期变更（暂停/恢复/取消）成功并重拉后的回调（订阅注入花费面板刷新）。 */
  onStatusChanged?(): void
  /** 行详情动作（打开计划详情弹窗；弹窗组件留适配器）。 */
  onOpenDetail(row: ScheduledPlanRow<E>): void
}

export interface UseScheduledPlanListReturn<E> {
  /** 行列表（只读）：仅模块内 load 写入。 */
  readonly rows: Readonly<Ref<readonly ScheduledPlanRow<E>[]>>
  /** 清单加载中（只读）：归 NDataTable loading 消费。 */
  readonly loading: Readonly<Ref<boolean>>
  /** 清单状态过滤值（只读），默认「进行中」；改动经 setStatusFilter。 */
  readonly statusFilter: Readonly<Ref<ScheduledStatus>>
  /** 重拉版本号：bump 即「清单数据完成一次重拉」，是唯一的重拉观察信号。 */
  readonly refreshVersion: Readonly<Ref<number>>
  /** 状态过滤后的行（纯前端过滤，不产生请求）。 */
  readonly filteredRows: ComputedRef<ScheduledPlanRow<E>[]>
  /** 按形态的状态过滤选项集（computed：标签随界面语言即时切换）。 */
  readonly statusFilterOptions: ComputedRef<ReadonlyArray<ScheduledPlanStatusOption>>
  /** 清单加载/刷新：按形态拉取计划 + 逐行详情扩展；成功完成 bump refreshVersion。 */
  load(): Promise<void>
  /** 状态过滤意图入口。 */
  setStatusFilter(status: ScheduledStatus): void
  /** Plan Lifecycle 变更：走既有状态命令，成功提示 + 重拉 + 回调；失败提示不重拉。 */
  changeStatus(id: string, newStatus: ScheduledStatus): Promise<void>
  /** 行操作描述符构建：标签、按状态的可用性、确认文案与 run 动作。 */
  rowActions(row: ScheduledPlanRow<E>): ScheduledPlanRowAction[]
}

/**
 * 计划清单工厂：每次调用返回独立实例（各页签独立状态与版本号，避免串扰）。
 * 必须在组件 setup 内调用（成功/失败提示经 useMessage，与仓库既有 composable 形态一致；
 * ADR-0040 的 Loadable 落地后可整体顺带迁移，见其决策 6 预留）。
 */
export function useScheduledPlanList<E>(
  options: UseScheduledPlanListOptions<E>,
): UseScheduledPlanListReturn<E> {
  const { kind, expandDetail, loadErrorText, cancelConfirmText, onStatusChanged, onOpenDetail } =
    options
  const message = useMessage()

  const rows = ref([]) as Ref<ScheduledPlanRow<E>[]>
  const loading = ref(false)
  const statusFilter = ref<ScheduledStatus>('active')
  const refreshVersion = ref(0)

  const filteredRows = computed(() =>
    rows.value.filter((r) => r.plan.core.status === statusFilter.value),
  )

  const statusFilterOptions = computed(() => buildStatusFilterOptions(kind))

  /**
   * 清单加载（唯一写 rows 的路径）：列表命令按形态过滤后，逐行取详情扩展；
   * 单行详情失败标记 detailFailed 不拖垮整单；列表命令失败提示形态文案、行保持旧值。
   * 时序彻底内化：loading 置收、错误提示、版本号 bump 均在此，调用方只管发起。
   */
  async function load() {
    loading.value = true
    try {
      const plans = (await api.listScheduledTransactions()).filter((p) => p.core.kind === kind)
      const details = await Promise.all(
        plans.map(async (p): Promise<ScheduledPlanRow<E>> => {
          try {
            const detail = await api.getScheduledTransactionDetail(p.core.id)
            return { plan: p, detailFailed: false, ext: expandDetail(p, detail) }
          } catch {
            return { plan: p, detailFailed: true, ext: expandDetail(p, null) }
          }
        }),
      )
      rows.value = details
      refreshVersion.value += 1
    } catch (e) {
      message.error(`${loadErrorText()}: ${errorMessage(e)}`)
    } finally {
      loading.value = false
    }
  }

  function setStatusFilter(status: ScheduledStatus) {
    statusFilter.value = status
  }

  async function changeStatus(id: string, newStatus: ScheduledStatus) {
    try {
      await api.updateScheduledTransactionStatus({ id, new_status: newStatus })
      message.success(
        newStatus === 'paused'
          ? t('scheduled.status.paused')
          : newStatus === 'active'
            ? t('scheduled.toast.resumed')
            : t('scheduled.status.cancelled'),
      )
      await load()
      onStatusChanged?.()
    } catch (e) {
      message.error(t('scheduled.toast.operationFailed', { message: errorMessage(e) }))
    }
  }

  /**
   * 行操作描述符（按 Plan Lifecycle 状态的可用性矩阵）：期次详情全状态可用；
   * 暂停限 active、恢复限 paused、取消限 active/paused（不删已生成交易与历史期次）。
   * run 经 void 触发，避免浮 promise；确认弹层由适配器按 confirm 文案渲染。
   * 标签与确认文案调用时经 t() 现取（适配器在渲染中调用本函数，切语言即时生效）。
   */
  function rowActions(row: ScheduledPlanRow<E>): ScheduledPlanRowAction[] {
    const id = row.plan.core.id
    const status = row.plan.core.status
    return [
      {
        key: 'detail',
        label: t('scheduled.action.detail'),
        available: true,
        confirm: null,
        run: () => onOpenDetail(row),
      },
      {
        key: 'pause',
        label: t('scheduled.action.pause'),
        available: status === 'active',
        confirm: null,
        run: () => void changeStatus(id, 'paused'),
      },
      {
        key: 'resume',
        label: t('scheduled.action.resume'),
        available: status === 'paused',
        confirm: null,
        run: () => void changeStatus(id, 'active'),
      },
      {
        key: 'cancel',
        label: t('scheduled.action.cancel'),
        available: status === 'active' || status === 'paused',
        confirm: cancelConfirmText(),
        run: () => void changeStatus(id, 'cancelled'),
      },
    ]
  }

  return {
    rows: readonly(rows) as Readonly<Ref<readonly ScheduledPlanRow<E>[]>>,
    loading: readonly(loading) as Readonly<Ref<boolean>>,
    statusFilter: readonly(statusFilter) as Readonly<Ref<ScheduledStatus>>,
    refreshVersion: readonly(refreshVersion) as Readonly<Ref<number>>,
    filteredRows,
    statusFilterOptions,
    load,
    setStatusFilter,
    changeStatus,
    rowActions,
  }
}

// ---------------------------------------------------------------------------
// 公共详情扩展器：最早 pending 期次（转账「下期转账」列 / 订阅「下期扣款」列）
// ---------------------------------------------------------------------------

/**
 * 最早 pending 期次（仅选取、不现场推算日期，避免第三套日期口径）：
 * 详情命令按 scheduled_date 返回，此处再排序取首条；无 pending（预生成窗口之外）为 null。
 */
export function earliestPendingOccurrence(
  detail: ScheduledTransactionDetail,
): ScheduledTransactionOccurrence | null {
  return (
    [...detail.pending_occurrences].sort((a, b) => a.scheduled_date.localeCompare(b.scheduled_date))[0] ??
    null
  )
}
