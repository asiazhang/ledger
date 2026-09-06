import { api } from '@/api'

/**
 * 成功提示后延迟重启（Restore 同型语义，先例 useBackup.pickRestore）：
 * 转换/搬迁等文件级操作落盘后，让 success toast 先落地再重启。
 * 800ms 与既有恢复流程一致。
 *
 * 重启语义（issue #644 / ADR-0080）：`restart_app` 在后端完成**原位重引导**
 * （重跑启动引导序列：DataLocation 解析 → 库文件判定 → 连接换入 → 两扇门
 * 翻转），返回后前端重载 WebView——重新探测启动相位，落到解锁屏/失败恢复
 * 屏/主界面。不再重建进程：进程重启在 `tauri dev` 下与 CLI 的 dev server
 * 生命周期相克（老进程退出即被 CLI 回收 dev server，新进程拉起时 devUrl
 * 已不可达），正是开发构建白屏的根因；原位重引导在开发与签名构建下行为
 * 一致且原子。重载只在命令成功后发生；失败保留当前界面（仍可操作），
 * 错误进 console 留痕。
 */
export function restartAppShortly(): void {
  setTimeout(() => {
    api
      .restartApp()
      .then(() => {
        window.location.reload()
      })
      .catch((e) => {
        console.warn('应用重启（原位重引导）失败，已保留当前界面', e)
      })
  }, 800)
}
