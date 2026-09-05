import { ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { useMessage } from 'naive-ui'
import { api } from '@/api'
import type { BackupMetaSummary } from '@/types'
import { t } from '@/i18n'
import { errorMessage } from '@/utils/errors'
import { restartAppShortly } from '@/utils/restart'
import { useModalIntent } from '@/composables/useModalIntent'
import type { RestoreIntent } from '@/composables/useBackup'

/**
 * 启动失败恢复屏的「从备份文件恢复…」通道（issue #602 / ADR-0075 决策 5 修订）。
 *
 * 走既有 Restore 全语义（后端 `restore_backup` 在无已打开库连接状态下可用，
 * issue #601 前置修复）：文件选择器选定备份 zip → 元数据校验 → 恢复确认弹窗
 * （跨模式警告与密文备份口令复用 #572 的恢复确认弹窗，见
 * `RestoreConfirmModal`）→ 恢复执行（RestoreSafetyBackup 字节副本 + 原子替换）
 * → 成功后自动重启进入恢复后的数据（Restore 同型重启语义）。
 *
 * 口令交互：明文库损坏场景无上下文口令，意图不携带 `contextPassphrase`——
 * 密文备份直接弹口令框，口令错误不关弹窗、可无限重输；解锁屏场景
 * （issue #603）复用本流并携带手输过的口令自动试开。
 */
export function useFailureRestore() {
  const message = useMessage();

  // 恢复确认弹窗（ADR-0072）：意图内化开启/目标/关闭编排，与设置页恢复同型。
  const restoreModal = useModalIntent<RestoreIntent>()
  const restoring = ref(false)

  /** 文件选择器选定备份 → 元数据校验 → 当前模式探测 → 打开恢复确认弹窗。 */
  async function pickRestoreFromFailure() {
    const path = await open({
      title: t('startupFailure.restorePickTitle'),
      directory: false,
      multiple: false,
      filters: [{ name: t('settings.data.msg.filterName'), extensions: ['zip', 'db'] }],
    })
    if (typeof path !== 'string' || !path) return
    // 元数据校验（issue #572 同款）：读取失败视为无效备份，报错中止（与恢复
    // 时同样失败，只是提前到确认前）。
    let meta: BackupMetaSummary
    try {
      meta = await api.getBackupMeta(path)
    } catch (e) {
      message.error(t('settings.data.msg.backupMetaFailed', { msg: errorMessage(e) }))
      return
    }
    // 当前库模式（文件即真相）：头探测不依赖已打开库连接，启动失败状态下
    // 可读；读取失败则中止恢复——按明文回落会在密文备份上静默跳过跨模式
    // 警告（破坏性操作前的安全面，宁可不弹窗）。
    let currentEncrypted: boolean
    try {
      currentEncrypted = (await api.getEncryptionStatus()).file_encrypted
    } catch (e) {
      message.error(t('settings.data.msg.encryptionStatusFailed', { msg: errorMessage(e) }))
      return
    }
    restoreModal.open({ path, backupEncrypted: meta.encrypted, currentEncrypted })
  }

  /** 恢复确认（弹窗提交）：密文备份附带主口令，明文备份不消费。 */
  async function confirmRestore(passphrase: string) {
    const intent = restoreModal.intent.value
    if (!intent) return
    restoring.value = true
    try {
      const r = await api.restoreBackup(
        intent.path,
        intent.backupEncrypted ? passphrase : null,
      )
      restoreModal.close()
      message.success(t('settings.data.msg.restoreOk', { version: r.schema_version }))
      // 恢复成功后应用自动重启，由启动探测接管实际模式（ADR-0075 决策 4/7）：
      // 重启后主界面/解锁屏/失败屏的再选择与本次恢复前的状态无关。
      restartAppShortly()
    } catch (e) {
      // 失败不关弹窗：口令错误可就地重试（解锁同语义，ADR-0075 决策 5），
      // 错误文案经码化错误归一（errorMessage）。
      throw e
    } finally {
      restoring.value = false
    }
  }

  return {
    restoring,
    restoreIntent: restoreModal.intent,
    restoreSeq: restoreModal.seq,
    closeRestore: restoreModal.close,
    confirmRestore,
    pickRestoreFromFailure,
  }
}
