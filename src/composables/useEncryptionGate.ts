import { ref } from 'vue'
import { api } from '@/api'

/**
 * 加密锁定门（issue #570 / ADR-0075 决策 5）：前端启动解锁屏的状态接缝。
 *
 * 模块级单例 ref——App.vue 挂载时探测，解锁屏与 App 共享同一状态：
 * - `null`：探测中（尚未知道是否锁定，主界面不渲染，避免锁定期间发出业务 IPC）；
 * - `true`：密文库等待解锁，渲染解锁屏并**不挂载主界面**——解锁先于一切
 *   业务读写，参考数据 store / 设备偏好推送等 IPC 消费方都随主界面一起
 *   延迟到解锁成功后启动；
 * - `false`：明文库或已解锁，主界面正常挂载。
 *
 * 解锁成功即翻转为 `false`；若后端在解锁时补做了等待中的搬迁
 * （relocated = true），由解锁屏触发应用重启（Restore 同型语义）。
 */
const locked = ref<boolean | null>(null)

export function useEncryptionGate() {
  /**
   * 启动探测：查询后端锁定门状态。探测失败按**锁定**处理（fail-closed，
   * 与加密的安全姿态一致）：解锁屏仍可渲染，后端门禁也仍在拦截，不存在
   * 「主界面挂载却全部 IPC 被拒、又无解锁入口」的死角。
   */
  async function probe(): Promise<void> {
    try {
      locked.value = (await api.getEncryptionStatus()).locked
    } catch (e) {
      console.warn('加密状态探测失败，按锁定处理（fail-closed）', e)
      locked.value = true
    }
  }

  /** 解锁：成功即翻转状态，主界面随之挂载；返回是否补做了搬迁。 */
  async function unlock(passphrase: string): Promise<boolean> {
    const outcome = await api.unlockEncryption(passphrase)
    locked.value = false
    return outcome.relocated
  }

  return { locked, probe, unlock }
}
