<script setup lang="ts">
import { computed, h, onMounted, ref, watch } from 'vue'
import {
  NButton,
  NDataTable,
  NIcon,
  NInput,
  NProgress,
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
import { usePricesChanged } from '@/composables/usePricesChanged'
import { useAppDialog } from '@/composables/useAppDialog'
import { errorMessage as extractErrorMessage } from '@/utils/errors'
import { formatPrice, INSTRUMENT_SOURCE_LABELS, INSTRUMENT_TYPE_LABELS, MARKET_TYPE_LABELS, canManualPrice } from '@/types'
import AppModal from '@/components/AppModal.vue'
import AppSelect from '@/components/AppSelect.vue'
import CreateInstrumentModal from '@/components/investments/CreateInstrumentModal.vue'
import ManualPriceModal from '@/components/investments/ManualPriceModal.vue'
import type { Instrument, MarketType } from '@/types'

const reference = useReferenceStore()
const { syncing, resultMessage, status, sync } = useHoldingPriceSync()
// 删除二次确认（issue #292）：与账户删除同语义（useAppDialog 命令式对话框，
// ADR-0035 接入弹层注册表驱动快捷键抑制）
const dialog = useAppDialog()

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

// 价格失效信号（ADR-0031）：增量/全量同步实际写价后原地重拉——
// 用 load() 保留分页与搜索状态；reload() 会重置到第 1 页，
// 抽走用户视线下的行（issue #238）。
usePricesChanged(() => {
  void load()
})

// ---------------------------------------------------------------------------
// 新建标的（issue #290 / ADR-0036）：手动创建非股票类标的的入口，类型白名单
// （债券/ETF/其他）与名称必填经 CreateInstrumentModal 表单约束 + 后端 IPC
// 命令入口层守卫双重收口；（代码，类型）已存在时后端复用并更新名称（upsert）。
// ---------------------------------------------------------------------------
const createOpen = ref(false)
const createMessage = ref<string | null>(null)

function onInstrumentCreated(message: string) {
  createMessage.value = message
  // 新标的行上列表：回到第 1 页重拉（创建不落价，不发价格失效信号）
  reload()
}

// ---------------------------------------------------------------------------
// 自建标的删除（issue #292 / ADR-0036 决策 5）：仅手动来源且无任何 buy/sell
// 流水引用（security_transactions 无行）的标的可物理删除，守卫在后端前置检查；
// 同步来源标的不渲染删除动作（填错由全量同步修正）。行内删除 → useAppDialog
// 二次确认（遮罩点击不构成关闭意图）→ 确认后调 IPC 命令并本地重拉。
// ---------------------------------------------------------------------------
const deleteMessage = ref<{ type: 'success' | 'error'; text: string } | null>(null)

async function removeInstrument(row: Instrument) {
  const label = row.name || row.symbol
  try {
    await api.deleteInstrument(row.id)
    deleteMessage.value = { type: 'success', text: `已删除标的：${label}` }
    // 原地重拉保留搜索/分页状态；若当前页删空则回退一页
    await load()
    if (instruments.value.length === 0 && page.value > 1) {
      page.value -= 1
      await load()
    }
  } catch (e) {
    // 后端守卫拒删（如确认间隙已产生买卖流水）：中文错误原样展示
    deleteMessage.value = { type: 'error', text: `删除失败：${extractErrorMessage(e)}` }
  }
}

/** 删除走 useAppDialog 二次确认（与账户删除同语义）：取消不删，确认后才删除。
 * 遮罩点击不构成关闭意图（issue #252 弹层关闭语义）：确认/取消须显式点击。 */
function confirmDeleteInstrument(row: Instrument) {
  dialog.warning({
    title: '删除标的',
    content: `确认删除标的「${row.name || row.symbol}」？删除后不可恢复。`,
    positiveText: '删除',
    negativeText: '取消',
    maskClosable: false,
    onPositiveClick: () => removeInstrument(row),
  })
}

// ---------------------------------------------------------------------------
// 手动报价（issue #291 / ADR-0036）：行内「录价」动作只对同步覆盖不到的标的
// 开放（自建标的与名称充代码的基金行，判定 canManualPrice 与「净值可拉」分区
// 同源）；报价弹窗提交后后端广播价格失效信号，现价列刷新由本组件既有的
// usePricesChanged 订阅完成，此处零手动重拉。
// ---------------------------------------------------------------------------
const quoteTarget = ref<Instrument | null>(null)
const quoteOpen = ref(false)
const quoteMessage = ref<string | null>(null)

function openQuote(row: Instrument) {
  quoteTarget.value = row
  quoteOpen.value = true
}

function onQuoted(message: string) {
  // 只记页面级回执：现价列刷新由价格失效信号驱动（信号在信号处理中已保留
  // 分页/搜索状态原地重拉），调用方不手动重拉。
  quoteMessage.value = message
}

// ---------------------------------------------------------------------------
// 添加基金（issue #301 / ADR-0038）：fund 类型唯一创建入口——输入 6 位基金代码，
// 东财按代码即拉名称/分类/最新净值自动回填；查无此码中文报错、不产生标的行。
// ---------------------------------------------------------------------------
const addFundOpen = ref(false)
const addFundCode = ref('')
const addFundSubmitting = ref(false)
/** 弹窗内错误提示（查无此码等）：保持弹窗打开供用户改码重试 */
const addFundError = ref<string | null>(null)
/** 页面级成功回执（展示东财回填的名称/分类/净值） */
const addFundMessage = ref<string | null>(null)

// 6 位纯数字才可提交（后端同样校验，前端仅提前拦截不发起无效请求）
const addFundCodeValid = computed(() => /^\d{6}$/.test(addFundCode.value))

// 输入过滤：只留数字、最长 6 位
watch(addFundCode, () => {
  const filtered = addFundCode.value.replace(/\D/g, '').slice(0, 6)
  if (filtered !== addFundCode.value) addFundCode.value = filtered
})

function openAddFund() {
  addFundError.value = null
  addFundOpen.value = true
}

function closeAddFund() {
  addFundOpen.value = false
  addFundCode.value = ''
  addFundError.value = null
}

async function submitAddFund() {
  if (!addFundCodeValid.value || addFundSubmitting.value) return
  addFundSubmitting.value = true
  addFundError.value = null
  try {
    const res = await api.addFundByCode(addFundCode.value)
    const navText = res.nav_cents !== null && res.nav_date !== null
      ? `最新净值 ${formatPrice(res.nav_cents)}（${res.nav_date}）`
      : '暂未取到净值'
    addFundMessage.value = `已添加基金：${res.name}（${res.symbol} · ${res.fund_class}）${navText}`
    closeAddFund()
    // 新标的行上列表：回到第 1 页重拉（价格缓存刷新由价格失效信号驱动）
    reload()
  } catch (e) {
    addFundError.value = extractErrorMessage(e)
  } finally {
    addFundSubmitting.value = false
  }
}

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
    title: '来源',
    key: 'source',
    width: 70,
    render(row) {
      const label = INSTRUMENT_SOURCE_LABELS[row.source] ?? row.source
      // 手动标 tag 突出（自建标的，ADR-0036），同步标为纯文本
      if (row.source !== 'manual') return label
      return h(
        NTag,
        { type: 'info', size: 'small', bordered: false, 'data-testid': 'source-manual' },
        { default: () => label },
      )
    },
  },
  {
    title: '现价',
    key: 'price_cents',
    width: 100,
    render(row) {
      if (row.price_cents === null || row.price_cents === undefined) return '-'
      // 现价为价格列（万分之一元刻度，ADR-0038），用 formatPrice 展示
      const ccy = reference.currencyMap.get(row.currency_code)
      return formatPrice(row.price_cents, ccy)
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
    // 录价（issue #291 / ADR-0036）：只对同步覆盖不到的标的开放（自建标的与
    // 名称充代码的基金行）；真实代码基金与股票的现价归同步，无录价入口。
    title: '录价',
    key: 'quote',
    width: 70,
    render(row) {
      if (!canManualPrice(row)) return '-'
      return h(
        NButton,
        {
          size: 'tiny',
          secondary: true,
          'data-testid': `quote-${row.symbol}`,
          onClick: () => openQuote(row),
        },
        { default: () => '录价' },
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
  {
    // 操作列（issue #292 / ADR-0036）：删除仅对自建标的开放；同步来源标的
    // 一律不可删，不渲染动作（后端守卫同样拒删，双保险）
    title: '操作',
    key: 'actions',
    width: 70,
    render(row) {
      if (row.source !== 'manual') return '-'
      return h(
        NButton,
        {
          size: 'tiny',
          type: 'error',
          secondary: true,
          'data-testid': `delete-instrument-${row.symbol}`,
          onClick: () => confirmDeleteInstrument(row),
        },
        { default: () => '删除' },
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
      <AppSelect
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
        secondary
        size="small"
        data-testid="add-fund"
        @click="openAddFund"
      >
        添加基金
      </NButton>
      <NButton
        secondary
        size="small"
        data-testid="create-instrument"
        @click="createOpen = true"
      >
        新建标的
      </NButton>
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
    <NText v-if="addFundMessage" type="success" data-testid="add-fund-result">
      {{ addFundMessage }}
    </NText>
    <NText v-if="createMessage" type="success" data-testid="create-instrument-result">
      {{ createMessage }}
    </NText>
    <NText
      v-if="deleteMessage"
      :type="deleteMessage.type"
      data-testid="delete-instrument-result"
    >
      {{ deleteMessage.text }}
    </NText>
    <NText v-if="quoteMessage" type="success" data-testid="manual-quote-result">
      {{ quoteMessage }}
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

    <!-- 新建标的（issue #290 / ADR-0036）：手动创建非股票类标的，类型白名单
         债券/ETF/其他、名称必填、市场固定未知、币种默认人民币 -->
    <CreateInstrumentModal
      v-model:show="createOpen"
      @created="onInstrumentCreated"
    />

    <!-- 手动报价（issue #291 / ADR-0036）：日期 + 价格弹窗；录价成功后现价列
         经价格失效信号自动刷新（本组件顶部既有订阅），此处只记页面级回执 -->
    <ManualPriceModal
      v-model:show="quoteOpen"
      :instrument="quoteTarget"
      @quoted="onQuoted"
    />

    <!-- 添加基金（按代码即拉，issue #301）：6 位代码 → 东财回填名称/分类/最新净值；
         查无此码中文报错且不产生标的行（弹窗保持打开供改码重试） -->
    <AppModal
      v-model:show="addFundOpen"
      preset="card"
      title="添加基金"
      style="width: 440px"
      :bordered="false"
    >
      <NSpace vertical :size="12">
        <NText depth="3">
          输入 6 位基金代码，自动从东方财富拉取名称、类型与最新净值回填，无需手抄。
        </NText>
        <NInput
          v-model:value="addFundCode"
          placeholder="6 位基金代码，如 000001"
          :maxlength="6"
          :disabled="addFundSubmitting"
          data-testid="add-fund-code"
          @keyup.enter="submitAddFund"
        />
        <NText v-if="addFundError" type="error" data-testid="add-fund-error">
          {{ addFundError }}
        </NText>
        <NSpace justify="end" :size="12">
          <NButton data-testid="cancel-add-fund" @click="closeAddFund">
            取消
          </NButton>
          <NButton
            type="primary"
            data-testid="submit-add-fund"
            :loading="addFundSubmitting"
            :disabled="!addFundCodeValid"
            @click="submitAddFund"
          >
            添加
          </NButton>
        </NSpace>
      </NSpace>
    </AppModal>

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
