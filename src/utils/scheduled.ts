import { t } from '@/i18n'
import type { ScheduledStatus, ScheduledTransactionOccurrence } from '@/types'

/** 计划状态 → 状态标签（订阅清单与花费分析面板共用，避免映射漂移；渲染时经 t() 翻译，随界面语言即时切换） */
export function scheduledStatusLabel(status: string): string {
  switch (status as ScheduledStatus) {
    case 'active':
      return t('scheduled.status.active')
    case 'paused':
      return t('scheduled.status.paused')
    case 'cancelled':
      return t('scheduled.status.cancelled')
    case 'completed':
      return t('scheduled.status.completed')
    default:
      return status
  }
}

/** 期次状态 → 状态标签（期次详情弹窗用，issue #205；与后端 OccurrenceStatus 枚举一致；渲染时经 t() 翻译） */
export function occurrenceStatusLabel(status: ScheduledTransactionOccurrence['status']): string {
  switch (status) {
    case 'pending':
      return t('scheduled.occurrenceStatus.pending')
    case 'processing':
      return t('scheduled.occurrenceStatus.processing')
    case 'completed':
      return t('scheduled.occurrenceStatus.completed')
    case 'failed':
      return t('scheduled.occurrenceStatus.failed')
    case 'cancelled':
      return t('scheduled.occurrenceStatus.cancelled')
    default:
      return status
  }
}
