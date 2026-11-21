<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { NButton, NButtonGroup, NIcon, NSpace, useThemeVars } from "naive-ui";
import { ChevronBack, ChevronDown, ChevronForward } from "@vicons/ionicons5";
import AppDatePicker from "@ledger/ui-kit/AppDatePicker.vue";
import { useInputMode } from "@/composables/useInputMode";
import { useLoadable } from "@ledger/loadable";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import type { ReportDateRange } from "@ledger/types";
import {
  TIME_PERIOD_PRESETS,
  canStepPeriod,
  derivePeriodBoundary,
  formatPeriodLabel,
  matchPreset,
  periodFromTimestamp,
  periodRange,
  periodStartTimestamp,
  presetRange,
  rangeToPeriod,
  isPeriodWithinBoundary,
  stepPeriod,
  type NullableDateRange,
  type PeriodUnit,
  type TimePeriodPreset,
} from "@ledger/utils/time-period";

const props = withDefaults(
  defineProps<{
    modelValue: NullableDateRange;
    presets?: readonly TimePeriodPreset[];
    stepShortcutKeys?: { prev: string; next: string } | null;
  }>(),
  { presets: () => TIME_PERIOD_PRESETS, stepShortcutKeys: null },
);

const emit = defineEmits<{
  "update:modelValue": [NullableDateRange];
}>();

const nowTick = ref(Date.now());
let nowTicker: ReturnType<typeof setInterval> | undefined;

const inputMode = useInputMode();
const themeVars = useThemeVars();
const isTouch = computed(() => inputMode.value === "touch");
const TOUCH_TARGET_STYLE = { minHeight: "48px" };

const stepKeys = computed(() =>
  props.stepShortcutKeys != null && inputMode.value === "pointer" ? props.stepShortcutKeys : null,
);
const stepPrevTitle = computed(() =>
  stepKeys.value ? `${t("quickTimeRange.period.prev")} (${stepKeys.value.prev})` : undefined,
);
const stepNextTitle = computed(() =>
  stepKeys.value ? `${t("quickTimeRange.period.next")} (${stepKeys.value.next})` : undefined,
);

const activePreset = computed(() =>
  matchPreset(props.modelValue.from, props.modelValue.to, nowTick.value),
);

const currentPeriod = computed(() => rangeToPeriod(props.modelValue.from, props.modelValue.to));

const dateRange = ref<ReportDateRange | null>(null);
let unlistenLedgerChanged: UnlistenFn | null = null;
let ledgerListenerDisposed = false;

const { error: dateRangeError, run: runLoadDateRange } = useLoadable(() => api.reportDateRange(), {
  silent: true,
});

async function loadDateRange() {
  const range = await runLoadDateRange();
  if (range !== null) dateRange.value = range;
  else if (dateRangeError.value !== null) dateRange.value = null;
}

const periodBoundary = computed(() => {
  const p = currentPeriod.value;
  if (!p || !dateRange.value) return null;
  return derivePeriodBoundary(p.unit, dateRange.value, nowTick.value);
});

const canStepPrev = computed(() => {
  const p = currentPeriod.value;
  return p !== null && canStepPeriod(p, -1, periodBoundary.value);
});
const canStepNext = computed(() => {
  const p = currentPeriod.value;
  return p !== null && canStepPeriod(p, 1, periodBoundary.value);
});

const periodPanelUnit = computed<PeriodUnit>(() => currentPeriod.value?.unit ?? "month");
const periodPanelValue = computed(() => {
  const p = currentPeriod.value;
  if (p) return periodStartTimestamp(p);
  return periodStartTimestamp(periodFromTimestamp("month", nowTick.value));
});
const periodPanelFormat = computed(() =>
  t(`quickTimeRange.periodPicker.format.${periodPanelUnit.value}`),
);
const periodPanelBoundary = computed(() => {
  if (!dateRange.value) return null;
  return derivePeriodBoundary(periodPanelUnit.value, dateRange.value, nowTick.value);
});
const periodPanelYearRange = computed<[number, number]>(() => {
  const boundary = periodPanelBoundary.value;
  if (boundary) return [boundary.earliest.year, boundary.latest.year];
  const year = new Date(nowTick.value).getFullYear();
  return [year - 100, year + 100];
});

type PeriodDatePickerDetail =
  | { type: "date"; year: number; month: number; date: number }
  | { type: "month"; year: number; month: number }
  | { type: "quarter"; year: number; quarter: number }
  | { type: "year"; year: number }
  | { type: "input" };

const isPeriodDateDisabled = (_timestamp: number, detail: PeriodDatePickerDetail): boolean => {
  const boundary = periodPanelBoundary.value;
  if (!boundary || detail.type === "input") return false;
  let period;
  if (periodPanelUnit.value === "month" && detail.type === "month") {
    period = { unit: "month" as const, year: detail.year, index: detail.month };
  } else if (periodPanelUnit.value === "quarter" && detail.type === "quarter") {
    period = { unit: "quarter" as const, year: detail.year, index: detail.quarter - 1 };
  } else if (periodPanelUnit.value === "year" && detail.type === "year") {
    period = { unit: "year" as const, year: detail.year, index: 0 };
  } else {
    return false;
  }
  return !isPeriodWithinBoundary(period, boundary);
};

const periodPanelOpen = ref(false);
const periodLabelText = computed(() =>
  currentPeriod.value
    ? formatPeriodLabel(currentPeriod.value)
    : t("quickTimeRange.periodLabel.none"),
);

function onPeriodPanelSelect(timestamp: number | null) {
  if (timestamp === null) return;
  const period = periodFromTimestamp(periodPanelUnit.value, timestamp);
  emit("update:modelValue", periodRange(period));
  periodPanelOpen.value = false;
}

function onStepPeriod(delta: 1 | -1) {
  const p = currentPeriod.value;
  if (!p || !canStepPeriod(p, delta, periodBoundary.value)) return;
  emit("update:modelValue", periodRange(stepPeriod(p, delta)));
}

function onPresetSelect(preset: TimePeriodPreset) {
  if (preset === "all") {
    emit("update:modelValue", { from: null, to: null });
    return;
  }
  emit("update:modelValue", presetRange(preset, nowTick.value));
}

function presetLabel(preset: TimePeriodPreset): string {
  return t(`quickTimeRange.period.${preset}`);
}

onMounted(() => {
  void loadDateRange();
  void listen("ledger:changed", () => {
    void loadDateRange();
  })
    .then((fn) => {
      if (ledgerListenerDisposed) {
        fn();
        return;
      }
      unlistenLedgerChanged = fn;
    })
    .catch(() => {});
  nowTicker = setInterval(() => {
    nowTick.value = Date.now();
  }, 60_000);
});

onBeforeUnmount(() => {
  if (nowTicker !== undefined) clearInterval(nowTicker);
  ledgerListenerDisposed = true;
  unlistenLedgerChanged?.();
  unlistenLedgerChanged = null;
});

defineExpose({
  stepPrev: () => onStepPeriod(-1),
  stepNext: () => onStepPeriod(1),
});
</script>

<template>
  <NSpace :size="8" align="center" :wrap="true">
    <NButtonGroup size="small">
      <NButton
        v-for="p in presets"
        :key="p"
        size="small"
        :type="activePreset === p ? 'primary' : 'default'"
        :quaternary="activePreset !== p"
        :aria-pressed="activePreset === p"
        :style="isTouch ? TOUCH_TARGET_STYLE : undefined"
        @click="onPresetSelect(p)"
      >
        {{ presetLabel(p) }}
      </NButton>
    </NButtonGroup>
    <NButtonGroup size="small">
      <NButton
        size="small"
        quaternary
        :style="isTouch ? TOUCH_TARGET_STYLE : undefined"
        :disabled="!canStepPrev"
        :aria-label="t('quickTimeRange.period.prev')"
        :title="stepPrevTitle"
        @click="onStepPeriod(-1)"
      >
        <NIcon><ChevronBack /></NIcon>
        <span v-if="stepKeys" class="period-key" :style="{ color: themeVars.textColor3 }">{{
          stepKeys?.prev
        }}</span>
      </NButton>
      <!-- 期间标签按钮（issue #425）：与步进箭头同款 quaternary small + ChevronDown
           「文字 + ▾」下拉通用信号，全部态占位文案同款可点（默认月档）；键盘 Tab 聚焦后
           Enter/Space 打开（原生按钮键盘激活 + keydown 显式双路），aria-haspopup/
           aria-expanded 随面板开合。透明面板载体 pointer-events 关闭，hover/点击
           均落在按钮自身；载体仅作面板锚点。 -->
      <span class="period-label" @click="periodPanelOpen = true">
        <NButton
          size="small"
          quaternary
          :style="isTouch ? TOUCH_TARGET_STYLE : undefined"
          aria-haspopup="dialog"
          :aria-expanded="periodPanelOpen"
          @keydown.enter.prevent="periodPanelOpen = true"
          @keydown.space.prevent="periodPanelOpen = true"
        >
          <span class="period-label-text">{{ periodLabelText }}</span>
          <NIcon class="period-label-chevron"><ChevronDown /></NIcon>
        </NButton>
        <AppDatePicker
          class="period-picker"
          :show="periodPanelOpen"
          :value="currentPeriod ? periodPanelValue : null"
          :type="periodPanelUnit"
          :format="periodPanelFormat"
          :year-range="periodPanelYearRange"
          :is-date-disabled="isPeriodDateDisabled"
          :bordered="false"
          :clearable="false"
          :input-readonly="true"
          @update:show="(show: boolean) => (periodPanelOpen = show)"
          @update:value="onPeriodPanelSelect"
        />
      </span>
      <NButton
        size="small"
        quaternary
        :style="isTouch ? TOUCH_TARGET_STYLE : undefined"
        :disabled="!canStepNext"
        :aria-label="t('quickTimeRange.period.next')"
        :title="stepNextTitle"
        @click="onStepPeriod(1)"
      >
        <NIcon><ChevronForward /></NIcon>
        <span v-if="stepKeys" class="period-key" :style="{ color: themeVars.textColor3 }">{{
          stepKeys?.next
        }}</span>
      </NButton>
    </NButtonGroup>
  </NSpace>
</template>

<style scoped>
.period-label {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 96px;
}

.period-label :deep(.period-picker) {
  position: absolute;
  inset: 0;
  opacity: 0;
  pointer-events: none;
}

.period-label :deep(.n-input) {
  min-width: 96px;
}

.period-label-chevron {
  margin-left: 4px;
}

.period-key {
  font-size: 11px;
  margin-left: 1px;
}
</style>
