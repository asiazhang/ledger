<script setup lang="ts">
import { NAlert, NButton, NCard, NInput, NSpace, NSpin, NSwitch, NTag, NText } from "naive-ui";
import { t } from "@ledger/i18n";
import { errorMessage } from "@ledger/utils/errors";
import { vendorTierKey } from "@ledger/utils/s3-vendors";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import { SYNC_HINT_CLASS } from "@/settings/sync-settings.css";
import { useSyncCard } from "@/settings/useSyncCard";

// 多端同步卡片（issue #862 / #863 / #864 / #1218 / ADR-0091）：设置页「数据」
// Tab 的同步可见面——上次同步时间、挂起数量、「立即同步」动作、挂起通知明细、
// 通道配置表单（S3 凭据，issue #1218）与检查点发布/引导（新端加入向导）。
//
// 本组件是 useSyncCard 深模块（issue #1397）的薄 adapter：加载与动作编排
// （状态 / 挂起明细 / 通道配置 / 检查点 / 引导向导）全部内化在模块中，这里只做
// 渲染接线——明文/密文警示 Alert（ADR-0091 决策 8 的界面义务）、引导向导的
// AppModal 模板与文案留在组件；挂起明细逐条经 `utils/errors.ts` 的 `errorMessage`
// 按码本地化（含 params 插值；码未命中或 params 不足则降级透传后端原文），挂起
// 原因的码→文案实现单点在 errors.ts，此处不自建（issue #957）。
//
// 密钥输入框恒以空串起填、已保存值不上屏的回显口径见 useSyncCard 头注；本组件
// 只负责把 secretKeyInput 绑上输入框。

const {
  status,
  statusLoading,
  lastSyncText,
  parkedOps,
  passphrase,
  syncing,
  syncNow,
  form,
  secretKeyInput,
  secretKeyPlaceholder,
  selectedVendor,
  selectedVendorPreset,
  vendorSelectOptions,
  onVendorChange,
  applyVendorRegion,
  testing,
  testConnection,
  saving,
  saveChannel,
  publishing,
  publishCheckpoint,
  bootstrapShow,
  prechecking,
  precheckError,
  checkpointInfo,
  bootstrapPassphrase,
  bootstrapping,
  openBootstrap,
  confirmBootstrap,
} = useSyncCard();

/** 快照体大小展示文本（字节 → MB，一位小数；向导回显渲染用——toast 插值助手的
 *  同名实现 module 私有，文案与模板留组件故此处保留渲染侧一份，口径一致）。 */
function formatSizeMb(bytes: number): string {
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
</script>

<template>
  <NCard :title="t('settings.data.sync.cardTitle')" size="small">
    <NSpace vertical :size="16">
      <!-- 明文模式显著提示（ADR-0091 决策 8）：密文库不显示。 -->
      <NAlert v-if="status && !status.library_encrypted" type="warning" :show-icon="true">
        {{ t("settings.data.sync.plaintextWarning") }}
      </NAlert>
      <NAlert v-else-if="status && status.library_encrypted" type="info" :show-icon="true">
        {{ t("settings.data.sync.encryptedHint") }}
      </NAlert>

      <!-- 状态区：上次同步时间 + 挂起数量 + 立即同步。 -->
      <NSpace vertical :size="4">
        <NText data-testid="sync-last-time">
          {{ t("settings.data.sync.lastSyncAt") }}{{ t("settings.data.sync.colon")
          }}{{ lastSyncText }}
        </NText>
        <NText data-testid="sync-parked-count">
          {{ t("settings.data.sync.parkedCount") }}{{ t("settings.data.sync.colon")
          }}{{ status?.parked_count ?? 0 }}
        </NText>
        <NText
          v-if="status && status.parked_count > 0"
          depth="3"
          style="font-size: 12px"
          data-testid="sync-parked-hint"
        >
          {{ t("settings.data.sync.parkedHint") }}
        </NText>
      </NSpace>

      <!-- 挂起通知明细（issue #863 验收项）：逐条按码化原因本地化呈现。 -->
      <NSpace v-if="parkedOps.length > 0" vertical :size="4" data-testid="sync-parked-list">
        <NText strong>{{ t("settings.data.sync.parkedListTitle") }}</NText>
        <NText
          v-for="op in parkedOps"
          :key="op.op_id"
          depth="3"
          style="font-size: 12px"
          data-testid="sync-parked-item"
        >
          {{ t("settings.data.sync.parkedItem", { reason: errorMessage(op) }) }}
        </NText>
      </NSpace>
      <NSpace>
        <NButton
          type="primary"
          :loading="syncing"
          :disabled="statusLoading"
          data-testid="sync-now"
          @click="syncNow"
        >
          {{ t("settings.data.sync.syncNow") }}
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

      <!-- 通道配置表单（S3 七字段 + 同步空间，issue #1218 / #1219 / #1220）：
           配置面没有后端判别字段（WebDAV 已随 #1221 退役）；厂商预设下拉只预填、
           不落库（issue #1220），「测试连接」按钮在此（issue #1219）。 -->
      <NText strong>{{ t("settings.data.sync.channelTitle") }}</NText>
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
          {{ t("settings.data.sync.vendorHint") }}
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
              {{ t("settings.data.sync.vendorDocs") }}
            </NButton>
          </NSpace>
          <NSpace align="center" :size="8" data-testid="sync-vendor-regions">
            <NText depth="3" :class="SYNC_HINT_CLASS">
              {{ t("settings.data.sync.vendorRegionLabel") }}
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
          <NText depth="3" style="font-size: 12px">{{
            t("settings.data.sync.pathStyleLabel")
          }}</NText>
        </NSpace>
        <NInput
          v-model:value="form.space_id"
          :placeholder="t('settings.data.sync.spacePlaceholder')"
          data-testid="sync-space"
        />
        <NText depth="3" style="font-size: 12px">{{ t("settings.data.sync.spaceHint") }}</NText>
        <NSpace>
          <!-- 保存前「测试连接」（issue #1219）：探测用的是当前表单而非落库配置，
                用户可以先把厂商预设/地域/手改填好，再确认这份凭据、桶与网络可用。 -->
          <NButton :loading="testing" data-testid="sync-test-connection" @click="testConnection">
            {{ t("settings.data.sync.testConnection") }}
          </NButton>
          <NButton :loading="saving" data-testid="sync-save-channel" @click="saveChannel">
            {{ t("settings.data.sync.saveChannel") }}
          </NButton>
        </NSpace>
      </NSpace>

      <!-- 检查点与新端加入（issue #864）：存量数据设备发布快照，全新设备引导加入。 -->
      <NText strong>{{ t("settings.data.sync.checkpointTitle") }}</NText>
      <NText depth="3" style="font-size: 12px">{{ t("settings.data.sync.checkpointHint") }}</NText>
      <NSpace>
        <NButton
          :loading="publishing"
          :disabled="!status?.channel_configured"
          data-testid="sync-publish-checkpoint"
          @click="publishCheckpoint"
        >
          {{ t("settings.data.sync.publishCheckpoint") }}
        </NButton>
        <NButton
          :disabled="!status?.channel_configured"
          data-testid="sync-bootstrap"
          @click="openBootstrap"
        >
          {{ t("settings.data.sync.bootstrapTitle") }}
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
          {{ t("settings.data.sync.bootstrapWarning") }}
        </NAlert>
        <NSpace v-if="prechecking" vertical :size="8">
          <NSpin size="small" />
          <NText depth="3">{{ t("settings.data.sync.bootstrapPrechecking") }}</NText>
        </NSpace>
        <template v-else>
          <NAlert v-if="precheckError" type="error" :show-icon="true">
            {{ t("settings.data.sync.bootstrapPrecheckFailed", { msg: precheckError }) }}
          </NAlert>
          <NAlert v-else-if="!checkpointInfo" type="info" :show-icon="true">
            {{ t("settings.data.sync.bootstrapNotFound") }}
          </NAlert>
          <NText v-else data-testid="sync-bootstrap-found">
            {{
              t("settings.data.sync.bootstrapFound", {
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
            {{ t("settings.data.sync.bootstrapCancel") }}
          </NButton>
          <NButton
            type="warning"
            :loading="bootstrapping"
            :disabled="!checkpointInfo || bootstrapping"
            data-testid="sync-bootstrap-confirm"
            @click="confirmBootstrap"
          >
            {{ t("settings.data.sync.bootstrapConfirm") }}
          </NButton>
        </NSpace>
      </NSpace>
    </AppModal>
  </NCard>
</template>
