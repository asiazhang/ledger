import { computed, ref, type ComputedRef, type Ref } from "vue";
import { useInstrumentSearch } from "@/investment/useInstrumentSearch";
import type { Instrument } from "@ledger/types";

/** 标的下拉选项：naive-ui options 的最小闭集（label + value）。 */
export interface InstrumentOption {
  label: string;
  value: string;
}

/** 标的引用：钉住标的的最小结构闭集（Instrument 结构兼容，编辑明细投影可就地构造）。 */
export interface InstrumentRef {
  id: string;
  symbol: string;
  name: string | null;
}

/**
 * 「代码 · 名称」label 拼法单点（issue #1798）：标的下拉候选与钉住项共用同一拼法，
 * 无名称（含空串）退化裸代码。同族异形不归本单点：持仓搜索判定文本
 *（holdingsSearchLabel，判定口径）与标的展示名（instrumentDisplayLabel，
 * 空格形、与后端 display_label 同口径，明细来源列与走势页签共用）各有自己的单点。
 */
export function instrumentOptionLabel(symbol: string, name: string | null): string {
  return name ? `${symbol} · ${name}` : symbol;
}

/** 原始标的 → 下拉选项投影（label 拼法经 instrumentOptionLabel 单点）。 */
export function toInstrumentOption(i: InstrumentRef): InstrumentOption {
  return { label: instrumentOptionLabel(i.symbol, i.name), value: i.id };
}

/**
 * 标的下拉候选面域件（issue #1798）：搜索候选投影 + 钉住合并的领域形态收口——
 * 在 useInstrumentSearch（issue #1308，取数编排：防抖、空查询清空、吞错、在途
 * 竞态）之上内化「候选投影（label 拼法单点）+ 钉住标的防丢失合并」，盈亏筛选
 * （useRealizedPnl）与投资表单（useInvestmentForm）共用本域件，第三份同形拷贝
 * 不再出现。
 *
 * 钉住语义：已选中 / 编辑回填的标的必须留在候选面里可见（下拉不出现裸 id）——
 * 搜索结果更替不冲掉钉住项，已含于结果则去重不重复。合入端 `pinnedAt` 是消费方
 * 决策：筛选回显钉尾部（不打扰搜索结果序，useRealizedPnl），编辑回填钉头部
 * （回填项优先可见，useInvestmentForm）。
 *
 * 基金判定等需要原始标的字段的领域形态仍归消费方（#1308 决策 4），经
 * `findInstrument` 查原始候选；本域件不解释标的字段语义。
 */
export interface UseInstrumentOptionsReturn {
  /** 候选面：搜索结果投影 + 钉住标的防丢失合并；初始与清空输入后为空集 */
  options: ComputedRef<InstrumentOption[]>;
  /** 搜索在途标志：请求发出前置位，终态（结果落位 / 失败 / 空查询）置收 */
  searching: Ref<boolean>;
  /** 搜索意图入口：与 naive-ui `@search` 回传同形 */
  search(query: string): void;
  /** 钉住标的（选中回显 / 编辑回填）：投影后合入候选面合入端；null = 清除钉住 */
  pin(instrument: InstrumentRef | null): void;
  /** 按 id 查原始搜索候选（基金判定等消费方领域形态），未命中 undefined */
  findInstrument(id: string): Instrument | undefined;
}

export function useInstrumentOptions(pinnedAt: "head" | "tail"): UseInstrumentOptionsReturn {
  const { items, searching, search } = useInstrumentSearch();
  const pinned = ref<InstrumentRef | null>(null);

  const options = computed<InstrumentOption[]>(() => {
    const projected = items.value.map(toInstrumentOption);
    const pinnedOption = pinned.value ? toInstrumentOption(pinned.value) : null;
    if (!pinnedOption || projected.some((o) => o.value === pinnedOption.value)) {
      return projected;
    }
    return pinnedAt === "head" ? [pinnedOption, ...projected] : [...projected, pinnedOption];
  });

  function pin(instrument: InstrumentRef | null) {
    pinned.value = instrument;
  }

  function findInstrument(id: string): Instrument | undefined {
    return items.value.find((i) => i.id === id);
  }

  return { options, searching, search, pin, findInstrument };
}
