import { computed, onMounted, ref, watch } from "vue";
import { api } from "@ledger/api";
import { usePricesChanged } from "@/investment/usePricesChanged";
import {
  useInvestmentsSessionStore,
  type TrendRangePreset,
} from "@/investment/investments-session";
import type { PortfolioValueTrend, TrendRange } from "@ledger/types";

/** 走势预设区间闭集随状态迁入投资页会话状态 store（issue #1192）；
 * 此处再导出维持既有导入路径（消费方经本模块取用），不制造第二口径。 */
export type { TrendRangePreset } from "@/investment/investments-session";

/** 区间档位闭集（issue #1907 扩展 3 年/5 年）：后端按数据实际起点裁剪，
 * 股票类历史仅近两年（ADR-0019），更长档位前段留白由曲线如实呈现。 */
export const TREND_RANGE_PRESETS: { value: TrendRangePreset; labelKey: string }[] = [
  { value: "1m", labelKey: "investments.trend.range1m" },
  { value: "3m", labelKey: "investments.trend.range3m" },
  { value: "1y", labelKey: "investments.trend.range1y" },
  { value: "3y", labelKey: "investments.trend.range3y" },
  { value: "5y", labelKey: "investments.trend.range5y" },
  { value: "all", labelKey: "investments.trend.rangeAll" },
];

/** 某月天数（month 1-12） */
function daysInMonth(year: number, month: number): number {
  return new Date(year, month, 0).getDate();
}

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

/** 档位 → 月数（闭集映射，调用方不写分支） */
const PRESET_MONTHS: Record<Exclude<TrendRangePreset, "all">, number> = {
  "1m": 1,
  "3m": 3,
  "1y": 12,
  "3y": 36,
  "5y": 60,
};

/**
 * 预设区间 → 查询区间（纯函数）：起点 = today 减去对应时长（月末溢出钳制到
 * 目标月最后一日），终点不设界（后端裁剪到今天为止）。「全部」不设起止。
 */
export function toTrendRange(preset: TrendRangePreset, today: Date): TrendRange {
  if (preset === "all") return {};
  const months = PRESET_MONTHS[preset];
  const total = today.getFullYear() * 12 + today.getMonth() - months;
  const year = Math.floor(total / 12);
  const month = (total % 12) + 1;
  const day = Math.min(today.getDate(), daysInMonth(year, month));
  return { start_date: `${year}-${pad2(month)}-${pad2(day)}`, end_date: null };
}

/** 图表序列：x 轴按周连续的日期槽位 + y 轴数值（分；缺周为 null，由 spanGaps 跨越） */
export interface TrendChartSeries {
  labels: string[];
  values: (number | null)[];
}

/** 取 ISO 日期（YYYY-MM-DD）所在周的周一（本地正午构造，避开 DST 漂移） */
function mondayOf(isoDate: string): Date {
  const [year, month, day] = isoDate.split("-").map(Number);
  const date = new Date(year, month - 1, day, 12);
  date.setDate(date.getDate() - ((date.getDay() + 6) % 7));
  return date;
}

function isoOf(date: Date): string {
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}

/**
 * 采样点序列 → 图表数据（纯函数）：x 轴按日期连续——从首点到末点逐周生成槽位，
 * 采样点按所在周归位（label 用真实采样日），缺周填 null 由图表 spanGaps 连点跨越。
 */
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

/** 空态判定：无任何采样点即无历史数据 */
export function isTrendEmpty(points: { date: string; value: number }[]): boolean {
  return points.length === 0;
}

/**
 * 投资资产走势数据层（issue #139 / ADR-0019；#1907 起组合曲线随概览页签，
 * 单标的走势随走势页签退役删除）：收口组合走势的获取与转换。查询区间由预设
 * 区间派生；数据由 T3 命令给出（区间裁剪、首有效点起始在后端完成），此处只做
 * 点位映射与空态判定。
 *
 * 区间选择住投资页会话状态 store（issue #1192，ADR-0094 会话内保留）：切走页签
 * 再回来仍是离开时的那个区间，冷启动回默认近一年。数据拉取、同键去重、竞态
 * 治愈与价格失效信号重拉仍在实例内（每趟进入现拉，保留的是选择不是快照）。
 */
export function usePortfolioTrend() {
  const session = useInvestmentsSessionStore();
  /** 当前预设区间（只读投影；写入经 setPreset） */
  const preset = computed<TrendRangePreset>(() => session.trendPreset);

  const loading = ref(false);
  const portfolioTrend = ref<PortfolioValueTrend | null>(null);

  const range = computed(() => toTrendRange(preset.value, new Date()));

  /** 上次已取数的请求键（区间起始）：同一键不重复请求，收敛双触发通道 */
  let lastFetchedKey: string | null = null;

  async function fetchTrend() {
    const key = `portfolio|${range.value.start_date ?? "all"}`;
    if (key === lastFetchedKey) return;
    lastFetchedKey = key;
    loading.value = true;
    try {
      portfolioTrend.value = await api.portfolioValueTrend(range.value);
    } catch (e) {
      // 失败允许同键重试
      lastFetchedKey = null;
      throw e;
    } finally {
      loading.value = false;
    }
  }

  /** 刷新走势数据（预设区间变化后由 watch 自动触发） */
  async function refresh() {
    await fetchTrend();
  }

  /** 强制重拉：重置同键去重短路后刷新（价格失效信号：键未变但数据已变） */
  async function forceRefresh() {
    lastFetchedKey = null;
    await refresh();
  }

  /** 预设区间切换意图（走势卡 NRadioGroup 回传）；区间是闭集字面量，无守卫语义。 */
  function setPreset(next: TrendRangePreset) {
    session.setTrendPreset(next);
  }

  // 内部自动刷新（watch / 挂载首刷 / 价格失效信号）治愈失败：不再产生未处理
  // rejection（spec 治愈清单①同款语义）；返回的 refresh 仍向外抛，由调用方处置。
  watch(preset, () => {
    void refresh().catch(() => {});
  });

  // 价格失效信号（ADR-0031）：同步实际写价后走势采样点（market_prices）
  // 已陈旧，强制重拉——不重置去重短路则重拉被吞、留下陈旧点（issue #238）。
  usePricesChanged(() => {
    void forceRefresh().catch(() => {});
  });

  onMounted(() => {
    void refresh().catch(() => {});
  });

  /** 采样点序列（统一形态，供图表与空态消费） */
  const trendPoints = computed(() =>
    (portfolioTrend.value?.points ?? []).map((p) => ({
      date: p.date,
      value: p.market_value_cents,
    })),
  );

  const chartSeries = computed(() => toTrendChartSeries(trendPoints.value));
  const isEmpty = computed(() => isTrendEmpty(trendPoints.value));

  /** 走势空态的补全状态（ADR-0122 决策 5 / issue #1377 三态判据）：读投影的
   * `backfill` 字段直出（后端只增字段，仅空采样点且库内有通道无历史序列标的时
   * 携带）；有采样点时恒为 null，空态渲染按三态分派。 */
  const backfill = computed(() => portfolioTrend.value?.backfill ?? null);

  /** 曲线金额币种：后端折算的本位币 */
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
