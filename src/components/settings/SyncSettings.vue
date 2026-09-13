<script setup lang="ts">
import {
  NAlert,
  NButton,
  NCard,
  NInput,
  NSpace,
  NSpin,
  NSwitch,
  NTag,
  NText,
  useMessage,
} from 'naive-ui'
import { computed, onMounted, ref } from 'vue'
import { api } from '@ledger/api'
import { t } from '@ledger/i18n'
import { errorMessage } from '@/utils/errors'
import { formatIsoMinute } from '@/utils/datetime'
import { restartAppShortly } from '@/utils/restart'
import {
  CUSTOM_VENDOR_ID,
  findVendorPreset,
  matchVendorByEndpoint,
  vendorOptions,
  vendorPrefill,
  vendorTierKey,
  type S3VendorPrefill,
} from '@/utils/s3-vendors'
import AppModal from '@/components/AppModal.vue'
import AppSelect from '@/components/AppSelect.vue'
import { SYNC_HINT_CLASS } from '@/components/settings/sync-settings.css'
import type { ParkedOpInfo, SyncChannelConfig, SyncCheckpointInfo, SyncStatus } from '@ledger/types'

// 多端同步卡片（issue #862 / #863 / #864 / #1218 / ADR-0091）：设置页「数据」
// Tab 的同步可见面——上次同步时间、挂起数量、「立即同步」动作、挂起通知明细、
// 通道配置表单（S3 凭据，issue #1218）与检查点发布/引导（新端加入向导）。
// 显示值一律来自命令返回（AppSettings 权威），不走 localStorage；通道配置是
// 本机设备配置（不同步）。
// 明文模式的显著提示是 ADR-0091 决策 8 的界面义务：未开加密时同步数据明文上
// 通道，警示常驻卡片。
//
// 密钥回显口径（#1218 验收「加载时不回显完整密钥」）：命令面照旧回显
// `secret_key`（「不改密钥直接保存」需要它），界面层不把该值渲染进输入框——
// 密钥输入框恒以空串起填，已保存值只留在内存表单里；用户未改动即沿用旧值、
// 改动即提交新值。这样既不看走漏密钥，也不逼迫每次保存重输密钥。
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

// 通道配置表单（S3 七字段 + 同步空间，issue #1218 / #1220）：初值来自命令回显
//（未配置为空表单，空间字段填默认值），保存固定发 `backend: 's3'`——WebDAV
// 字段已从界面移除（后端字段与后端本体随 #1221 收口）。厂商预设下拉（issue
// #1220）只做界面预填与端点反查回显，不落库、不进后端契约；「测试连接」按钮
// 归 #1219。
//
// 表单只装本界面拥有的字段：命令回显形态 `SyncChannelConfig` 里被判别的 WebDAV
// 三字段没有输入面，却会随对象存进表单成为无人读的死状态，故回显时投影一次。
type ChannelForm = Pick<
  SyncChannelConfig,
  | 'space_id'
  | 'endpoint'
  | 'region'
  | 'bucket'
  | 'prefix'
  | 'access_key'
  | 'secret_key'
  | 'path_style'
>

const form = ref<ChannelForm>({
  space_id: 'default',
  endpoint: '',
  region: '',
  bucket: '',
  prefix: '',
  access_key: '',
  secret_key: '',
  path_style: false,
})

// 密钥输入缓冲（#1218 验收「加载时不回显完整密钥」）：输入框只绑本 ref，加载与
// 保存成功后一律清回空串——已保存密钥只活在 form.secret_key（内存），不上屏。
const secretKeyInput = ref('')

/** 密钥输入框占位：已保存过密钥时提示「留空则保持不变」，否则是普通字段名。 */
const secretKeyPlaceholder = computed(() =>
  form.value.secret_key
    ? t('settings.data.sync.secretKeySavedPlaceholder')
    : t('settings.data.sync.secretKeyPlaceholder'),
)

// 厂商预设（issue #1220）：用户选中的厂商判别键。它只是界面态——不随表单保存，
// 命令面 `SyncChannelConfig` 也没有厂商字段；「是谁」由端点反查决定（再次打开
// 或保存回显时按端点重算），所以这条状态不可能是落库数据的第二事实源。
const selectedVendor = ref<string>(CUSTOM_VENDOR_ID)

/** 当前选中厂商的预设（「其他（自定义）」或未知 id 为 null）。 */
const selectedVendorPreset = computed(() => findVendorPreset(selectedVendor.value))

/**
 * 下拉项：预设按声明序 + 末尾固定「其他（自定义）」（issue #1220 验收判据）。
 * 选项标签 = 厂商专名 + 档位标注（options 里的 name 不进翻译，档位文案经 i18n）。
 */
const vendorSelectOptions = computed(() =>
  vendorOptions().map((option) => ({
    value: option.id,
    label: option.custom
      ? t('settings.data.sync.vendorCustom')
      : t('settings.data.sync.vendorOption', {
          name: option.name,
          tier: t(vendorTierKey(option.verified)),
        }),
  })),
)

/**
 * 选中厂商：预填端点模板、默认地域与寻址方式（纯函数产出的值，本处只落表单）。
 * 「其他（自定义）」不预填——字段保持用户已填内容，等待用户自己写端点。
 */
function onVendorChange(vendorId: string) {
  selectedVendor.value = vendorId
  const prefill = vendorPrefill(vendorId)
  if (prefill) applyPrefill(prefill)
}

/** 常用地域快捷项：换地域即按当前厂商模板重写端点（字段随后仍可手改）。 */
function applyVendorRegion(region: string) {
  const prefill = vendorPrefill(selectedVendor.value, region)
  if (prefill) applyPrefill(prefill)
}

/** 预填值落进表单的单一落点（选中预填与地域快捷项共用，避免两处各写一遍字段）。 */
function applyPrefill(prefill: S3VendorPrefill) {
  form.value.endpoint = prefill.endpoint
  form.value.region = prefill.region
  form.value.path_style = prefill.pathStyle
}

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
    // 投影进表单（不持有回显对象本体）：表单的 v-model 会就地改写所绑对象，
    // 直接拿 IPC 契约快照当草稿纸用，等于把响应体当可变状态。
    form.value = {
      space_id: config.configured ? config.space_id : 'default',
      endpoint: config.endpoint,
      region: config.region,
      bucket: config.bucket,
      prefix: config.prefix,
      access_key: config.access_key,
      secret_key: config.secret_key,
      path_style: config.path_style,
    }
    // 密钥输入恒从空白起（不回显完整密钥）；已保存值留在 form 内供「留空沿用」。
    secretKeyInput.value = ''
    // 厂商回显按端点反查（issue #1220 验收判据）：命中厂商即回显该厂商，未命中
    //（自建服务、空表单、改过的端点）回「其他（自定义）」——不额外落库厂商字段。
    selectedVendor.value = matchVendorByEndpoint(form.value.endpoint)
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

/**
 * 保存通道配置（issue #1218）：S3 七字段与同步空间（跨端共识的世界身份；空值
 * 交由后端回默认），固定发 `backend: 's3'`。
 *
 * 密钥取值：输入框有内容（用户改过）用新值，为空则沿用内存里的已保存值——这是
 * 「加载时不回显完整密钥」前提下仍能「不改密钥直接保存」的机制。保存成功后重新
 * 回显，把落库结果（含后端归一化后的字段）呈现在表单上。
 */
async function saveChannel() {
  saving.value = true
  try {
    await api.setSyncChannelConfig({
      backend: 's3',
      space_id: form.value.space_id.trim() || undefined,
      endpoint: form.value.endpoint,
      region: form.value.region,
      bucket: form.value.bucket,
      prefix: form.value.prefix,
      access_key: form.value.access_key,
      secret_key: secretKeyInput.value !== '' ? secretKeyInput.value : form.value.secret_key,
      path_style: form.value.path_style,
    })
    message.success(t('settings.data.sync.saveOk'))
    await Promise.all([refreshStatus(), refreshChannelConfig()])
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
    if (result.plaintext_mode) {
      // 明文显著提示（ADR-0091 决策 8）：快照整库明文上通道，与常驻卡片警示同义。
      message.warning(t('settings.data.sync.publishPlaintextToast'))
    }
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

      <!-- 通道配置表单（S3 七字段 + 同步空间，issue #1218 / #1220）：WebDAV 字段
           已从界面移除（后端字段与后端本体随 #1221 收口）；厂商预设下拉只预填、
           不落库（issue #1220），「测试连接」按钮归 #1219。 -->
      <NText strong>{{ t('settings.data.sync.channelTitle') }}</NText>
      <NSpace vertical :size="8">
        <!-- 厂商预设（issue #1220）：末尾固定「其他（自定义）」；选中只预填，字段
             全部保持可编辑；档位标注与官方文档外链随所选厂商展示。选项是 6 项静态
             闭集，虚拟滚动无收益（关掉后选项全量进 DOM，利于无障碍与查找）。 -->
        <AppSelect
          :value="selectedVendor"
          :options="vendorSelectOptions"
          :virtual-scroll="false"
          data-testid="sync-vendor"
          @update:value="onVendorChange"
        />
        <NText depth="3" :class="SYNC_HINT_CLASS" data-testid="sync-vendor-hint">
          {{ t('settings.data.sync.vendorHint') }}
        </NText>
        <template v-if="selectedVendorPreset">
          <NSpace align="center" :size="8" data-testid="sync-vendor-meta">
            <NTag size="small" :bordered="false" data-testid="sync-vendor-tier">
              {{ t(vendorTierKey(selectedVendorPreset.verified)) }}
            </NTag>
            <NButton
              text
              tag="a"
              :href="selectedVendorPreset.docsUrl"
              target="_blank"
              rel="noreferrer"
              data-testid="sync-vendor-docs"
            >
              {{ t('settings.data.sync.vendorDocs') }}
            </NButton>
          </NSpace>
          <NSpace align="center" :size="8" data-testid="sync-vendor-regions">
            <NText depth="3" :class="SYNC_HINT_CLASS">
              {{ t('settings.data.sync.vendorRegionLabel') }}
            </NText>
            <NButton
              v-for="region in selectedVendorPreset.regions"
              :key="region"
              size="tiny"
              data-testid="sync-vendor-region"
              @click="applyVendorRegion(region)"
            >
              {{ region }}
            </NButton>
          </NSpace>
        </template>
        <NInput
          v-model:value="form.endpoint"
          :placeholder="t('settings.data.sync.endpointPlaceholder')"
          data-testid="sync-endpoint"
        />
        <NInput
          v-model:value="form.region"
          :placeholder="t('settings.data.sync.regionPlaceholder')"
          data-testid="sync-region"
        />
        <NInput
          v-model:value="form.bucket"
          :placeholder="t('settings.data.sync.bucketPlaceholder')"
          data-testid="sync-bucket"
        />
        <NInput
          v-model:value="form.prefix"
          :placeholder="t('settings.data.sync.prefixPlaceholder')"
          data-testid="sync-prefix"
        />
        <NInput
          v-model:value="form.access_key"
          :placeholder="t('settings.data.sync.accessKeyPlaceholder')"
          data-testid="sync-access-key"
        />
        <NInput
          v-model:value="secretKeyInput"
          type="password"
          show-password-on="click"
          :placeholder="secretKeyPlaceholder"
          data-testid="sync-secret-key"
        />
        <NSpace align="center" :size="8">
          <NSwitch v-model:value="form.path_style" data-testid="sync-path-style" />
          <NText depth="3" style="font-size: 12px">{{ t('settings.data.sync.pathStyleLabel') }}</NText>
        </NSpace>
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
