<script setup lang="ts">
import { NAlert, NButton, NCard, NInput, NSpace, NSpin, NText, useMessage } from 'naive-ui'
import { computed, onMounted, ref } from 'vue'
import { api } from '@/api'
import { t } from '@/i18n'
import { errorMessage } from '@/utils/errors'
import { formatIsoMinute } from '@/utils/datetime'
import { restartAppShortly } from '@/utils/restart'
import AppModal from '@/components/AppModal.vue'
import type {
  ParkedOpInfo,
  SyncChannelConfig,
  SyncCheckpointInfo,
  SyncStatus,
} from '@/types'

// 多端同步卡片（issue #862 / #863 / #864 / ADR-0091）：设置页「数据」Tab 的同步可见面——
// 上次同步时间、挂起数量、「立即同步」动作、挂起通知明细、通道配置表单
//（WebDAV 凭据）与检查点发布/引导（新端加入向导）。显示值一律来自命令返回
//（AppSettings 权威），不走 localStorage；通道配置是本机设备配置（不同步）。
// 明文模式的显著提示是 ADR-0091 决策 8 的界面义务：未开加密时同步数据明文上
// 通道，警示常驻卡片。
//
// 自动触发（打开应用即同步 + 运行期低频轮询）由后端编排，前端零调用面；本卡片
// 只呈现「同步到什么状态」。挂起通知（issue #863 验收项）：数量 > 0 时展开明细，
// 逐条经 `utils/errors.ts` 的 `errorMessage` 按码本地化（含 params 插值；码未命中
// 或 params 不足则降级透传后端原文），供用户知道哪些操作待裁决——挂起原因
// 的码→文案实现单点在 errors.ts，此处不自建（issue #957）。
//
// 检查点（issue #864）：存量数据的设备先「发布检查点」把全量快照放上通道；
// 全新设备（典型是手机）经「从通道引导本端」预检 → 确认 → 整库换入 → 重启。
// 引导是 ADR-0098 决策 5 钉死的显式向导动作（不挂自动轮次），确认步展示
// 「整库替换 + 重启」后果；成功后经 restartAppShortly 原位重引导（Restore
// 同型，Android 上表现为应用退出重开，ADR-0074 决策 6 既有差异）。

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

// ---------------------------------------------------------------------------
// 检查点发布与新端引导（issue #864）：命令面 get_sync_channel_checkpoint /
// publish_sync_checkpoint / bootstrap_sync_from_channel；引导是显式向导动作
//（ADR-0098 决策 5），确认步展示整库替换与重启后果，成功即原位重引导。
// ---------------------------------------------------------------------------

/** 快照体大小展示文本（字节 → MB，一位小数；仅向导展示用）。 */
function formatSizeMb(bytes: number): string {
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

const publishing = ref(false)

/** 发布检查点到通道：存量数据的设备把「新端可引导的来源」放上通道。 */
async function publishCheckpoint() {
  publishing.value = true
  try {
    const result = await api.publishSyncCheckpoint(passphrase.value || undefined)
    message.success(
      t('settings.data.sync.publishOk', {
        generation: result.generation,
        size: formatSizeMb(result.size),
      }),
    )
  } catch (e: any) {
    message.error(t('settings.data.sync.publishFailed', { msg: errorMessage(e) }))
  } finally {
    publishing.value = false
  }
}

// 引导向导状态：预检三态（loading / found + 指针 / none）+ 口令 + 提交中。
const bootstrapShow = ref(false)
const prechecking = ref(false)
const checkpointInfo = ref<SyncCheckpointInfo | null>(null)
const precheckFailed = ref('')
const bootstrapPassphrase = ref('')
const bootstrapping = ref(false)

/** 打开引导向导并预检通道（只读 manifest，不下载快照体）。 */
async function openBootstrap() {
  bootstrapShow.value = true
  bootstrapPassphrase.value = ''
  checkpointInfo.value = null
  precheckFailed.value = ''
  prechecking.value = true
  try {
    checkpointInfo.value = await api.getSyncChannelCheckpoint()
  } catch (e: any) {
    precheckFailed.value = errorMessage(e)
  } finally {
    prechecking.value = false
  }
}

/** 确认引导：整库换入通道快照，成功后原位重引导（Restart 同型，重启载入数据）。 */
async function confirmBootstrap() {
  if (!checkpointInfo.value || bootstrapping.value) return
  bootstrapping.value = true
  try {
    const outcome = await api.bootstrapSyncFromChannel(bootstrapPassphrase.value || undefined)
    bootstrapShow.value = false
    message.success(
      t('settings.data.sync.bootstrapOk', {
        generation: outcome.generation,
        size: formatSizeMb(outcome.size),
        reencrypted: outcome.reencrypted ? t('settings.data.sync.bootstrapReencrypted') : '',
      }),
    )
    restartAppShortly()
  } catch (e: any) {
    // 引导失败弹窗保持打开：口令错误/形态不一致可就地修正重试（Restore 同语义）。
    message.error(t('settings.data.sync.bootstrapFailed', { msg: errorMessage(e) }))
  } finally {
    bootstrapping.value = false
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
          {{ t('settings.data.sync.parkedItem', { reason: errorMessage(op) }) }}
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

      <!-- 检查点与新端加入（issue #864）：存量数据设备发布快照，全新设备引导加入。 -->
      <NText strong>{{ t('settings.data.sync.checkpointTitle') }}</NText>
      <NText depth="3" style="font-size: 12px">{{ t('settings.data.sync.checkpointHint') }}</NText>
      <NSpace>
        <NButton
          :loading="publishing"
          :disabled="!status?.channel_configured"
          data-testid="sync-publish-checkpoint"
          @click="publishCheckpoint"
        >
          {{ t('settings.data.sync.publishCheckpoint') }}
        </NButton>
        <NButton
          :disabled="!status?.channel_configured"
          data-testid="sync-bootstrap"
          @click="openBootstrap"
        >
          {{ t('settings.data.sync.bootstrapTitle') }}
        </NButton>
      </NSpace>
    </NSpace>

    <!-- 新端引导向导（issue #864）：预检 → 后果确认 → 口令（密文快照）→ 整库换入 →
         重启。失败弹窗保持打开（口令/形态问题就地重试，Restore 同语义）；弹层纪律
         与桌面/移动档形态由 AppModal 收口。 -->
    <AppModal
      v-model:show="bootstrapShow"
      preset="card"
      card-size="sm"
      :title="t('settings.data.sync.bootstrapModalTitle')"
    >
      <NSpace vertical :size="12" data-testid="sync-bootstrap-modal">
        <NAlert type="warning" :show-icon="true" data-testid="sync-bootstrap-warning">
          {{ t('settings.data.sync.bootstrapWarning') }}
        </NAlert>
        <NSpace v-if="prechecking" vertical :size="8">
          <NSpin size="small" />
          <NText depth="3">{{ t('settings.data.sync.bootstrapPrechecking') }}</NText>
        </NSpace>
        <template v-else>
          <NAlert v-if="precheckFailed" type="error" :show-icon="true">
            {{ t('settings.data.sync.bootstrapPrecheckFailed', { msg: precheckFailed }) }}
          </NAlert>
          <NAlert v-else-if="!checkpointInfo" type="info" :show-icon="true">
            {{ t('settings.data.sync.bootstrapNotFound') }}
          </NAlert>
          <NText v-else data-testid="sync-bootstrap-found">
            {{
              t('settings.data.sync.bootstrapFound', {
                generation: checkpointInfo.generation,
                size: formatSizeMb(checkpointInfo.size),
              })
            }}
          </NText>
        </template>
        <NInput
          v-model:value="bootstrapPassphrase"
          type="password"
          show-password-on="click"
          :placeholder="t('settings.data.sync.bootstrapPassphrasePlaceholder')"
          :disabled="bootstrapping || !checkpointInfo"
          data-testid="sync-bootstrap-passphrase"
          @keyup.enter="confirmBootstrap"
        />
        <NSpace justify="end">
          <NButton
            :disabled="bootstrapping"
            data-testid="sync-bootstrap-cancel"
            @click="bootstrapShow = false"
          >
            {{ t('settings.data.sync.bootstrapCancel') }}
          </NButton>
          <NButton
            type="warning"
            :loading="bootstrapping"
            :disabled="!checkpointInfo || bootstrapping"
            data-testid="sync-bootstrap-confirm"
            @click="confirmBootstrap"
          >
            {{ t('settings.data.sync.bootstrapConfirm') }}
          </NButton>
        </NSpace>
      </NSpace>
    </AppModal>
  </NCard>
</template>
