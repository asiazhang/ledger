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
 * 「从备份文件恢复」共享流（issue #572 / #602 / ADR-0075 决策 7）：文件选择器
 * 选定备份 → 元数据校验 → 当前模式探测 → 恢复确认弹窗（跨模式警告 + 密文备份
 * 主口令，语义面在 RestoreConfirmModal）→ 既有 Restore 全语义（RestoreSafetyBackup
 * 字节副本 + 原子替换）→ 成功后自动重启（Restore 同型重启语义）。
 *
 * 宿主只各给一份文件选择器参数，编排零拷贝：
 * - 设置页备份卡（useBackup，issue #572）；
 * - 启动失败恢复屏（useFailureRestore，issue #602，无已打开库连接状态可用）；
 * - 解锁屏恢复入口（issue #603，携带上下文口令自动试开）。
 *
 * 校验失败即中止（确认弹窗不开）：元数据读取失败视为无效备份；当前模式探测
 * 失败按明文回落会在密文备份上静默跳过跨模式警告（破坏性操作前的安全面，
 * 宁可不弹窗）。恢复失败不关弹窗：口令错误可就地重试（解锁同语义，ADR-0075
 * 决策 5），错误文案经码化错误归一（errorMessage）。
 */
export function useRestoreFromFile(options: {
  /** 文件选择器标题的 i18n key。 */
  pickTitleKey: string
  /** 文件选择器默认目录（可选；设置页传配置的备份目录，无则系统默认）。 */
  defaultPath?: () => string | undefined
  /** 宿主上下文口令（issue #603，可选）：非空时随意图携带，恢复确认弹窗先
   *  自动试开、失败才显出口令框重输（解锁屏传手输过的主口令；取值在选定
   *  备份那一刻快照，不随后续输入漂移）。 */
  contextPassphrase?: () => string
}) {
  const message = useMessage();

  // 恢复确认弹窗（ADR-0072）：意图内化开启/目标/关闭编排。
  const restoreModal = useModalIntent<RestoreIntent>()
  const restoring = ref(false)

  /** 文件选择器选定备份 → 元数据校验 → 当前模式探测 → 打开恢复确认弹窗。 */
  async function pickRestore() {
    const path = await open({
      title: t(options.pickTitleKey),
      directory: false,
      multiple: false,
      defaultPath: options.defaultPath?.(),
      filters: [{ name: t('settings.data.msg.filterName'), extensions: ['zip', 'db'] }],
    })
    if (typeof path !== 'string' || !path) return
    // 元数据校验（加密标记驱动跨模式警告与口令输入）：读取失败视为无效
    // 备份，报错中止（与恢复时同样失败，只是提前到确认前）。
    let meta: BackupMetaSummary
    try {
      meta = await api.getBackupMeta(path)
    } catch (e) {
      message.error(t('settings.data.msg.backupMetaFailed', { msg: errorMessage(e) }))
      return
    }
    // 当前库模式（文件即真相）：读取失败则中止恢复——按明文回落会在加密库
    // 上静默跳过跨模式警告（销毁性操作前的安全面，宁可不弹窗）。
    let currentEncrypted: boolean
    try {
      currentEncrypted = (await api.getEncryptionStatus()).file_encrypted
    } catch (e) {
      message.error(t('settings.data.msg.encryptionStatusFailed', { msg: errorMessage(e) }))
      return
    }
    const contextPassphrase = options.contextPassphrase?.() ?? ''
    restoreModal.open({
      path,
      backupEncrypted: meta.encrypted,
      currentEncrypted,
      // 上下文口令（issue #603）：宿主提供且非空才携带，空白不进意图（与
      // 不携带同形，弹窗直接弹口令框）；取值在选定备份那一刻快照。
      ...(contextPassphrase.trim() ? { contextPassphrase } : {}),
    })
  }

  /** 恢复确认（弹窗提交）：密文备份附带主口令；明文备份口令位为空也不消费
   * （传 null）。口令非空即随请求上送——元数据谎报明文而实库为密文时，弹窗
   * 依后端 `backup.passphrase-required` 显出口令框，重输由此可达（不空转）。 */
  async function confirmRestore(passphrase: string) {
    const intent = restoreModal.intent.value
    if (!intent) return
    restoring.value = true
    // 失败不关弹窗：口令错误可就地重试；错误经弹窗内错误位展示（close 仅成功路径到达）。
    try {
      const r = await api.restoreBackup(
        intent.path,
        intent.backupEncrypted || passphrase ? passphrase : null,
      )
      restoreModal.close()
      message.success(t('settings.data.msg.restoreOk', { version: r.schema_version }))
      // 恢复成功后应用重启，由启动探测接管实际模式（ADR-0075 决策 4/7）。
      restartAppShortly()
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
    pickRestore,
  }
}
