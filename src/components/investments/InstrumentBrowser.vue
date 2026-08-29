<script setup lang="ts">
import { computed, h, onMounted, ref, watch } from 'vue'
import {
  NButton,
  NDataTable,
  NIcon,
  NInput,
  NProgress,
  NSelect,
  NSpace,
  NSwitch,
  NTag,
  NText,
} from 'naive-ui'
import { Refresh } from '@vicons/ionicons5'
import type { DataTableColumn } from 'naive-ui'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { useHoldingPriceSync } from '@/composables/useHoldingPriceSync'
import { useInstrumentFullSync } from '@/composables/useInstrumentFullSync'
import { formatAmount, INSTRUMENT_TYPE_LABELS, MARKET_TYPE_LABELS } from '@/types'
import AppModal from '@/components/AppModal.vue'
import type { Instrument, MarketType } from '@/types'

const reference = useReferenceStore()
const { syncing, resultMessage, status, sync } = useHoldingPriceSync()

// 标的行「走势」入口（issue #139）：向视图层发出带标的信息的事件，由其切换到走势 tab
const emit = defineEmits<{ 'view-trend': [instrument: Instrument] }>()

// 股票标的全量同步（issue #109）：二次确认 + 模态进度 + 中断 + 终态反馈
const {
  syncStatus,
  syncing: fullSyncing,
  progress,
  current,
  total: syncTotal,
  inserted,
  updated,
  errorMessage,
  confirmOpen,
  modalOpen,
  cancelling,
  openConfirm,
  closeConfirm,
  confirmSync,
  requestCancel,
  openModal,
  closeModal,
} = useInstrumentFullSync()

// 全量同步按钮：同步中点击重开进度框（同步后台继续），否则弹二次确认（防重复触发同步）
function onFullSyncClick() {
  if (fullSyncing.value) openModal()
  else openConfirm()
}

// 标的浏览（服务端分页 + 搜索）
const searchText = ref('')
const selectedMarket = ref<MarketType | null>(null)
// 只看持仓标的（issue #108）：勾选后仅列出有当前持仓的标的
const onlyInvested = ref(false)
const instruments = ref<Instrument[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = 50
const loading = ref(false)
let searchTimer: ReturnType<typeof setTimeout> | undefined

const marketOptions = computed(() =>
  (Object.entries(MARKET_TYPE_LABELS) as [MarketType, string][]).map(
    ([value, label]) => ({ label, value }),
  ),
)

async function load() {
  loading.value = true
  try {
    const res = await api.listInstruments({
      search: searchText.value.trim() || null,
      market: selectedMarket.value,
      // only_invested 为 false/缺省时不过滤，仅勾选时传 true
      only_invested: onlyInvested.value ? true : null,
      page: page.value,
      page_size: pageSize,
    })
    instruments.value = res.items
    total.value = res.total
  } finally {
    loading.value = false
  }
}

function reload() {
  page.value = 1
  load()
}

watch(searchText, () => {
  clearTimeout(searchTimer)
  searchTimer = setTimeout(reload, 300)
})
watch(selectedMarket, reload)
watch(onlyInvested, reload)

const pagination = computed(() => ({
  page: page.value,
  pageSize,
  itemCount: total.value,
  onChange: (p: number) => {
    page.value = p
    load()
  },
}))

const instrumentBrowseColumns: DataTableColumn<Instrument>[] = [
  { title: '代码', key: 'symbol', width: 100 },
  { title: '名称', key: 'name', width: 200 },
  {
    title: '现价',
    key: 'price_cents',
    width: 100,
    render(row) {
      if (row.price_cents === null || row.price_cents === undefined) return '-'
      const ccy = reference.currencyMap.get(row.currency_code)
      return formatAmount(row.price_cents, ccy)
    },
  },
  {
    title: '市场',
    key: 'market',
    width: 80,
    render(row) {
      return MARKET_TYPE_LABELS[row.market] ?? row.market
    },
  },
  {
    title: '类型',
    key: 'type',
    width: 80,
    render(row) {
      return INSTRUMENT_TYPE_LABELS[row.type] ?? row.type
    },
  },
  { title: '币种', key: 'currency_code', width: 60 },
  {
    title: '走势',
    key: 'trend',
    width: 70,
    render(row) {
      return h(
        NButton,
        {
          size: 'tiny',
          secondary: true,
          'data-testid': `view-trend-${row.symbol}`,
          onClick: () => emit('view-trend', row),
        },
        { default: () => '走势' },
      )
    },
  },
  {
    title: '持仓',
    key: 'invested',
    width: 80,
    render(row) {
      if (!row.invested) return '-'
      return h(
        NTag,
        { type: 'success', size: 'small', bordered: false },
        { default: () => '持仓' },
      )
    },
  },
]

onMounted(load)
</script>

<template>
  <NSpace vertical :size="12">
    <NSpace align="center" :size="12">
      <NInput
        v-model:value="searchText"
        placeholder="搜索代码或名称..."
        clearable
        style="width: 240px"
      />
      <NSelect
        v-model:value="selectedMarket"
        :options="marketOptions"
        placeholder="全部市场"
        clearable
        style="width: 140px"
      />
      <NSwitch
        v-model:value="onlyInvested"
        size="small"
        data-testid="only-invested-switch"
      />
      <span style="font-size: 13px">只看持仓</span>
      <NButton
        type="primary"
        size="small"
        :loading="syncing"
        data-testid="sync-holding-prices"
        @click="sync"
      >
        同步持仓价格
      </NButton>
      <NButton
        secondary
        size="small"
        data-testid="full-sync"
        @click="onFullSyncClick"
      >
        <template v-if="fullSyncing" #icon>
          <NIcon class="sync-spin"><Refresh /></NIcon>
        </template>
        {{ fullSyncing ? '同步中' : '全量同步' }}
      </NButton>
    </NSpace>
    <NText v-if="resultMessage" :type="status === 'error' ? 'error' : 'info'">
      {{ resultMessage }}
    </NText>
    <NDataTable
      :columns="instrumentBrowseColumns"
      :data="instruments"
      :loading="loading"
      :bordered="false"
      size="small"
      remote
      :pagination="pagination"
    />

    <!-- 二次确认：未确认不发起同步（issue #109） -->
    <AppModal
      v-model:show="confirmOpen"
      preset="card"
      title="全量同步股票标的"
      style="width: 480px"
      :bordered="false"
    >
      <NSpace vertical :size="12">
        <NText depth="3">
          将从东方财富拉取沪市、深市、港股的股票标的最新行情，涉及数百次 API
          请求，可能需要数分钟。已存在的标的名称或市场变更会自动更新，不会删除已有数据。
        </NText>
        <NSpace justify="end" :size="12">
          <NButton data-testid="cancel-confirm-full-sync" @click="closeConfirm">
            取消
          </NButton>
          <NButton
            type="primary"
            data-testid="confirm-full-sync"
            :loading="fullSyncing"
            @click="confirmSync"
          >
            开始同步
          </NButton>
        </NSpace>
      </NSpace>
    </AppModal>

    <!-- 模态进度：进度条 + 已处理/总数 + 累计新增/更新 + 中断；终态明确反馈 -->
    <AppModal
      v-model:show="modalOpen"
      preset="card"
      title="股票标的全量同步"
      style="width: 480px"
      :bordered="false"
      @update:show="(v: boolean) => !v && closeModal()"
    >
      <NSpace vertical :size="12">
        <template v-if="fullSyncing">
          <NProgress
            type="line"
            :percentage="progress"
            :show-indicator="true"
            :indicator-placement="'inside'"
            status="success"
            :height="28"
          />
          <NText depth="3" data-testid="full-sync-count">
            已处理 {{ current }} / 共 {{ syncTotal }} 只{{ syncTotal === 0 ? '（正在获取行情...）' : '' }}
          </NText>
          <NText depth="3" data-testid="full-sync-cumulative">
            累计新增 {{ inserted }} 只 · 更新 {{ updated }} 只
          </NText>
          <NSpace justify="end" :size="12">
            <NButton
              type="error"
              size="small"
              :loading="cancelling"
              data-testid="cancel-full-sync"
              @click="requestCancel"
            >
              中断同步
            </NButton>
          </NSpace>
        </template>
        <template v-else-if="syncStatus === 'done'">
          <NText type="success" data-testid="full-sync-result">
            同步完成：新增 {{ inserted }} 只，更新 {{ updated }} 只
          </NText>
        </template>
        <template v-else-if="syncStatus === 'cancelled'">
          <NText type="warning" data-testid="full-sync-result">
            已中断，已同步 {{ inserted }} 只，更新 {{ updated }} 只
          </NText>
        </template>
        <template v-else-if="syncStatus === 'error'">
          <NText type="error" data-testid="full-sync-result">
            同步失败：{{ errorMessage }}
          </NText>
        </template>
      </NSpace>
    </AppModal>
  </NSpace>
</template>

<style scoped>
/* 同步中按钮的旋转装载指示：视觉呈「loading」，但按钮保持可点击以重开进度框（issue #109） */
.sync-spin {
  animation: sync-spin 1s linear infinite;
}
@keyframes sync-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
