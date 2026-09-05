import { api } from '@/api'

/**
 * 成功提示后延迟重启（Restore 同型语义，先例 useBackup.pickRestore）：
 * 转换/搬迁等文件级操作落盘后，让 success toast 先落地再重启进程。
 * 800ms 与既有恢复流程一致。
 */
export function restartAppShortly(): void {
  setTimeout(() => {
    void api.restartApp()
  }, 800)
}
