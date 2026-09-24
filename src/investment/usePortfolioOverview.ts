import { computed, onMounted, ref } from "vue";
import { api } from "@ledger/api";
import { useLoadable } from "@ledger/loadable";
import { useReferenceStore } from "@/stores/reference";
import type { Holding, InstrumentPriceChannel } from "@ledger/types";

/** 当前持仓概览的一行：Holding 行叠加标的字典与账户的展示信息。 */
export interface PortfolioRow {
  holdingId: string;
  accountId: string;
  /** 账户名称（参考数据缺失时为 null，展示降级为「-」） */
  accountName: string | null;
  instrumentId: string;
  symbol: string | null;
  instrumentName: string | null;
  quantity: number;
  costBasisCents: number;
  costCurrencyCode: string;
  latestPriceCents: number | null;
  latestPriceCurrencyCode: string | null;
  /** 净值日期：基金现价（= 最新公布单位净值）携带，持仓可见现价对应哪天的净值；股票恒 null */
  latestNavDate: string | null;
  /** 账户本位币市值（v_holdings 实时计算；无行情时为 null） */
  marketValueCents: number | null;
  /** 账户本位币未实现盈亏（v_holdings 实时计算；无行情/汇率缺失时为 null） */
  unrealizedPnlCents: number | null;
  /** 折全局默认币种的市值（当期汇率逐行软折算，issue #1797；缺现价或缺折算汇率为 null） */
  nativeMarketValueCents: number | null;
  /** 折全局默认币种的未实现盈亏（当期汇率逐行软折算，issue #1797；空值语义同市值列） */
  nativeUnrealizedPnlCents: number | null;
  /** 市值/未实现盈亏的折算币种 = 账户币（账户缺失时回退成本币种） */
  valueCurrencyCode: string;
  /** 价格写入通道（标的字典透传的后端派生事实，issue #1060）：缺价行引导
   * （issue #1193）只读本事实，前端不再按类型与市场自行推断；标的字典缺行时
   * 为 null（无可读通道，不给引导） */
  priceChannel: InstrumentPriceChannel | null;
}

/**
 * 行集的折本位币合计（issue #1797，ADR-0131 修订）：合计三卡改折全局默认币种单值，
 * 多币种分组拼接的展示形态退役。缺料三态显式分离，展示层据此分流：
 * - 缺现价行 → `missingPriceCount`（未计入，卡面小字标注——既不虚增也不静默低估）；
 * - 有金额但缺账户币→本位币汇率的行 → `rateMissingCount`（整卡警告 + 重试，
 *   不给半截数字）；
 * - 其余行同币（DefaultCurrency）直接相加。
 */
export interface NativeTotal {
  /** 折本位币单值（分）；null = 无可计入行或缺折算汇率行在场（不给半截数字，展示「-」/警告态） */
  cents: number | null;
  /** 缺现价未计入的行数（>0 卡面小字标注） */
  missingPriceCount: number;
  /** 有金额但缺折算汇率的行数（>0 → 整卡警告 + 重试） */
  rateMissingCount: number;
}

/** 合计卡单卡形态（issue #1797）：行集合计或命令透传 + 报错文案（码化上抛，如累计收益缺汇率） */
export type StatCardValue = NativeTotal & {
  /** 命令报错文案（置位即整卡警告 + 重试）；行集合计恒为 null */
  error: string | null;
};

/**
 * 行集（全量或过滤子集）→ 市值/收益两卡的单值装配：缺价计数、缺汇率计数与同币求和
 * 的唯一表达式——全量口径（usePortfolioOverview，首页）与过滤子集口径
 * （useHoldingsFilter，持仓页签）共用，不复制第二份。
 */
export function rowStatCardValues(rows: PortfolioRow[]): {
  marketValue: StatCardValue;
  unrealizedPnl: StatCardValue;
} {
  return {
    marketValue: {
      ...nativeRowTotal(
        rows.map((r) => ({ cents: r.marketValueCents, nativeCents: r.nativeMarketValueCents })),
      ),
      error: null,
    },
    unrealizedPnl: {
      ...nativeRowTotal(
        rows.map((r) => ({
          cents: r.unrealizedPnlCents,
          nativeCents: r.nativeUnrealizedPnlCents,
        })),
      ),
      error: null,
    },
  };
}

/** 行集的单列（市值或未实现盈亏）→ 折本位币合计：缺料三态分离的单点（见 NativeTotal） */
function nativeRowTotal(rows: { cents: number | null; nativeCents: number | null }[]): NativeTotal {
  let cents: number | null = null;
  let missingPriceCount = 0;
  let rateMissingCount = 0;
  for (const r of rows) {
    if (r.cents === null) missingPriceCount++;
    else if (r.nativeCents === null) rateMissingCount++;
    else cents = (cents ?? 0) + r.nativeCents;
  }
  // 缺折算汇率行在场即不给半截数字（NativeTotal 语义）：合计置空、整卡走警告态
  if (rateMissingCount > 0) cents = null;
  return { cents, missingPriceCount, rateMissingCount };
}

/** 一次拉全「持仓标的」字典的每页条数上限（list_instruments 单页上限） */
const INVESTED_INSTRUMENT_FETCH_LIMIT = 500;

/**
 * 盈亏页持仓概览数据层（issue #110 / T6；issue #324 起为 Loadable 之上的薄壳，ADR-0040）：
 * 从 `list_holdings`（v_holdings 视图 + 逐行当期汇率折本位币，issue #1797）取当前持仓行，
 * 并与「持仓标的」字典（only_invested=true，与增量同步同口径）及账户信息拼装出可展示的
 * 明细与合计三卡的单值。
 *
 * 两路读各自独立 Loadable（issue #1797）：持仓行（list_holdings + list_instruments）与
 * 累计收益·折本位币单值（cumulative_pnl_native_total）并行发起、各自竞态裁决与错误
 * 状态——缺折算汇率码化上抛只降级累计收益卡（警告 + 重试），不拖累持仓行装配
 * （表格的账户币展示不依赖折算）。
 *
 * loading 置收、错误捕获与文案归一、错误展示（默认 toast + error 双通道）、
 * 竞态裁决全部内化进 Loadable；失败不向上抛（首刷/刷新不再产生未处理 rejection，
 * spec 治愈清单①）：error 置位 + 默认 toast，rows/cumulativePnl 保持原值不清空成空态。
 */
export function usePortfolioOverview() {
  const reference = useReferenceStore();

  const rows = ref<PortfolioRow[]>([]);

  const { loading, error, run } = useLoadable(async () => {
    const [holdings, invested] = await Promise.all([
      api.listHoldings(),
      // 「持仓标的」字典：有当前持仓批次（remaining_quantity > 0），与增量同步同口径
      api.listInstruments({
        only_invested: true,
        // 持仓标的一般远少于标的数；list_instruments 单页上限即此值
        page_size: INVESTED_INSTRUMENT_FETCH_LIMIT,
      }),
    ]);
    // 行数通常远小于标的数，直接按 id 建 map（O(n+m)）
    const instrumentMap = new Map(invested.items.map((i) => [i.id, i]));
    return holdings.map((h: Holding) => toRow(h, instrumentMap, reference.accountMap));
  });

  // 累计收益·折本位币单值（issue #1797）：后端三腿逐行当期汇率折 DefaultCurrency 后
  // 求和（缺折算汇率码化上抛、缺价持仓的未实现腿按空值跳过），是**全账本**口径、
  // 不随持仓页签的搜索/账户过滤收窄——已实现腿来自平仓匹配、无法归到某一行可见持仓。
  // 持仓页签合计区与首页投资卡共用本结果。
  const { error: cumulativeError, run: runCumulative } = useLoadable(() =>
    api.cumulativePnlNativeTotal(),
  );
  const cumulativeLoaded = ref<{ cents: number; currencyCode: string } | null>(null);

  async function refresh() {
    // 两路并行发起，互不等待也互不拖累；各自失败回原值（error 已置位）、迟到前发
    // 结果已被各自 Loadable 竞态裁决作废，不会覆写终态
    const [rowResult, cumulativeResult] = await Promise.all([run(), runCumulative()]);
    if (rowResult !== null) rows.value = rowResult;
    if (cumulativeResult !== null) {
      cumulativeLoaded.value = {
        cents: cumulativeResult.total_cents,
        currencyCode: cumulativeResult.native_currency,
      };
    }
  }

  // 全量口径（首页投资卡）：随 rows 全集合计
  const statCards = computed(() => rowStatCardValues(rows.value));
  // 过滤子集口径（持仓页签合计区）由 useHoldingsFilter 复用 rowStatCardValues 派生

  /** 累计收益卡：命令透传单值 + 报错文案（缺汇率码化上抛 → 整卡警告 + 重试） */
  const cumulativePnl = computed<StatCardValue>(() => ({
    cents: cumulativeLoaded.value?.cents ?? null,
    missingPriceCount: 0,
    rateMissingCount: 0,
    error: cumulativeError.value,
  }));

  /** 折算基准币种（全局默认币种，累计收益命令透传）：三卡共用，取符号与标注口径 */
  const nativeCurrency = computed(() => cumulativeLoaded.value?.currencyCode ?? null);

  onMounted(() => {
    void refresh();
  });

  return {
    rows,
    loading,
    error,
    statCards,
    cumulativePnl,
    nativeCurrency,
    refresh,
  };
}

interface InstrumentLike {
  symbol: string;
  name: string | null;
  price_channel: InstrumentPriceChannel;
}

interface AccountLike {
  name: string;
  currency_code: string;
}

/** Holding 行 + 标的字典 + 账户参考数据 → 概览明细行 */
function toRow(
  h: Holding,
  instrumentMap: Map<string, InstrumentLike>,
  accountMap: Map<string, AccountLike>,
): PortfolioRow {
  const inst = instrumentMap.get(h.instrument_id);
  const acct = accountMap.get(h.account_id);
  return {
    holdingId: h.id,
    accountId: h.account_id,
    accountName: acct?.name ?? null,
    instrumentId: h.instrument_id,
    symbol: inst?.symbol ?? null,
    instrumentName: inst?.name ?? null,
    quantity: h.quantity,
    costBasisCents: h.cost_basis_cents,
    costCurrencyCode: h.cost_currency_code,
    latestPriceCents: h.latest_price_cents,
    latestPriceCurrencyCode: h.latest_price_currency_code,
    latestNavDate: h.latest_nav_date,
    marketValueCents: h.market_value_cents,
    unrealizedPnlCents: h.unrealized_pnl_cents,
    // 折本位币列随行透传（后端逐行当期汇率软折算，issue #1797），合计卡消费
    nativeMarketValueCents: h.native_market_value_cents,
    nativeUnrealizedPnlCents: h.native_unrealized_pnl_cents,
    // 市值/未实现盈亏由 v_holdings 折算到账户本位币；账户缺失时回退成本币种保证可展示
    valueCurrencyCode: acct?.currency_code ?? h.cost_currency_code,
    // 价格通道随标的行透传（后端派生单点），缺价行引导据此分流
    priceChannel: inst?.price_channel ?? null,
  };
}
