import type { ScheduledStatus, ScheduledTransactionOccurrence } from '@/types'

/** 计划状态 → 中文标签（订阅清单与花费分析面板共用，避免映射漂移） */
export function scheduledStatusLabel(status: string): string {
  switch (status as ScheduledStatus) {
    case 'active':
      return '进行中'
    case 'paused':
      return '已暂停'
    case 'cancelled':
      return '已取消'
    case 'completed':
      return '已完成'
    default:
      return status
  }
}

/** 期次状态 → 中文标签（期次详情弹窗用，issue #205；与后端 OccurrenceStatus 枚举一致） */
export function occurrenceStatusLabel(status: ScheduledTransactionOccurrence['status']): string {
  switch (status) {
    case 'pending':
      return '待执行'
    case 'processing':
      return '执行中'
    case 'completed':
      return '已完成'
    case 'failed':
      return '失败'
    case 'cancelled':
      return '已取消'
    default:
      return status
  }
}
