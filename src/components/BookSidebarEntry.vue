<script setup lang="ts">
import { watch } from 'vue'
import {
  NAlert,
  NButton,
  NForm,
  NFormItem,
  NIcon,
  NInput,
  NSpace,
  NTag,
  NText,
} from 'naive-ui'
import { BookOutline, ChevronUpOutline, CreateOutline, TrashOutline } from '@vicons/ionicons5'
import AppModal from '@/components/AppModal.vue'
import AppPopconfirm from '@/components/AppPopconfirm.vue'
import AppPopover from '@/components/AppPopover.vue'
import { useBookSwitcher } from '@/composables/useBookSwitcher'
import { t } from '@/i18n'

// 侧栏左下角账本入口与弹层（issue #834 / ADR-0089）：入口（当前账本名按钮 +
// 折叠态浮标）是 useBookSwitcher 深模块的薄适配器——清单渲染、切换确认、
// 新建/改名/移除编排全部内化在模块中，本组件只做渲染接线。
//
// 弹层纪律：清单弹层经 AppPopover（非模态面板，点外部关闭 + 开关上报弹层
// 注册表）；移除走行级气泡确认 AppPopconfirm（既有轻量形态，ADR-0078 范围
// 边界）；切换确认与命名弹窗见 useBookSwitcher / AppModal。

const props = defineProps<{
  /** 侧栏折叠态：折叠时入口切换为固定左下角的浮标图标形态（随时可达）。 */
  collapsed: boolean
}>()

const {
  books,
  activeId,
  activeBook,
  mutable,
  fallbackReason,
  loading,
  loadFailed,
  refresh,
  requestSwitch,
  panelShow,
  closePanel,
  nameIntent,
  nameDraft,
  nameBusy,
  nameModalTitle,
  nameModalConfirmText,
  openCreate,
  openRename,
  submitName,
  removeBook,
} = useBookSwitcher()

// 侧栏折叠/展开切换时收起弹层：锚点按钮随形态互换，避免弹层悬空
watch(
  () => props.collapsed,
  () => closePanel(),
)
</script>

<template>
  <div class="book-entry-root">
    <!-- 账本清单弹层（非模态面板：点外部/再点入口关闭；点击触发，锚定入口上方）。
         两形态入口（展开态常驻按钮 / 折叠态浮标）同处 #trigger，v-if/v-else 同期只渲染一个。 -->
    <AppPopover
      trigger="click"
      placement="top-start"
      :show-arrow="false"
      :show="panelShow"
      class="book-panel-popover"
      @update:show="panelShow = $event"
    >
      <template #trigger>
        <!-- 展开态：侧栏底部常驻入口，显示当前账本名 -->
        <NButton
          v-if="!collapsed"
          quaternary
          block
          size="small"
          class="book-entry"
          data-testid="book-entry"
          :aria-label="t('books.entry.open')"
        >
          <span class="book-entry-inner">
            <NIcon :size="16" class="book-entry-icon"><BookOutline /></NIcon>
            <span class="book-entry-name">{{ activeBook?.name ?? t('books.entry.label') }}</span>
            <NIcon :size="12" class="book-entry-caret"><ChevronUpOutline /></NIcon>
          </span>
        </NButton>

        <!-- 折叠态：固定左下角浮标图标形态（侧栏折叠宽度归零后入口仍随时可达） -->
        <NButton
          v-else
          circle
          quaternary
          size="medium"
          class="book-entry-float"
          data-testid="book-entry-float"
          :title="t('books.entry.open')"
          :aria-label="t('books.entry.open')"
        >
          <NIcon :size="18"><BookOutline /></NIcon>
        </NButton>
      </template>

      <div class="book-panel">
        <!-- 注册表回退警示（fallback_reason 通道，损坏时显著提示） -->
        <NAlert v-if="fallbackReason" type="warning" :show-icon="true" class="book-panel-alert">
          <NText strong>{{ t('books.fallback.title') }}</NText>
          <div class="book-panel-alert-detail">{{ fallbackReason }}</div>
        </NAlert>

        <!-- 读取失败诚实呈现：错误行 + 重试（不用空态假装一切正常） -->
        <div v-if="loadFailed" class="book-panel-status">
          <NText depth="3">{{ t('books.panel.loadFailed') }}</NText>
          <NButton size="tiny" quaternary type="primary" @click="() => void refresh()">
            {{ t('books.panel.retry') }}
          </NButton>
        </div>

        <div class="book-list">
          <div
            v-for="book in books"
            :key="book.id"
            class="book-row"
            :class="{ 'is-active': book.id === activeId }"
            @click="requestSwitch(book)"
          >
            <span class="book-row-name" :title="book.dir">{{ book.name }}</span>
            <span class="book-row-actions" @click.stop>
              <NTag v-if="book.id === activeId" size="small" round :bordered="false" type="primary">
                {{ t('books.panel.current') }}
              </NTag>
              <button
                type="button"
                class="book-row-action"
                :data-testid="`book-rename-${book.id}`"
                :disabled="!mutable"
                :title="t('books.panel.rename')"
                :aria-label="t('books.panel.rename')"
                @click="openRename(book)"
              >
                <NIcon :size="14"><CreateOutline /></NIcon>
              </button>
              <AppPopconfirm
                v-if="book.id !== activeId"
                :positive-text="t('books.panel.removeConfirmPositive')"
                @positive-click="() => void removeBook(book)"
              >
                <template #trigger>
                  <button
                    type="button"
                    class="book-row-action"
                    :data-testid="`book-remove-${book.id}`"
                    :disabled="!mutable"
                    :title="t('books.panel.remove')"
                    :aria-label="t('books.panel.remove')"
                  >
                    <NIcon :size="14"><TrashOutline /></NIcon>
                  </button>
                </template>
                {{ t('books.panel.removeConfirm', { name: book.name }) }}
              </AppPopconfirm>
            </span>
          </div>
        </div>

        <div v-if="!loading && books.length === 0 && !loadFailed" class="book-panel-status">
          <NText depth="3">{{ t('books.panel.empty') }}</NText>
        </div>

        <NButton
          size="small"
          block
          dashed
          class="book-panel-new"
          :disabled="!mutable"
          @click="openCreate"
        >
          {{ t('books.panel.new') }}
        </NButton>
      </div>
    </AppPopover>

    <!-- 新建 / 改名弹窗（ModalIntent 编排；sm 编辑类轻弹窗，spec #630 分档） -->
    <AppModal
      :show="nameIntent.intent.value !== null"
      preset="card"
      :title="nameModalTitle"
      card-size="sm"
      @update:show="(value: boolean) => { if (!value) nameIntent.close() }"
    >
      <NForm
        label-placement="left"
        :show-feedback="false"
        size="small"
        @submit.prevent="() => void submitName()"
      >
        <NSpace vertical :size="12">
          <NFormItem :label="t('books.createModal.nameLabel')">
            <NInput
              v-model:value="nameDraft"
              data-testid="book-name-input"
              :placeholder="t('books.createModal.namePlaceholder')"
              @keydown.enter.prevent="() => void submitName()"
            />
          </NFormItem>
          <NSpace justify="end">
            <NButton :disabled="nameBusy" @click="nameIntent.close()">
              {{ t('books.createModal.cancel') }}
            </NButton>
            <NButton
              type="primary"
              :loading="nameBusy"
              :disabled="nameBusy || nameDraft.trim().length === 0"
              @click="() => void submitName()"
            >
              {{ nameModalConfirmText }}
            </NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>
  </div>
</template>

<style scoped>
/* 入口根：展开态按钮与侧栏边缘留白（浮标与弹层均不受布局影响） */
.book-entry-root {
  padding: 4px 8px 10px;
}

/* 展开态入口：占满侧栏底部一行，图标 + 当前账本名 + 上箭头（弹层向上展开） */
.book-entry :deep(.n-button__content) {
  width: 100%;
}

.book-entry-inner {
  display: flex;
  width: 100%;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.book-entry-icon {
  flex-shrink: 0;
}

.book-entry-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  text-align: left;
}

.book-entry-caret {
  flex-shrink: 0;
  opacity: 0.55;
}

/* 折叠态浮标：固定视口左下角（侧栏折叠宽度归零，入口以图标形态保留） */
.book-entry-float {
  position: fixed;
  left: 10px;
  bottom: 10px;
  z-index: 100;
}

/* 弹层面板（class 落在 teleport 到 body 的浮层上，scoped 命中需 :deep 透传） */
.book-panel-popover :deep(.n-popover) {
  padding: 0;
}

.book-panel {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px;
  min-width: 240px;
}

.book-panel-alert-detail {
  margin-top: 2px;
  font-weight: normal;
}

.book-panel-status {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.book-list {
  display: flex;
  flex-direction: column;
}

.book-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 8px;
  border-radius: 4px;
  cursor: pointer;
}

.book-row:hover {
  background: rgba(128, 128, 128, 0.12);
}

.book-row.is-active {
  cursor: default;
}

.book-row-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.book-row-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.book-row-action {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border: none;
  padding: 0;
  border-radius: 4px;
  background: transparent;
  color: inherit;
  opacity: 0.6;
  cursor: pointer;
}

.book-row-action:hover:not(:disabled) {
  opacity: 1;
  background: rgba(128, 128, 128, 0.16);
}

.book-row-action:disabled {
  cursor: not-allowed;
  opacity: 0.3;
}
</style>
