import { useRestoreFromFile } from '@/composables/useRestoreFromFile'

/**
 * 启动失败恢复屏的「从备份文件恢复…」通道（issue #602 / ADR-0075 决策 5 修订）。
 *
 * 编排收口在共享恢复流 useRestoreFromFile（文件选择器 → 元数据校验 → 当前
 * 模式探测 → 恢复确认弹窗 → Restore 全语义 → 自动重启），本适配器只表达
 * 失败屏差异：无文件选择器默认目录。既有 `restore_backup` 在无已打开库连接
 * 状态下可用（issue #601 前置修复），恢复后自动重启进入恢复后的数据，由启动
 * 探测接管实际模式。
 *
 * 口令交互：明文库损坏场景无上下文口令——密文备份直接弹口令框，口令错误
 * 不关弹窗、可无限重输；解锁屏场景（issue #603）复用同一恢复流并携带手输过
 * 的口令自动试开（上下文口令接缝在 RestoreIntent.contextPassphrase）。
 */
export function useFailureRestore() {
  const flow = useRestoreFromFile({
    pickTitleKey: 'startupFailure.restorePickTitle',
  })

  const { restoring, restoreIntent, restoreSeq, closeRestore, confirmRestore } = flow

  return {
    restoring,
    restoreIntent,
    restoreSeq,
    closeRestore,
    confirmRestore,
    pickRestoreFromFailure: flow.pickRestore,
  }
}
