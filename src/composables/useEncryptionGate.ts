import { ref } from 'vue'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import type { RememberPassphraseSupport } from '@/types'

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
 * 忘记口令重置（issue #573）同样翻转状态：主界面随全新明文空库挂载。
 */
const locked = ref<boolean | null>(null)

/** 本机记住主口令的平台能力（issue #574）：模块级单例，解锁屏与设置页共享
 *  （懒加载只查一次）。`null` = 尚未查询（调用 [`loadRememberSupport`] 填充）。 */
const rememberSupport = ref<RememberPassphraseSupport | null>(null)

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

  /** 懒加载「本机记住主口令」的平台能力（issue #574）：成功填充，失败按不支持处理
   *  （fail-closed：不支持平台隐藏选项、回退手输，与加密安全姿态一致）。 */
  async function loadRememberSupport(): Promise<void> {
    if (rememberSupport.value) return
    try {
      rememberSupport.value = await api.getRememberPassphraseSupport()
    } catch (e) {
      console.warn('读取本机记住主口令能力失败，按不支持处理', e)
      rememberSupport.value = { supported: false }
    }
  }

  /** 凭本机缓存的主口令解锁（issue #574）：成功即翻转状态，主界面随之挂载；
   *  返回是否补做了搬迁。口令在后端钥匙串读出，不回流前端。失败（无缓存 / 生物
   *  认证取消 / 缓存口令已过期）由调用方回退手输。 */
  async function unlockWithRemembered(): Promise<boolean> {
    const outcome = await api.unlockWithRememberedPassphrase()
    locked.value = false
    return outcome.relocated
  }

  /** 清空「记住」的钥匙串缓存与偏好（关闭加密 / 忘记口令重置 / 关闭开关共用）。 */
  async function clearRememberCache(): Promise<void> {
    useAppStore().setRememberPassphrase(false)
    try {
      await api.clearRememberPassphrase()
    } catch {
      /* 清除失败幂等容忍（无缓存即成功；异常不阻断主流程） */
    }
  }

  /** 按「记住」勾选同步钥匙串缓存与偏好（issue #574）：勾选缓存主口令，失败回退不记住
   *  并返回 false（由调用方提示）；取消勾选清缓存、恢复手输。返回是否成功缓存。
   *  偏好写入是应用启动时的落盘轻量设置，故在调用点读取 store，不做工厂期持有。 */
  async function syncRememberCache(passphrase: string, checked: boolean): Promise<boolean> {
    if (!checked) {
      await clearRememberCache()
      return true
    }
    const store = useAppStore()
    store.setRememberPassphrase(true)
    try {
      await api.setRememberPassphrase(passphrase)
      return true
    } catch {
      store.setRememberPassphrase(false)
      return false
    }
  }

  /**
   * 忘记口令重置（issue #573 / ADR-0075 决策 2/5）：逃生门确认后的执行面。
   * 后端把密文库重置为全新明文空库（旧库保留密文副本），成功即翻转状态，
   * 主界面随全新空库挂载，无需重启；失败保持锁定，可重试。
   */
  async function reset(): Promise<void> {
    await api.resetAfterForgottenPassphrase()
    locked.value = false
  }

  return {
    locked,
    rememberSupport,
    probe,
    unlock,
    unlockWithRemembered,
    loadRememberSupport,
    syncRememberCache,
    clearRememberCache,
    reset,
  }
}
