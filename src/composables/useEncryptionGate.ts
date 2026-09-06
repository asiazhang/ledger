import { ref } from 'vue'
import { api } from '@/api'
import { useAppStore } from '@/stores/app'
import type { RememberPassphraseSupport } from '@/types'

/**
 * 启动门（issue #570 / #601 / ADR-0075 决策 5 修订）：前端启动首屏的状态接缝。
 *
 * 模块级单例 ref——App.vue 挂载时探测，启动屏与 App 共享同一状态：
 * - `locked = null`：探测中（尚未知道启动相位，主界面不渲染，避免业务 IPC
 *   在门禁放行前发出）；
 * - `locked = true`：密文库等待解锁，渲染解锁屏并**不挂载主界面**——解锁先于
 *   一切业务读写，参考数据 store / 设备偏好推送等 IPC 消费方都随主界面一起
 *   延迟到解锁成功后启动；
 * - `locked = false`：明文库或已解锁，主界面正常挂载；
 * - `bootFailed = true`：启动失败（库打不开），渲染启动失败恢复屏——后端已
 *   登记失败门、业务 IPC 被门禁拦截，恢复通道（重置为空库）成功后翻转。
 *
 * 解锁成功即翻转为 `false`；若后端在解锁时补做了等待中的搬迁
 * （relocated = true），由解锁屏触发应用重启（Restore 同型语义）。
 * 忘记口令重置（issue #573）与启动失败重置（issue #601）同样翻转状态：
 * 主界面随全新明文空库挂载。
 */
const locked = ref<boolean | null>(null)

/** 启动失败状态（issue #601）：后端启动失败门的前端镜像，失败恢复屏由它驱动。 */
const bootFailed = ref(false)

/**
 * 自动解锁有界等待上限（issue #644）：钥匙串读取/生物认证在受限形态或系统
 * 弹窗滞留时可能长时间不返回，解锁屏对自动解锁的等待必须有边界——到期回退
 * 手输解锁屏并提示，绝不无限停留在「正在尝试自动解锁」加载态（加载态遮蔽
 * 了手输与逃生门双入口，是白屏/卡死的表现形态之一）。取值兼顾合法慢路径：
 * 生物认证门下用户完成 Touch ID 需要时间，30s 内未完成视为受阻。后端调用
 * 不取消：迟到成功照常进入应用，迟到失败由回退手输路径消化。
 */
export const AUTO_UNLOCK_TIMEOUT_MS = 30_000

/** 凭缓存解锁的等待结果（issue #644）：`timeout` 表示有界等待到期——后端调用
 *  仍在进行，其迟到结局仍有归属（成功照常翻门进入、失败不再上抛），只是
 *  解锁屏不再替它等待。 */
export type AutoUnlockWait = { status: 'unlocked'; relocated: boolean } | { status: 'timeout' }


/** 本机记住主口令的平台能力与运行形态（issue #574 / #662）：模块级单例，解锁屏
 *  与设置页共享（懒加载只查一次）。`null` = 尚未查询（调用 [`loadRememberSupport`] 填充）。 */
const rememberSupport = ref<RememberPassphraseSupport | null>(null)

export function useEncryptionGate() {
  /**
   * 启动探测（issue #601）：查询后端启动状态，一次拿到主界面/解锁屏/失败
   * 恢复屏三态选择。探测失败按**锁定**处理（fail-closed，与加密的安全姿态
   * 一致）：解锁屏仍可渲染，后端门禁也仍在拦截，不存在「主界面挂载却全部
   * IPC 被拒、又无解锁入口」的死角。
   */
  async function probe(): Promise<void> {
    try {
      const status = await api.getBootStatus()
      if (status.phase === 'failed') {
        bootFailed.value = true
      } else {
        locked.value = status.phase === 'locked'
      }
    } catch (e) {
      console.warn('启动状态探测失败，按锁定处理（fail-closed）', e)
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
      // mode 为占位：supported=false 时前端只读 supported 隐藏全部选项，mode 不被消费。
      rememberSupport.value = { supported: false, mode: 'biometry' }
    }
  }

  /** 凭本机缓存的主口令解锁（issue #574）：成功即翻转状态，主界面随之挂载。
   *  等待有界（issue #644）：超过 [`AUTO_UNLOCK_TIMEOUT_MS`] 返回 `timeout`，
   *  调用方回退手输；后端调用不取消，迟到结局仍有归属——迟到成功先复核后端
   *  相位再翻门（等待期间可能已手输解锁/触发重引导，相位可能已变；仅后端
   *  确已就绪才翻转，不遮蔽真实锁定/失败态），迟到失败静默消化，不炸未处理
   *  拒绝。口令在后端钥匙串读出，不回流前端。失败（无缓存 / 生物认证取消 /
   *  缓存口令已过期）在到期前发生则原样上抛，由调用方回退手输。 */
  async function unlockWithRemembered(): Promise<AutoUnlockWait> {
    let timer: ReturnType<typeof setTimeout> | undefined
    let expired = false
    const core = api.unlockWithRememberedPassphrase().then(
      async (outcome): Promise<AutoUnlockWait> => {
        if (expired) {
          // 迟到结局守卫（issue #644 审查）：正常路径不付这次探测，仅迟到时复核。
          const status = await api.getBootStatus()
          if (status.phase !== 'ready') {
            return { status: 'unlocked', relocated: outcome.relocated }
          }
        }
        locked.value = false
        return { status: 'unlocked', relocated: outcome.relocated }
      },
    )
    // 迟到结局的归属：成功照常翻转锁定门（见上）；失败不再上抛——等待已
    // 到期、回退提示已出，迟到的失败由手输路径与下次启动消化。
    core.catch(() => {})
    try {
      return await Promise.race([
        core,
        new Promise<AutoUnlockWait>((resolve) => {
          timer = setTimeout(() => {
            expired = true
            resolve({ status: 'timeout' })
          }, AUTO_UNLOCK_TIMEOUT_MS)
        }),
      ])
    } finally {
      // 竞速早胜后清定时器（审查）：不悬挂 30s 计时器。
      if (timer !== undefined) clearTimeout(timer)
    }
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

  /**
   * 启动失败重置（issue #601 / ADR-0075 决策 5 修订）：失败恢复屏确认后的
   * 执行面。后端把打不开的旧库重置为全新明文空库（旧库保留 .bak 副本）并
   * 原位换连、拉起调度，成功即翻转状态，主界面随全新空库挂载，无需重启；
   * 失败保持失败屏，可重试。
   */
  async function resetFromFailure(): Promise<void> {
    await api.resetAfterStartupFailure()
    bootFailed.value = false
    locked.value = false
  }

  return {
    locked,
    bootFailed,
    rememberSupport,
    probe,
    unlock,
    unlockWithRemembered,
    loadRememberSupport,
    syncRememberCache,
    clearRememberCache,
    reset,
    resetFromFailure,
  }
}
