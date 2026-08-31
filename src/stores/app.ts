import { defineStore } from 'pinia'
import { ref } from 'vue'
import { loadLocal, saveLocal } from '@/utils/storage'

export type Theme = 'dark' | 'light'

/**
 * UI 设置（UI Settings）store：主题 / 默认币种 / 备份设置 / 设备级「自动执行」开关，本地持久化。
 *
 * 参考数据（currencies/accounts/categories）及全部派生映射、分类树逻辑、
 * 加载函数已迁至 `useReferenceStore`（单一来源，见 #78–#85），本 store 不再
 * 暴露任何参考数据接口，仅保留设备偏好。后端消费的镜像推送（备份目录、
 * 自动执行开关）收口在 `useDevicePreferenceSync`，由应用根组件挂载一次。
 */
export const useAppStore = defineStore('app', () => {
  const theme = ref<Theme>(loadLocal<Theme>('appearance', 'dark'))
  const defaultCurrency = ref<string>(loadLocal<string>('default_currency', 'CNY'))
  const backupDir = ref<string>(loadLocal<string>('backup_dir', ''))
  const backupMaxCount = ref<number>(loadLocal<number>('backup_max_count', 30))
  // 设备级「自动执行」开关（issue #308 / ADR-0042）：默认关，真源在本机
  // localStorage，不随 Backup/Restore 迁移——换新机器或恢复备份后保持默认关；
  // 后端只持运行时镜像，由 useDevicePreferenceSync 启动/变更时推送。
  const autoExecutionEnabled = ref<boolean>(loadLocal<boolean>('auto_execution_enabled', false))

  function setTheme(t: Theme) {
    theme.value = t
    saveLocal('appearance', t)
  }

  function setDefaultCurrency(code: string) {
    defaultCurrency.value = code
    saveLocal('default_currency', code)
  }

  function setBackupDir(dir: string) {
    backupDir.value = dir
    saveLocal('backup_dir', dir)
  }

  function setBackupMaxCount(n: number) {
    backupMaxCount.value = n
    saveLocal('backup_max_count', n)
  }

  function setAutoExecutionEnabled(enabled: boolean) {
    autoExecutionEnabled.value = enabled
    saveLocal('auto_execution_enabled', enabled)
  }

  return {
    theme,
    defaultCurrency,
    backupDir,
    backupMaxCount,
    autoExecutionEnabled,
    setTheme,
    setDefaultCurrency,
    setBackupDir,
    setBackupMaxCount,
    setAutoExecutionEnabled,
  }
})
