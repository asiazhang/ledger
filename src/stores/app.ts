import { defineStore } from 'pinia'
import { ref } from 'vue'
import { loadLocal, saveLocal } from '@/utils/storage'

export type Theme = 'dark' | 'light'

/**
 * UI 设置（UI Settings）store：主题 / 默认币种 / 备份设置，本地持久化。
 *
 * 参考数据（currencies/accounts/categories）及全部派生映射、分类树逻辑、
 * 加载函数已迁至 `useReferenceStore`（单一来源，见 #78–#85），本 store 不再
 * 暴露任何参考数据接口，仅保留 UI 偏好。
 */
export const useAppStore = defineStore('app', () => {
  const theme = ref<Theme>(loadLocal<Theme>('appearance', 'dark'))
  const defaultCurrency = ref<string>(loadLocal<string>('default_currency', 'CNY'))
  const backupDir = ref<string>(loadLocal<string>('backup_dir', ''))
  const backupMaxCount = ref<number>(loadLocal<number>('backup_max_count', 30))

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

  return {
    theme,
    defaultCurrency,
    backupDir,
    backupMaxCount,
    setTheme,
    setDefaultCurrency,
    setBackupDir,
    setBackupMaxCount,
  }
})
