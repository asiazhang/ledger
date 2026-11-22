import { formatLocalDateISO } from "./date";
import { t } from "@ledger/i18n";

export type DatedTimePeriodPreset = "month" | "quarter" | "year" | "lastYear";

export type TimePeriodPreset = "all" | DatedTimePeriodPreset;

export const DATED_TIME_PERIOD_PRESETS: readonly DatedTimePeriodPreset[] = [
  "month",
  "quarter",
  "year",
  "lastYear",
];

export const TIME_PERIOD_PRESETS: readonly TimePeriodPreset[] = [
  "all",
  ...DATED_TIME_PERIOD_PRESETS,
];

export interface DateRange {
  from: string;
  to: string;
}

export interface NullableDateRange {
  from: string | null;
  to: string | null;
}

const iso = formatLocalDateISO;

function lastDayOf(y: number, m0: number): number {
  return new Date(y, m0 + 1, 0).getDate();
}

function monthRange(y: number, m0: number): DateRange {
  return { from: iso(y, m0, 1), to: iso(y, m0, lastDayOf(y, m0)) };
}

function quarterRange(y: number, q: number): DateRange {
  const startMonth = q * 3;
  return { from: iso(y, startMonth, 1), to: iso(y, startMonth + 2, lastDayOf(y, startMonth + 2)) };
}

export function yearRange(y: number): DateRange {
  return { from: iso(y, 0, 1), to: iso(y, 11, 31) };
}

export function presetRange(preset: DatedTimePeriodPreset, today: number | Date): DateRange {
  const d = typeof today === "number" ? new Date(today) : today;
  const y = d.getFullYear();
  switch (preset) {
    case "month":
      return monthRange(y, d.getMonth());
    case "quarter":
      return quarterRange(y, Math.floor(d.getMonth() / 3));
    case "year":
      return yearRange(y);
    case "lastYear":
      return yearRange(y - 1);
  }
}

export type PeriodUnit = "month" | "quarter" | "year";

export interface NaturalPeriod {
  unit: PeriodUnit;
  year: number;
  index: number;
}

export interface PeriodBoundary {
  earliest: NaturalPeriod;
  latest: NaturalPeriod;
}

function periodValue(p: NaturalPeriod): number {
  const perYear = p.unit === "month" ? 12 : p.unit === "quarter" ? 4 : 1;
  return p.year * perYear + p.index;
}

export function comparePeriods(a: NaturalPeriod, b: NaturalPeriod): number {
  return periodValue(a) - periodValue(b);
}

export function derivePeriodBoundary(
  unit: PeriodUnit,
  dateRange: { min_date: string | null; max_date: string | null } | null | undefined,
  today: number | Date,
): PeriodBoundary {
  const d = typeof today === "number" ? new Date(today) : today;
  const currentYear = d.getFullYear();
  const currentMonth = d.getMonth();
  const currentIndex =
    unit === "month" ? currentMonth : unit === "quarter" ? Math.floor(currentMonth / 3) : 0;
  const current: NaturalPeriod = { unit, year: currentYear, index: currentIndex };

  if (!dateRange || !dateRange.min_date || !dateRange.max_date) {
    return { earliest: current, latest: current };
  }

  const minParsed = parseISODate(dateRange.min_date);
  const maxParsed = parseISODate(dateRange.max_date);
  if (!minParsed || !maxParsed) {
    return { earliest: current, latest: current };
  }

  const minIndex =
    unit === "month" ? minParsed.m0 : unit === "quarter" ? Math.floor(minParsed.m0 / 3) : 0;
  const earliestFromData: NaturalPeriod = { unit, year: minParsed.y, index: minIndex };
  const earliest =
    periodValue(earliestFromData) < periodValue(current) ? earliestFromData : current;

  const maxIndex =
    unit === "month" ? maxParsed.m0 : unit === "quarter" ? Math.floor(maxParsed.m0 / 3) : 0;
  const maxTxPeriod: NaturalPeriod = { unit, year: maxParsed.y, index: maxIndex };

  const latest = periodValue(maxTxPeriod) > periodValue(current) ? maxTxPeriod : current;

  return { earliest, latest };
}

export function deriveAllPeriodBoundaries(
  dateRange: { min_date: string | null; max_date: string | null } | null | undefined,
  today: number | Date,
): Record<PeriodUnit, PeriodBoundary> {
  return {
    month: derivePeriodBoundary("month", dateRange, today),
    quarter: derivePeriodBoundary("quarter", dateRange, today),
    year: derivePeriodBoundary("year", dateRange, today),
  };
}

export function isPeriodWithinBoundary(p: NaturalPeriod, boundary: PeriodBoundary): boolean {
  if (p.unit !== boundary.earliest.unit || p.unit !== boundary.latest.unit) {
    return false;
  }
  const val = periodValue(p);
  return val >= periodValue(boundary.earliest) && val <= periodValue(boundary.latest);
}

export function clampPeriod(p: NaturalPeriod, boundary: PeriodBoundary): NaturalPeriod {
  if (periodValue(p) < periodValue(boundary.earliest)) {
    return boundary.earliest;
  }
  if (periodValue(p) > periodValue(boundary.latest)) {
    return boundary.latest;
  }
  return p;
}

export function canStepPeriod(
  p: NaturalPeriod,
  delta: number,
  boundary?: PeriodBoundary | null,
): boolean {
  if (!boundary) return true;
  const next = stepPeriod(p, delta);
  return isPeriodWithinBoundary(next, boundary);
}

function parseISODate(s: string): { y: number; m0: number; day: number } | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s);
  if (!m) return null;
  const y = Number(m[1]);
  const m0 = Number(m[2]) - 1;
  const day = Number(m[3]);
  if (m0 < 0 || m0 > 11) return null;
  if (day < 1 || day > lastDayOf(y, m0)) return null;
  return { y, m0, day };
}

export function rangeToPeriod(from: string | null, to: string | null): NaturalPeriod | null {
  if (from === null || to === null) return null;
  const f = parseISODate(from);
  const end = parseISODate(to);
  if (!f || !end) return null;
  if (f.m0 === 0 && f.day === 1 && end.y === f.y && end.m0 === 11 && end.day === 31) {
    return { unit: "year", year: f.y, index: 0 };
  }
  const fq = Math.floor(f.m0 / 3);
  if (
    end.y === f.y &&
    f.m0 === fq * 3 &&
    end.m0 === fq * 3 + 2 &&
    f.day === 1 &&
    end.day === lastDayOf(end.y, end.m0)
  ) {
    return { unit: "quarter", year: f.y, index: fq };
  }
  if (f.y === end.y && f.m0 === end.m0 && f.day === 1 && end.day === lastDayOf(end.y, end.m0)) {
    return { unit: "month", year: f.y, index: f.m0 };
  }
  return null;
}

export function periodFromTimestamp(unit: PeriodUnit, timestamp: number): NaturalPeriod {
  const d = new Date(timestamp);
  return {
    unit,
    year: d.getFullYear(),
    index: unit === "month" ? d.getMonth() : unit === "quarter" ? Math.floor(d.getMonth() / 3) : 0,
  };
}

export function periodStartTimestamp(p: NaturalPeriod): number {
  const month = p.unit === "month" ? p.index : p.unit === "quarter" ? p.index * 3 : 0;
  return new Date(p.year, month, 1, 12).getTime();
}

export function periodRange(p: NaturalPeriod): DateRange {
  switch (p.unit) {
    case "month":
      return monthRange(p.year, p.index);
    case "quarter":
      return quarterRange(p.year, p.index);
    case "year":
      return yearRange(p.year);
  }
}

export function stepPeriod(p: NaturalPeriod, delta: number): NaturalPeriod {
  if (p.unit === "year") {
    return { unit: "year", year: p.year + delta, index: 0 };
  }
  const perYear = p.unit === "month" ? 12 : 4;
  const total = p.year * perYear + p.index + delta;
  const year = Math.floor(total / perYear);
  return { unit: p.unit, year, index: total - year * perYear };
}

export function formatPeriodLabel(p: NaturalPeriod): string {
  const year = String(p.year);
  if (p.unit === "year") {
    return t("quickTimeRange.periodLabel.year", { year });
  }
  const namesKey = p.unit === "month" ? "monthNames" : "quarterNames";
  const name = t(`quickTimeRange.periodLabel.${namesKey}.${p.index + 1}`);
  const key = p.unit === "month" ? "month" : "quarter";
  return t(`quickTimeRange.periodLabel.${key}`, { year, name });
}

export function matchPreset(
  from: string | null,
  to: string | null,
  today: number | Date,
): TimePeriodPreset | null {
  if (from === null && to === null) return "all";
  if (from === null || to === null) return null;
  const d = typeof today === "number" ? new Date(today) : today;
  const hit = DATED_TIME_PERIOD_PRESETS.find((p) => {
    const r = presetRange(p, d);
    return r.from === from && r.to === to;
  });
  return hit ?? null;
}
