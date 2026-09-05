<script setup lang="ts">
/**
 * 解锁屏（issue #570 / ADR-0075 决策 5）：加密模式启动首屏。
 *
 * 密文库启动后、解锁成功前，本组件**整体替代**主界面（App.vue 以
 * `v-if` 分支渲染，主界面与全部业务 IPC 消费方都不挂载——解锁先于一切
 * 业务读写）。手输主口令、可无限重试；「口令错误」与「文件损坏」的
 * 用户可见文案由后端码化错误区分（`encryption.passphrase-incorrect` /
 * `encryption.db-corrupt`），经 errorMessage 按码本地化。
 *
 * 常驻「忘记口令」入口（issue #573 / ADR-0075 决策 2/5）：进入无后门
 * 后果说明（数据不可恢复）→ 二次确认（native confirm，与设置页加密
 * 卡片同型）→ 后端重置为全新明文空库，旧库保留密文副本；重置成功即
 * 翻转锁定门，主界面随全新空库挂载，无需重启。
 *
 * 无需注册 Overlay Suppression：本屏挂载期间侧栏/视图/快捷键宿主全部
 * 不存在，无被抑制对象；ESC 守卫由 useWindowGuard 的全局 preventDefault
 * 覆盖，不存在「ESC 关掉解锁屏」的通路。「本机记住」由后续票交付。
 */
import { NButton, NCard, NInput, NSpace, NText, useMessage } from 'naive-ui'
import { confirm } from '@tauri-apps/plugin-dialog'
import { ref } from 'vue'
import { t } from '@/i18n'
import { useEncryptionGate } from '@/composables/useEncryptionGate'
import { errorMessage } from '@/utils/errors'
import { restartAppShortly } from '@/utils/restart'

const { unlock, reset } = useEncryptionGate()
const message = useMessage()

const passphrase = ref('')
const submitting = ref(false)
const resetting = ref(false)
const errorText = ref('')

async function submit() {
  if (submitting.value) return
  errorText.value = ''
  submitting.value = true
  try {
    const relocated = await unlock(passphrase.value)
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

/** 忘记口令重置：后果说明（无后门、不可恢复）→ 二次确认 → 重置为全新明文空库。
 *  取消或失败都留在解锁屏，可继续尝试口令或再次进入。 */
async function forgotPassphrase() {
  if (resetting.value) return
  errorText.value = ''
  const ok = await confirm(t('unlock.resetBody'), {
    title: t('unlock.resetTitle'),
    kind: 'warning',
    okLabel: t('unlock.resetConfirm'),
    cancelLabel: t('unlock.resetCancel'),
  })
  if (!ok) return
  resetting.value = true
  try {
    await reset()
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
        <NButton type="primary" block :loading="submitting" :disabled="!passphrase" @click="submit">
          {{ t('unlock.button') }}
        </NButton>
        <!-- 忘记口令入口（issue #573）：常驻可达的逃生门，无后门后果说明后二次确认 -->
        <NButton quaternary size="small" :disabled="resetting" @click="forgotPassphrase">
          {{ t('unlock.forgot') }}
        </NButton>
      </NSpace>
    </NCard>
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

.unlock-error {
  /* 独占一行，重试提示完整可见 */
  align-self: flex-start;
}
</style>
