<script setup lang="ts">
import { NAlert, NButton, NCard, NInput, NSpace, NText, useMessage } from 'naive-ui'
import { computed, onMounted, ref } from 'vue'
import { api } from '@/api'
import { t } from '@/i18n'
import { errorMessage } from '@/utils/errors'
import { formatIsoMinute } from '@/utils/datetime'
import type { ParkedOpInfo, SyncChannelConfig, SyncStatus } from '@/types'

// 多端同步卡片（issue #862 / #863 / ADR-0091）：设置页「数据」Tab 的同步可见面——
// 上次同步时间、挂起数量、「立即同步」动作、挂起通知明细与通道配置表单
//（WebDAV 凭据）。显示值一律来自命令返回（AppSettings 权威），不走 localStorage；
// 通道配置是本机设备配置（不同步）。明文模式的显著提示是 ADR-0091 决策 8 的界面
// 义务：未开加密时同步数据明文上通道，警示常驻卡片。
//
// 自动触发（打开应用即同步 + 运行期低频轮询）由后端编排，前端零调用面；本卡片
// 只呈现「同步到什么状态」。挂起通知（issue #863 验收项）：数量 > 0 时展开明细，
// 逐条按码化原因本地化呈现（`errors.<code>`），供用户知道哪些操作待裁决。

const message = useMessage()

const status = ref<SyncStatus | null>(null)
const loading = ref(false)
const syncing = ref(false)
const saving = ref(false)
// 口令输入：仅密文库需要（留空则后端回退本机已记住口令）；不落任何本地存储。
const passphrase = ref('')
// 挂起操作明细（issue #863 挂起通知）：数量 > 0 时按需拉取，展示码化原因。
const parkedOps = ref<ParkedOpInfo[]>([])

// 通道配置表单：初值来自命令回显（未配置为空表单，空间字段填默认值）。
const form = ref<SyncChannelConfig>({
  base_url: '',
  username: '',
  password: '',
  space_id: 'default',
  configured: false,
})

async function refreshStatus() {
  loading.value = true
  try {
    status.value = await api.getSyncStatus()
    await refreshParkedOps()
  } catch (e: any) {
    message.error(t('settings.data.sync.loadFailed', { msg: errorMessage(e) }))
  } finally {
    loading.value = false
  }
}

/** 拉取挂起明细（issue #863）：仅当数量 > 0 时调用，避免无谓 IPC。 */
async function refreshParkedOps() {
  if (!status.value || status.value.parked_count === 0) {
    parkedOps.value = []
    return
  }
  try {
    parkedOps.value = await api.getParkedOps()
  } catch (e: any) {
    // 明细拉取失败不升级为卡片级错误：数量仍由状态回显，重试即下次刷新。
    console.warn('挂起明细拉取失败', e)
    parkedOps.value = []
  }
}

async function refreshChannelConfig() {
  try {
    const config = await api.getSyncChannelConfig()
    form.value = config.configured
      ? config
      : { ...config, space_id: 'default' }
  } catch (e: any) {
    message.error(t('settings.data.sync.loadFailed', { msg: errorMessage(e) }))
  }
}

onMounted(async () => {
  await Promise.all([refreshStatus(), refreshChannelConfig()])
})

/** 上次同步时刻展示文本（ISO → 本地可读截断，单一格式化点 utils/datetime）。 */
const lastSyncText = computed(() =>
  status.value?.last_sync_at
    ? formatIsoMinute(status.value.last_sync_at)
    : t('settings.data.sync.neverSynced'),
)

/** 立即同步：手动触发一轮同步，成功轻量提示轮次报告并刷新状态。 */
async function syncNow() {
  syncing.value = true
  try {
    const report = await api.syncNow(passphrase.value || undefined)
    message.success(
      t('settings.data.sync.syncOk', {
        uploaded: report.uploaded_ops,
        applied: report.applied,
        parked: report.parked,
      }),
    )
    await refreshStatus()
    if (report.parked > 0) {
      message.warning(
        t('settings.data.sync.parkedToast', { count: report.parked }),
      )
    }
  } catch (e: any) {
    message.error(t('settings.data.sync.syncFailed', { msg: errorMessage(e) }))
  } finally {
    syncing.value = false
  }
}

/** 挂起原因展示文本：码化原因按当前语言模板本地化，未知码降级透传原文。 */
function parkedReason(op: ParkedOpInfo): string {
  const key = `errors.${op.code}`
  return t(key) !== key ? t(key) : op.message
}

/** 保存通道配置：WebDAV 凭据与同步空间（跨端共识的世界身份；空值交由后端回默认）。 */
async function saveChannel() {
  saving.value = true
  try {
    await api.setSyncChannelConfig({
      base_url: form.value.base_url,
      username: form.value.username,
      password: form.value.password,
      space_id: form.value.space_id.trim() || undefined,
    })
    message.success(t('settings.data.sync.saveOk'))
    await refreshStatus()
  } catch (e: any) {
    message.error(t('settings.data.sync.saveFailed', { msg: errorMessage(e) }))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <NCard :title="t('settings.data.sync.cardTitle')" size="small">
    <NSpace vertical :size="16">
      <!-- 明文模式显著提示（ADR-0091 决策 8）：密文库不显示。 -->
      <NAlert v-if="status && !status.library_encrypted" type="warning" :show-icon="true">
        {{ t('settings.data.sync.plaintextWarning') }}
      </NAlert>
      <NAlert v-else-if="status && status.library_encrypted" type="info" :show-icon="true">
        {{ t('settings.data.sync.encryptedHint') }}
      </NAlert>

      <!-- 状态区：上次同步时间 + 挂起数量 + 立即同步。 -->
      <NSpace vertical :size="4">
        <NText data-testid="sync-last-time">
          {{ t('settings.data.sync.lastSyncAt') }}{{ t('settings.data.sync.colon') }}{{ lastSyncText }}
        </NText>
        <NText data-testid="sync-parked-count">
          {{ t('settings.data.sync.parkedCount') }}{{ t('settings.data.sync.colon') }}{{ status?.parked_count ?? 0 }}
        </NText>
        <NText
          v-if="status && status.parked_count > 0"
          depth="3"
          style="font-size: 12px"
          data-testid="sync-parked-hint"
        >
          {{ t('settings.data.sync.parkedHint') }}
        </NText>
      </NSpace>

      <!-- 挂起通知明细（issue #863 验收项）：逐条按码化原因本地化呈现。 -->
      <NSpace
        v-if="parkedOps.length > 0"
        vertical
        :size="4"
        data-testid="sync-parked-list"
      >
        <NText strong>{{ t('settings.data.sync.parkedListTitle') }}</NText>
        <NText
          v-for="op in parkedOps"
          :key="op.op_id"
          depth="3"
          style="font-size: 12px"
          data-testid="sync-parked-item"
        >
          {{ t('settings.data.sync.parkedItem', { reason: parkedReason(op) }) }}
        </NText>
      </NSpace>
      <NSpace>
        <NButton
          type="primary"
          :loading="syncing"
          :disabled="loading"
          data-testid="sync-now"
          @click="syncNow"
        >
          {{ t('settings.data.sync.syncNow') }}
        </NButton>
      </NSpace>
      <NInput
        v-if="status && status.library_encrypted"
        v-model:value="passphrase"
        type="password"
        :placeholder="t('settings.data.sync.passphrasePlaceholder')"
        show-password-on="click"
        data-testid="sync-passphrase"
        style="max-width: 320px"
      />

      <!-- 通道配置表单：WebDAV 凭据 + 同步空间。 -->
      <NText strong>{{ t('settings.data.sync.channelTitle') }}</NText>
      <NSpace vertical :size="8">
        <NInput
          v-model:value="form.base_url"
          :placeholder="t('settings.data.sync.urlPlaceholder')"
          data-testid="sync-url"
        />
        <NInput
          v-model:value="form.username"
          :placeholder="t('settings.data.sync.usernamePlaceholder')"
          data-testid="sync-username"
        />
        <NInput
          v-model:value="form.password"
          type="password"
          show-password-on="click"
          :placeholder="t('settings.data.sync.passwordPlaceholder')"
          data-testid="sync-password"
        />
        <NInput
          v-model:value="form.space_id"
          :placeholder="t('settings.data.sync.spacePlaceholder')"
          data-testid="sync-space"
        />
        <NText depth="3" style="font-size: 12px">{{ t('settings.data.sync.spaceHint') }}</NText>
        <NButton
          :loading="saving"
          data-testid="sync-save-channel"
          @click="saveChannel"
        >
          {{ t('settings.data.sync.saveChannel') }}
        </NButton>
      </NSpace>
    </NSpace>
  </NCard>
</template>
