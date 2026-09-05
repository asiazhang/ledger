<script setup lang="ts">
/**
 * 解锁屏（issue #570 / ADR-0075 决策 5）：加密模式启动首屏。
 *
 * 密文库启动后、解锁成功前，本组件**整体替代**主界面（App.vue 以
 * `v-if` 分支渲染，主界面与全部业务 IPC 消费方都不挂载——解锁先于一切
 * 业务读写）。手输主口令、可无限重试；解锁失败提示采「口令错误或文件损坏」
 * 合并口径（issue #603 / ADR-0075 决策 5 修订：SQLCipher 下错误口令与文件
 * 损坏同为 NOTADB、运行期不可区分，不误报损坏），按码经 errorMessage 本地化。
 *
 * 本机记住主口令（issue #574 / ADR-0075 决策 3/5）：开启「记住」后，
 * 启动即在**有缓存时**凭系统钥匙串（macOS 以 Touch ID 生物认证门保护）
 * 自动解锁——本屏挂载即尝试，认证通过直接进入、认证取消或无缓存回退手输
 * （只损失便利，不损失数据）。平台不支持时隐藏「记住」选项并回退手输。
 * 「记住」偏好是前端 localStorage 轻量设置项（app store），钥匙串内容为
 * 主口令本身、密钥仍由口令派生（备份跨设备可移植性不受影响）。口令在
 * 自动解锁时由后端钥匙串读出，不回流前端。
 *
 * 常驻「忘记口令」入口（issue #573 / ADR-0075 决策 2/5）：进入无后门
 * 后果说明（数据不可恢复）→ error 级二次确认（issue #652 / ADR-0078，
 * 应用内分级弹窗）→ 后端重置为全新明文空库，旧库保留密文副本；重置成功即
 * 翻转锁定门，主界面随全新空库挂载，无需重启。
 *
 * 常驻「从备份文件恢复」入口（issue #603 / ADR-0075 决策 5 修订）：与
 * 「忘记口令」并列可见的恢复通道，复用共享恢复流 useRestoreFromFile（与
 * 设置页备份卡/失败恢复屏零拷贝）——文件选择器 → 元数据校验 → 当前模式
 * 探测 → 恢复确认弹窗（跨模式警告 + 密文备份主口令，语义面在
 * RestoreConfirmModal）→ 既有 Restore 全语义 → 成功后自动重启，由启动
 * 探测接管实际模式（恢复出明文库则重启后直达主界面，恢复出密文库则回到
 * 本屏）。上下文口令自动试开：手输过的主口令随意图携带，密文备份先自动
 * 试开、失败才在弹窗内显出口令框重输（可无限重试）。锁定门禁白名单已放行
 * 恢复通道最小命令面（get_backup_meta / restore_backup / restart_app）。
 *
 * 无需注册 Overlay Suppression：本屏挂载期间侧栏/视图/快捷键宿主全部
 * 不存在，无被抑制对象；ESC 守卫由 useWindowGuard 的全局 preventDefault
 * 覆盖，不存在「ESC 关掉解锁屏」的通路。
 */
import { NButton, NCard, NCheckbox, NInput, NSpace, NSpin, NText, useMessage } from 'naive-ui'
import { onMounted, ref } from 'vue'
import RestoreConfirmModal from '@/components/RestoreConfirmModal.vue'
import { t } from '@/i18n'
import { useEncryptionGate } from '@/composables/useEncryptionGate'
import { useRestoreFromFile } from '@/composables/useRestoreFromFile'
import { useAppStore } from '@/stores/app'
import { errorMessage } from '@/utils/errors'
import { restartAppShortly } from '@/utils/restart'
import AppDangerConfirmModal from '@/components/AppDangerConfirmModal.vue'

const {
  unlock,
  reset,
  rememberSupport,
  unlockWithRemembered,
  loadRememberSupport,
  syncRememberCache,
} = useEncryptionGate()
const store = useAppStore()
const message = useMessage()

const passphrase = ref('')
const submitting = ref(false)
const resetting = ref(false)
const errorText = ref('')

// 从备份文件恢复（issue #603）：共享恢复流第三处复用（设置页备份卡/失败恢复
// 屏同源零拷贝）；上下文口令取当前手输框的值——选定备份那一刻非空即随意图
// 携带，密文备份确认时先自动试开。
const {
  restoreIntent,
  restoreSeq,
  closeRestore,
  confirmRestore,
  pickRestore,
} = useRestoreFromFile({
  pickTitleKey: 'unlock.restorePickTitle',
  defaultPath: () => store.backupDir || undefined,
  contextPassphrase: () => passphrase.value,
})

// 本机记住主口令（issue #574）：`autoUnlocking` 初始即反映偏好——「记住」开启时
// 启动即显示自动解锁加载态；关闭时直接进手输表单。`rememberChecked` 是手动解锁
// 时是否缓存口令的复选框（反映当前偏好，可在解锁时即时开/关）。
const autoUnlocking = ref(store.rememberPassphrase)
const autoUnlockFallback = ref('')
const rememberChecked = ref(store.rememberPassphrase)

// 忘记口令重置确认弹窗（issue #652 / ADR-0078）：error 级应用内弹窗替代原生
// confirm——不可逆（重置为全新空库），后果说明（无后门、密文副本保留）必选。
const resetConfirmShow = ref(false)

onMounted(async () => {
  // 平台能力恒加载（关闭「记住」时也要让复选框可按「平台支持」显隐）；开启时再尝试自动解锁。
  await loadRememberSupport()
  if (!autoUnlocking.value) return
  if (!rememberSupport.value?.supported) {
    // 平台不支持：回退手输（「记住」已开但本机不缓存，等价于未开启）。
    autoUnlocking.value = false
    return
  }
  try {
    const relocated = await unlockWithRemembered()
    if (relocated) {
      // 自动解锁时补做了等待中的搬迁：提示后立即重启（Restore 同型语义）。
      message.success(t('unlock.relocated'))
      restartAppShortly()
    }
  } catch (e) {
    // 无缓存 / 生物认证取消 / 缓存口令已过期：回退手输，提示按码本地化。
    autoUnlocking.value = false
    autoUnlockFallback.value = errorMessage(e)
  }
})

async function submit() {
  if (submitting.value) return
  errorText.value = ''
  submitting.value = true
  try {
    const relocated = await unlock(passphrase.value)
    const cached = await syncRememberCache(passphrase.value, rememberChecked.value)
    if (!cached) message.warning(t('unlock.rememberFailed'))
    if (relocated) {
      // 解锁时补做了等待中的搬迁：提示后立即重启，由启动引导接管目标位置
      // （与 Restore「恢复成功后自动重启」同型；不让用户在旧位置继续写入）。
      message.success(t('unlock.relocated'))
      restartAppShortly()
    }
    passphrase.value = ''
  } catch (e) {
    // 口令错误 → 可重试文案；文件损坏等其它错误 → 透传区分文案（按码本地化）。
    errorText.value = errorMessage(e)
  } finally {
    submitting.value = false
  }
}

/** 忘记口令第一步：弹 error 级确认弹窗（后果说明：无后门、不可恢复，ADR-0078）。 */
function forgotPassphrase() {
  if (resetting.value) return
  errorText.value = ''
  resetConfirmShow.value = true
}

/** 重置确认：重置为全新明文空库。取消或失败都留在解锁屏，可继续尝试口令或
 *  再次进入。重置后旧主口令不再适用，后端已清钥匙串缓存（ADR-0075 决策 5），
 *  此处同步清「记住」偏好为关。 */
async function confirmReset() {
  resetConfirmShow.value = false
  if (resetting.value) return
  resetting.value = true
  try {
    await reset()
    // 后端已清钥匙串缓存；此处仅清前端「记住」偏好（全新明文空库无主口令可记住）。
    store.setRememberPassphrase(false)
    message.success(t('unlock.resetOk'))
  } catch (e) {
    errorText.value = errorMessage(e)
  } finally {
    resetting.value = false
  }
}
</script>

<template>
  <div class="unlock-screen">
    <NCard class="unlock-card" :bordered="false">
      <NSpace vertical :size="16" align="center" :style="{ width: '100%' }">
        <!-- 自动解锁加载态（issue #574）：「记住」开启且正在尝试，认证通过即进入应用 -->
        <NSpin v-if="autoUnlocking" size="small">
          <div class="unlock-loading">{{ t('unlock.autoUnlocking') }}</div>
        </NSpin>

        <template v-else>
          <NText class="unlock-title">{{ t('unlock.title') }}</NText>
          <NText depth="3">{{ t('unlock.hint') }}</NText>
          <NInput
            v-model:value="passphrase"
            type="password"
            show-password-on="click"
            :placeholder="t('unlock.placeholder')"
            :disabled="submitting"
            autofocus
            @keyup.enter="submit"
          />
          <NText v-if="errorText" type="error" class="unlock-error">{{ errorText }}</NText>
          <!-- 自动解锁回退提示（无缓存 / 生物认证取消）：告知用户回退手输 -->
          <NText v-if="autoUnlockFallback" type="warning" class="unlock-error">
            {{ autoUnlockFallback }}
          </NText>
          <NButton type="primary" block :loading="submitting" :disabled="!passphrase" @click="submit">
            {{ t('unlock.button') }}
          </NButton>
          <!-- 本机记住主口令（issue #574）：平台不支持（v1 非 macOS）时隐藏该选项 -->
          <NCheckbox
            v-if="rememberSupport?.supported"
            v-model:checked="rememberChecked"
            :disabled="submitting"
          >
            <NText depth="3">{{ t('unlock.remember') }}</NText>
          </NCheckbox>
          <NText v-if="rememberSupport?.supported" depth="3" class="unlock-remember-hint">
            {{ t('unlock.rememberHint') }}
          </NText>
          <!-- 逃生门双入口（issue #573 / #603）：忘记口令重置与从备份文件恢复并列常驻 -->
          <NSpace :size="4" justify="center">
            <NButton
              quaternary
              size="small"
              data-testid="unlock-restore-open"
              :disabled="resetting"
              @click="pickRestore"
            >
              {{ t('unlock.restore') }}
            </NButton>
            <NButton quaternary size="small" :disabled="resetting" @click="forgotPassphrase">
              {{ t('unlock.forgot') }}
            </NButton>
          </NSpace>
        </template>
      </NSpace>
    </NCard>

    <!-- 备份恢复确认弹窗（issue #603）：跨模式警告 + 密文备份口令，语义面与
         设置页/失败恢复屏同一 RestoreConfirmModal（上下文口令自动试开） -->
    <RestoreConfirmModal
      :intent="restoreIntent"
      :seq="restoreSeq"
      :on-confirm="confirmRestore"
      @close="closeRestore"
    />

    <!-- 忘记口令重置确认（issue #652 / ADR-0078）：error 级——不可逆（清空数据），
         承载无后门后果说明与密文副本保留兑底；确认后重置流程不变 -->
    <AppDangerConfirmModal
      level="error"
      v-model:show="resetConfirmShow"
      :title="t('unlock.resetTitle')"
      :strong-warning="t('unlock.resetStrong')"
      :detail="t('unlock.resetDetail')"
      :confirm-text="t('unlock.resetConfirm')"
      :cancel-text="t('unlock.resetCancel')"
      :submitting="resetting"
      :on-confirm="confirmReset"
      :on-cancel="() => (resetConfirmShow = false)"
    />
  </div>
</template>

<style scoped>
.unlock-screen {
  position: fixed;
  inset: 0;
  z-index: 4000;
  display: flex;
  align-items: center;
  justify-content: center;
}

.unlock-card {
  width: 360px;
}

.unlock-title {
  font-size: 18px;
  font-weight: 600;
}

.unlock-loading {
  padding: 12px 0;
}

.unlock-error {
  /* 独占一行，重试提示完整可见 */
  align-self: flex-start;
}

.unlock-remember-hint {
  align-self: flex-start;
  font-size: 12px;
  line-height: 1.6;
}
</style>
