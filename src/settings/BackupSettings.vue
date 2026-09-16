<script setup lang="ts">
import { computed, h, ref, watch } from 'vue'
import {
  NAlert,
  NButton,
  NCard,
  NDataTable,
  NIcon,
  NInputNumber,
  NSpace,
  NSwitch,
  NText,
} from 'naive-ui'
import { LockClosedOutline } from '@vicons/ionicons5'
import { useAppStore } from '@/stores/app'
import { useBackup } from '@/backup/useBackup'
import { t } from '@ledger/i18n'
import AppDangerConfirmModal from '@ledger/ui-kit/AppDangerConfirmModal.vue'
import RestoreConfirmModal from '@/backup/RestoreConfirmModal.vue'
import { SETTINGS_CARD_STACK_CLASS } from '@/settings/settings-layout.css.ts'

const store = useAppStore()

const {
  backingUp,
  restoring,
  lastBackup,
  backups,
  pruning,
  backupRows,
  restoreIntent,
  restoreSeq,
  closeRestore,
  confirmRestore,
  pickBackupDir,
  clearBackupDir,
  onBackupMaxCountChange,
  manualPrune,
  pruneConfirmShow,
  pruneExcess,
  confirmPrune,
  cancelPrune,
  backupOnce,
  backupAs,
  pickRestore,
  revealInFinder,
  copyLastBackupPath,
  autoBackupEnabled,
  autoBackupLastText,
  toggleAutoBackup,
  refreshing,
  refreshList,
} = useBackup()

// 客户端切片分页（issue #1383）：备份列表是有界快照列表（受管产物受保留上限封顶，
// ADR-0008 分界的有界侧），行集一次全量拉取、翻页只是展示切片，不发数据请求。
// 页大小固定 10，无页大小选择器与快捷跳页；单页收起分页条（paginate-single-page，
// 持仓页签先例）。页码组件内持有、每次进入回第一页（商户管理表先例，对 ADR-0094
// 默认粒度的显式豁免）；数据重拉（新备份/清理/手动刷新）保持当前页——页码越界时
// 回落到有效范围，不落空页、不留陈旧页码（词条「回退不归零」等价形态）。
const BACKUP_PAGE_SIZE = 10
const currentPage = ref(1)

const maxPage = computed(() =>
  Math.max(1, Math.ceil(backups.value.length / BACKUP_PAGE_SIZE)),
)
watch(maxPage, (max) => {
  if (currentPage.value > max) currentPage.value = max
})

const pagination = computed(() => ({
  page: currentPage.value,
  pageSize: BACKUP_PAGE_SIZE,
  onChange: (next: number) => {
    currentPage.value = next
  },
}))

const backupColumns = [
  { title: () => t('settings.data.backup.columns.fileName'), key: 'file_name' },
  { title: () => t('settings.data.backup.columns.source'), key: 'source_text', width: 70 },
  // 加密列（issue #572）：密文备份显示锁形标记，明文/旧备份缺标记不显。
  {
    title: () => t('settings.data.backup.columns.encrypted'),
    key: 'encrypted',
    width: 56,
    render: (row: { encrypted: boolean }) =>
      row.encrypted
        ? h(
            NIcon,
            { title: t('settings.data.backup.encryptedLabel') },
            { default: () => h(LockClosedOutline) },
          )
        : null,
  },
  { title: () => t('settings.data.backup.columns.size'), key: 'size_text', width: 100 },
  { title: () => t('settings.data.backup.columns.time'), key: 'created_at', width: 160 },
  // 操作列（issue #653）：行级「在访达中显示」——系统文件管理器定位到该备份文件，
  // 免在目录里人工比对长文件名（界面文本不可选，显式通道；路径与文件名不给
  // 复制入口，分配见父 spec #649）。
  {
    title: () => t('settings.data.backup.columns.actions'),
    key: 'actions',
    width: 120,
    render: (row: { file_name: string; path: string }) =>
      h(
        NButton,
        {
          size: 'tiny',
          'data-testid': `backup-reveal-${row.file_name}`,
          onClick: () => revealInFinder(row.path),
        },
        { default: () => t('settings.data.backup.revealInFinder') },
      ),
  },
]
</script>

<template>
  <!-- 卡片顺序（issue #651）：备份（动作）→ 备份目录 → 自动备份 → 备份文件列表 → 恢复，
       最常用的一键备份免滚动直达。单列，卡片随窗口宽度铺满内容区（issue #651 修订）。 -->
  <div>
    <div :class="SETTINGS_CARD_STACK_CLASS">
      <NCard :title="t('settings.data.backup.backupTitle')" size="small">
        <NSpace vertical :size="12">
          <NText depth="3">
            {{ t('settings.data.backup.backupHint') }}
          </NText>
          <NSpace align="center" :size="12">
            <NButton type="primary" :loading="backingUp" @click="backupOnce">{{ t('settings.data.backup.backupOnce') }}</NButton>
            <NButton :loading="backingUp" @click="backupAs">{{ t('settings.data.backup.backupAs') }}</NButton>
          </NSpace>
          <NSpace v-if="lastBackup" align="center" :size="8">
            <NText type="success" style="word-break: break-all">
              {{ t('settings.data.backup.lastBackup') }}{{ lastBackup }}
            </NText>
            <!-- 复制完整路径（issue #653）：展示文案含大小括注，复制的是原始完整路径 -->
            <NButton size="small" data-testid="copy-last-backup-path" @click="copyLastBackupPath">
              {{ t('settings.data.backup.copyPath') }}
            </NButton>
          </NSpace>
        </NSpace>
      </NCard>

      <NCard :title="t('settings.data.backup.dirTitle')" size="small">
        <NSpace vertical :size="12">
          <NText depth="3">
            {{ t('settings.data.backup.dirHint') }}
          </NText>
          <NSpace align="center" :size="12">
            <NText style="word-break: break-all">
              {{ store.backupDir || t('settings.data.backup.dirUnset') }}
            </NText>
            <NButton size="small" @click="pickBackupDir">
              {{ store.backupDir ? t('settings.data.backup.changeDir') : t('settings.data.backup.chooseDir') }}
            </NButton>
            <NButton v-if="store.backupDir" size="small" quaternary type="error" @click="clearBackupDir">
              {{ t('settings.data.backup.clear') }}
            </NButton>
          </NSpace>
          <NSpace align="center" :size="12">
            <NText>{{ t('settings.data.backup.keepLimitLabel') }}</NText>
            <NInputNumber
              :value="store.backupMaxCount"
              :min="1"
              :max="100"
              :update-value-on-input="false"
              style="max-width: 120px"
              @update:value="onBackupMaxCountChange"
            />
            <NText depth="3">{{ t('settings.data.backup.keepLimitSuffix') }}</NText>
          </NSpace>
        </NSpace>
      </NCard>

      <NCard :title="t('settings.data.backup.autoTitle')" size="small">
        <NSpace vertical :size="12">
          <NSpace align="center" :size="12">
            <NSwitch
              :value="autoBackupEnabled"
              @update:value="toggleAutoBackup"
            />
            <NText>{{ t('settings.data.backup.autoSwitchLabel') }}</NText>
          </NSpace>
          <NText depth="3">{{ t('settings.data.backup.autoLast') }}{{ autoBackupLastText }}</NText>
          <NText v-if="!store.backupDir" type="warning">
            {{ t('settings.data.backup.autoNeedDir') }}
          </NText>
        </NSpace>
      </NCard>

      <NCard :title="t('settings.data.backup.listTitle')" size="small">
        <!-- 手动刷新（issue #651）：文件管理器手动增删文件后使列表与磁盘一致。 -->
        <template #header-extra>
          <NButton
            size="small"
            :loading="refreshing"
            data-testid="backup-list-refresh"
            @click="refreshList"
          >
            {{ t('settings.data.backup.refresh') }}
          </NButton>
        </template>
        <NSpace vertical :size="12">
          <!-- 备份按账本分域（issue #836）：列表只呈现当前账本的产物。 -->
          <NText depth="3">{{ t("settings.data.backup.listHint") }}</NText>
          <NSpace align="center" justify="space-between" style="width: 100%">
            <NText depth="3">{{ t('settings.data.backup.count', { n: backups.length, max: store.backupMaxCount }) }}</NText>
            <NButton
              size="small"
              type="warning"
              secondary
              :disabled="backups.length === 0 || pruning"
              :loading="pruning"
              @click="manualPrune"
            >
              {{ t('settings.data.backup.pruneNow') }}
            </NButton>
          </NSpace>
          <NDataTable
            :columns="backupColumns"
            :data="backupRows"
            :bordered="false"
            size="small"
            :pagination="pagination"
            :paginate-single-page="false"
            :empty="store.backupDir ? t('settings.data.backup.emptyWithDir') : t('settings.data.backup.emptyNoDir')"
          />
        </NSpace>
      </NCard>

      <NCard :title="t('settings.data.backup.restoreTitle')" size="small">
        <!-- 破坏性警示升为显著警示块（issue #651 / ADR-0078 决策 4）：入口按钮降为
             default 形态，红色语义由警示块与恢复确认弹窗承载；恢复前安全备份＋
             确认弹窗双闸不变。 -->
        <NSpace vertical :size="12">
          <NAlert type="error" :show-icon="true">
            {{ t('settings.data.backup.restoreHintBefore') }}<strong>{{ t('settings.data.backup.restoreHintStrong') }}</strong>{{ t('settings.data.backup.restoreHintAfter') }}
          </NAlert>
          <NButton :loading="restoring" @click="pickRestore">{{ t('settings.data.backup.restoreButton') }}</NButton>
        </NSpace>
      </NCard>

    </div>

    <!-- 恢复确认弹窗（issue #572）：跨模式警告 + 密文备份主口令，失败可就地重试 -->
    <RestoreConfirmModal
      :intent="restoreIntent"
      :seq="restoreSeq"
      :on-confirm="confirmRestore"
      @close="closeRestore"
    />

    <!-- 手动清理确认弹窗（issue #652 / ADR-0078）：warning 级——删除的是可再生
         备份产物（有兜底），待删数量与不可恢复后果显式呈现；取消零副作用 -->
    <AppDangerConfirmModal
      level="warning"
      v-model:show="pruneConfirmShow"
      :title="t('settings.data.msg.pruneConfirmTitle')"
      :strong-warning="t('settings.data.msg.pruneConfirmStrong', { n: pruneExcess })"
      :detail="t('settings.data.msg.pruneConfirmDetail')"
      :confirm-text="t('settings.data.msg.pruneConfirmOk')"
      :cancel-text="t('settings.data.msg.pruneConfirmCancel')"
      :submitting="pruning"
      :on-confirm="confirmPrune"
      :on-cancel="cancelPrune"
    />
  </div>
</template>
