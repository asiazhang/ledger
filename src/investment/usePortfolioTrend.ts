import { computed, onMounted, ref, watch } from "vue";
import { api } from "@ledger/api";
import { usePricesChanged } from "@/investment/usePricesChanged";
import {
  useInvestmentsSessionStore,
  type TrendRangePreset,
} from "@/investment/investments-session";
import type { PortfolioValueTrend, TrendRange } from "@ledger/types";

export type { TrendRangePreset } from "@/investment/investments-session";

export const TREND_RANGE_PRESETS: { value: TrendRangePreset; labelKey: string }[] = [
  { value: "1m", labelKey: "investments.trend.range1m" },
  { value: "3m", labelKey: "investments.trend.range3m" },
  { value: "1y", labelKey: "investments.trend.range1y" },
  { value: "3y", labelKey: "investments.trend.range3y" },
  { value: "5y", labelKey: "investments.trend.range5y" },
  { value: "all", labelKey: "investments.trend.rangeAll" },
];

function daysInMonth(year: number, month: number): number {
  return new Date(year, month, 0).getDate();
}

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

const PRESET_MONTHS: Record<Exclude<TrendRangePreset, "all">, number> = {
  "1m": 1,
  "3m": 3,
  "1y": 12,
  "3y": 36,
  "5y": 60,
};

export function toTrendRange(preset: TrendRangePreset, today: Date): TrendRange {
  if (preset === "all") return {};
  const months = PRESET_MONTHS[preset];
  const total = today.getFullYear() * 12 + today.getMonth() - months;
  const year = Math.floor(total / 12);
  const month = (total % 12) + 1;
  const day = Math.min(today.getDate(), daysInMonth(year, month));
  return { start_date: `${year}-${pad2(month)}-${pad2(day)}`, end_date: null };
}

export interface TrendChartSeries {
  labels: string[];
  values: (number | null)[];
}

function mondayOf(isoDate: string): Date {
  const [year, month, day] = isoDate.split("-").map(Number);
  const date = new Date(year, month - 1, day, 12);
  date.setDate(date.getDate() - ((date.getDay() + 6) % 7));
  return date;
}

function isoOf(date: Date): string {
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}

export function toTrendChartSeries(points: { date: string; value: number }[]): TrendChartSeries {
  if (points.length === 0) return { labels: [], values: [] };

  const slots: string[] = [];
  const cursor = mondayOf(points[0].date);
  const end = mondayOf(points[points.length - 1].date);
  while (cursor <= end) {
    slots.push(isoOf(cursor));
    cursor.setDate(cursor.getDate() + 7);
  }

  const pointByWeek = new Map(points.map((p) => [mondayOf(p.date).getTime(), p]));
  return {
    labels: slots.map((slot) => pointByWeek.get(mondayOf(slot).getTime())?.date ?? slot),
    values: slots.map((slot) => pointByWeek.get(mondayOf(slot).getTime())?.value ?? null),
  };
}

export function isTrendEmpty(points: { date: string; value: number }[]): boolean {
  return points.length === 0;
}

export function usePortfolioTrend() {
  const session = useInvestmentsSessionStore();
  const preset = computed<TrendRangePreset>(() => session.trendPreset);

  const loading = ref(false);
  const portfolioTrend = ref<PortfolioValueTrend | null>(null);

  const range = computed(() => toTrendRange(preset.value, new Date()));

  let lastFetchedKey: string | null = null;

  async function fetchTrend() {
    const key = `portfolio|${range.value.start_date ?? "all"}`;
    if (key === lastFetchedKey) return;
    lastFetchedKey = key;
    loading.value = true;
    try {
      portfolioTrend.value = await api.portfolioValueTrend(range.value);
    } catch (e) {
      lastFetchedKey = null;
      throw e;
    } finally {
      loading.value = false;
    }
  }

  async function refresh() {
    await fetchTrend();
  }

  async function forceRefresh() {
    lastFetchedKey = null;
    await refresh();
  }

  function setPreset(next: TrendRangePreset) {
    session.setTrendPreset(next);
  }

  watch(preset, () => {
    void refresh().catch(() => {});
  });

  usePricesChanged(() => {
    void forceRefresh().catch(() => {});
  });

  onMounted(() => {
    void refresh().catch(() => {});
  });

  const trendPoints = computed(() =>
    (portfolioTrend.value?.points ?? []).map((p) => ({
      date: p.date,
      value: p.market_value_cents,
    })),
  );

  const chartSeries = computed(() => toTrendChartSeries(trendPoints.value));
  const isEmpty = computed(() => isTrendEmpty(trendPoints.value));

  const backfill = computed(() => portfolioTrend.value?.backfill ?? null);

  const currencyCode = computed(() => portfolioTrend.value?.currency_code ?? null);

  return {
    preset,
    loading,
    chartSeries,
    isEmpty,
    backfill,
    currencyCode,
    refresh,
    setPreset,
  };
}
