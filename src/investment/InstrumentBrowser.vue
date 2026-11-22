<script setup lang="ts">
import { computed, h, onMounted, ref, watch } from "vue";
import { NButton, NDataTable, NEmpty, NInput, NSpace, NSwitch, NTag, NText } from "naive-ui";
import type { DataTableColumn } from "naive-ui";
import { api } from "@ledger/api";
import { useReferenceStore } from "@/stores/reference";
import { t } from "@ledger/i18n";
import { useInstrumentInfoSync } from "@/investment/useInstrumentInfoSync";
import { usePricesChanged } from "@/investment/usePricesChanged";
import { useAppDialog } from "@/composables/useAppDialog";
import { createLatestWins } from "@ledger/latest-wins";
import { SEARCH_DEBOUNCE_MS } from "@/composables/search-debounce";
import { useWindowTier } from "@ledger/window-tier";
import SyncProgressBar from "@/investment/SyncProgressBar.vue";
import SyncDegradedNotice from "@/investment/SyncDegradedNotice.vue";
import ConceptLabel from "@/investment/ConceptLabel.vue";
import { errorMessage as extractErrorMessage } from "@ledger/utils/errors";
import {
  INSTRUMENT_PRICE_CHANNELS,
  INSTRUMENT_TYPES,
  MARKET_FILTER_TYPES,
  MARKET_TYPES,
} from "@ledger/types";
import { formatPrice } from "@ledger/money";
import { sumFixedColumnWidths } from "@ledger/utils/table";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import AddInstrumentModal from "@/investment/AddInstrumentModal.vue";
import ManualPriceModal from "@/investment/ManualPriceModal.vue";
import type { Instrument, MarketType } from "@ledger/types";

const reference = useReferenceStore();

const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");
const { syncing, resultMessage, status, progress, degraded, sync } = useInstrumentInfoSync();
const dialog = useAppDialog();

const props = defineProps<{ focusInstrumentId?: string | null }>();

const searchText = ref("");
const selectedMarket = ref<MarketType | null>(null);
const onlyInvested = ref(false);
const instruments = ref<Instrument[]>([]);
const total = ref(0);
const page = ref(1);
const pageSize = 50;
const loading = ref(false);
let searchTimer: ReturnType<typeof setTimeout> | undefined;
const loadWins = createLatestWins();

const marketOptions = computed(() =>
  MARKET_FILTER_TYPES.map((value) => ({ label: t(`investments.market.${value}`), value })),
);

function enumLabel(
  domain: "priceChannel" | "market" | "type",
  closed: readonly string[],
  value: string,
): string {
  return (closed as string[]).includes(value) ? t(`investments.${domain}.${value}`) : value;
}

async function load() {
  const myToken = loadWins.begin();
  loading.value = true;
  try {
    const res = await api.listInstruments({
      search: searchText.value.trim() || null,
      market: selectedMarket.value,
      only_invested: onlyInvested.value ? true : null,
      page: page.value,
      page_size: pageSize,
    });
    if (myToken.isStale()) return;
    instruments.value = res.items;
    total.value = res.total;
  } finally {
    if (!myToken.isStale()) loading.value = false;
  }
}

function reload() {
  page.value = 1;
  load();
}

watch(searchText, () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(reload, SEARCH_DEBOUNCE_MS);
});
watch(selectedMarket, reload);
watch(onlyInvested, reload);

const hasActiveFilter = computed(
  () => searchText.value.trim() !== "" || selectedMarket.value !== null || onlyInvested.value,
);
const emptyDescription = computed(() =>
  hasActiveFilter.value ? t("investments.browser.filterNoMatch") : t("investments.browser.empty"),
);

watch(
  () => props.focusInstrumentId,
  async (id) => {
    if (!id) return;
    const myToken = loadWins.begin();
    loading.value = true;
    try {
      const res = await api.listInstruments({
        page_size: pageSize,
        anchor_id: id,
      });
      if (myToken.isStale()) return;
      page.value = res.anchor_page ?? 1;
      instruments.value = res.items;
      total.value = res.total;
    } finally {
      if (!myToken.isStale()) loading.value = false;
    }
  },
  { immediate: true },
);

function rowClassName(row: Instrument): string {
  return row.id === props.focusInstrumentId ? "instrument-row-focused" : "";
}

usePricesChanged(() => {
  void load();
});

const addInstrumentOpen = ref(false);

const addInstrumentMessage = ref<string | null>(null);

function onInstrumentAdded(message: string) {
  addInstrumentMessage.value = message;
  reload();
}

const deleteMessage = ref<{ type: "success" | "error"; text: string } | null>(null);

async function removeInstrument(row: Instrument) {
  const label = row.name || row.symbol;
  try {
    await api.deleteInstrument(row.id);
    deleteMessage.value = {
      type: "success",
      text: t("investments.browser.deleteSuccess", { name: label }),
    };
    await load();
    if (instruments.value.length === 0 && page.value > 1) {
      page.value -= 1;
      await load();
    }
  } catch (e) {
    deleteMessage.value = {
      type: "error",
      text: t("investments.browser.deleteFailed", { message: extractErrorMessage(e) }),
    };
  }
}

function confirmDeleteInstrument(row: Instrument) {
  dialog.warning({
    title: t("investments.browser.deleteTitle"),
    content: t("investments.browser.deleteContent", { name: row.name || row.symbol }),
    positiveText: t("investments.browser.deleteConfirm"),
    negativeText: t("investments.browser.deleteCancel"),
    maskClosable: false,
    onPositiveClick: () => removeInstrument(row),
  });
}

const quoteTarget = ref<Instrument | null>(null);
const quoteOpen = ref(false);
const quoteMessage = ref<string | null>(null);

function openQuote(row: Instrument) {
  quoteTarget.value = row;
  quoteOpen.value = true;
}

function onQuoted(message: string) {
  quoteMessage.value = message;
}

const pagination = computed(() => ({
  page: page.value,
  pageSize,
  itemCount: total.value,
  onChange: (p: number) => {
    page.value = p;
    load();
  },
}));

const instrumentBrowseColumns = computed<DataTableColumn<Instrument>[]>(() => [
  { title: t("investments.browser.columns.symbol"), key: "symbol", width: 100 },
  { title: t("investments.browser.columns.name"), key: "name", width: 200 },
  {
    title: t("investments.browser.columns.priceSource"),
    key: "price_channel",
    width: 90,
    render(row) {
      return enumLabel("priceChannel", INSTRUMENT_PRICE_CHANNELS, row.price_channel);
    },
  },
  {
    title: () =>
      h(ConceptLabel, {
        label: t("investments.browser.columns.price"),
        concept: "price",
        testId: "browser-price",
      }),
    key: "price_cents",
    width: 100,
    render(row) {
      if (row.price_cents === null || row.price_cents === undefined) return "-";
      const ccy = reference.currencyMap.get(row.currency_code);
      return formatPrice(row.price_cents, ccy);
    },
  },
  {
    title: t("investments.browser.columns.market"),
    key: "market",
    width: 80,
    render(row) {
      return enumLabel("market", MARKET_TYPES, row.market);
    },
  },
  {
    title: t("investments.browser.columns.type"),
    key: "type",
    width: 80,
    render(row) {
      return enumLabel("type", INSTRUMENT_TYPES, row.type);
    },
  },
  { title: t("investments.browser.columns.currency"), key: "currency_code", width: 60 },
  {
    title: t("investments.browser.columns.quote"),
    key: "quote",
    width: 70,
    render(row) {
      if (row.price_channel !== "manual") return "-";
      return h(
        NButton,
        {
          size: "tiny",
          secondary: true,
          "data-testid": `quote-${row.symbol}`,
          onClick: () => openQuote(row),
        },
        { default: () => t("investments.browser.quoteAction") },
      );
    },
  },
  {
    title: t("investments.browser.columns.invested"),
    key: "invested",
    width: 80,
    render(row) {
      if (!row.invested) return "-";
      return h(
        NTag,
        { type: "success", size: "small", bordered: false },
        { default: () => t("investments.browser.investedTag") },
      );
    },
  },
  {
    title: t("investments.browser.columns.actions"),
    key: "actions",
    width: 70,
    render(row) {
      if (row.source !== "manual") return "-";
      return h(
        NButton,
        {
          size: "tiny",
          type: "error",
          secondary: true,
          "data-testid": `delete-instrument-${row.symbol}`,
          onClick: () => confirmDeleteInstrument(row),
        },
        { default: () => t("investments.browser.deleteAction") },
      );
    },
  },
]);

onMounted(() => {
  if (!props.focusInstrumentId) load();
});

const browseScrollX = computed(() => sumFixedColumnWidths(instrumentBrowseColumns.value));
</script>

<template>
  <NSpace vertical :size="12">
    <!-- 同步确定进度条（issue #897）：列表顶部就近反馈，与盈亏页同一展示组件 -->
    <SyncProgressBar :progress="progress" />
    <NSpace align="center" :size="12">
      <NInput
        v-model:value="searchText"
        :placeholder="t('investments.browser.searchPlaceholder')"
        clearable
        style="width: 240px"
      />
      <AppSelect
        v-model:value="selectedMarket"
        :options="marketOptions"
        :placeholder="t('investments.browser.allMarkets')"
        clearable
        style="width: 140px"
      />
      <NSwitch v-model:value="onlyInvested" size="small" data-testid="only-invested-switch" />
      <span style="font-size: 13px">{{ t("investments.browser.onlyInvested") }}</span>
      <NButton
        secondary
        size="small"
        data-testid="add-instrument"
        @click="addInstrumentOpen = true"
      >
        {{ t("investments.browser.addInstrument") }}
      </NButton>
      <NButton
        type="primary"
        size="small"
        :loading="syncing"
        data-testid="sync-instrument-info"
        @click="sync"
      >
        {{ t("investments.browser.syncInstrumentInfo") }}
      </NButton>
    </NSpace>
    <NText v-if="resultMessage" :type="status === 'error' ? 'error' : 'info'">
      {{ resultMessage }}
    </NText>
    <!-- 降级可见（issue #1376 / ADR-0121 决策 4）：回退逐标的通道时明示本次较慢 -->
    <SyncDegradedNotice :degraded="degraded" />
    <NText v-if="addInstrumentMessage" type="success" data-testid="add-instrument-result">
      {{ addInstrumentMessage }}
    </NText>
    <NText v-if="deleteMessage" :type="deleteMessage.type" data-testid="delete-instrument-result">
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
      :scroll-x="isMobileTier ? browseScrollX : undefined"
      :pagination="pagination"
      :row-class-name="rowClassName"
    >
      <!-- 空态两态区分（issue #1193）：全部标的不在场 vs 筛选未命中；
           testid 随态切换，测试按用户可观察文案断言。加载门与持仓页同款：
           首载/筛选重拉在途不渲染空态，否则空库会先闪现「暂无标的」再出列表 -->
      <template #empty>
        <NEmpty
          v-if="!loading"
          :description="emptyDescription"
          :data-testid="hasActiveFilter ? 'instruments-no-match' : 'instruments-empty'"
        />
      </template>
    </NDataTable>

    <!-- 添加投资标的（issue #697 / spec #690；六通道修订 #826）：标的创建唯一
         入口——市场必选录入通道 + 按代码查询（命中自动识别类型并回填名称/最新价）；
         自定义标的通道零查询直接建档，未命中只报错并引导切换该通道；场外基金
         通道即原「添加基金」按代码即拉，语义不变。既有「新建标的」独立弹窗随
         本入口收编退役 -->
    <AddInstrumentModal v-model:show="addInstrumentOpen" @added="onInstrumentAdded" />

    <!-- 手动报价（issue #291 / ADR-0036）：日期 + 价格弹窗；录价成功后现价列
         经价格失效信号自动刷新（本组件顶部既有订阅），此处只记页面级回执 -->
    <ManualPriceModal v-model:show="quoteOpen" :instrument="quoteTarget" @quoted="onQuoted" />
  </NSpace>
</template>

<style>
tr.instrument-row-focused > td {
  background-color: rgba(245, 158, 11, 0.16);
}
</style>
