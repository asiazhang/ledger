import type { ScheduledStatus } from '@/types'

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
