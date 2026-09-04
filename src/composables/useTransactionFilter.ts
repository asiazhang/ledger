import { reactive, readonly, ref, watch } from 'vue'
import type { Ref } from 'vue'
import { useReferenceStore } from '@/stores/reference'
import { TRANSACTION_KINDS } from '@/types'
import type { TransactionKind } from '@/types'

/** 「仅无分类」哨兵值（issue #377）：分类过滤维度三态之一（不过滤 null / 精确 id / 哨兵）。
 * 同时是 URL ?category= 的保留参数值；分类 id 为 UUID，与哨兵串不可能撞值。
 * 视图装配时哨兵映射为后端 `uncategorized_only`。 */
export const UNCATEGORIZED_ONLY = 'none'

/** 报表分类下钻跳转的收支类型集合（issue #581）：支出 + 退款，与分类聚合的参与类型
 * 同源（退款继承原分类、计入柱值）。URL 编码为逗号分隔闭集字面量，与后端列表过滤
 * 契约的 HTTP 查询串 `kinds=expense,refund` 同一约定；消费方在 TransactionFilter
 * 类型集合维度按同表解析。字面量经 satisfies 钉在 TransactionKind 闭集内：
 * kind 字面量改名时此处编译报错，而非跳转载荷静默失效。 */
const CATEGORY_DRILLDOWN_KIND_TOKENS = ['expense', 'refund'] as const satisfies readonly TransactionKind[]
export const CATEGORY_DRILLDOWN_KINDS = CATEGORY_DRILLDOWN_KIND_TOKENS.join(',')

/** URL 日期参数格式（issue #380）：YYYY-MM-DD，月/日限定在可能范围内（01-12 / 01-31）；
 * 非法格式视为参数不在场（回退不过滤）。不校验日历真实性（如 02-30 可通过）：后端按
 * 字典序比较，此类手工构造的畸形参数得到的是平移的边界而非报错——应用内跳转载荷
 * 恒为合法自然年边界，该形态仅手工构造 URL 可达。 */
const DATE_PARAM_PATTERN = /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])$/

/**
 * 交易列表过滤维度（组件状态是过滤的唯一事实源，URL 仅只读初始化入口、不写回）。
 * 字段与 `TransactionListFilter` 请求参数一一对应（见视图 load 装配）。
 */
export interface TransactionFilters {
  /** 日期起止过滤（YYYY-MM-DD，与后端 date 字典序一致，含边界） */
  dateFrom: string | null
  dateTo: string | null
  /** 涉及账户过滤（account_id 或 to_account_id 命中即算，含转入的转账；issue #97/#98） */
  involvingAccountId: string | null
  /** 商户过滤（含软删商户，历史交易口径；issue #191） */
  merchantId: string | null
  /** 分类过滤维度（issue #377，三态）：分类 id = 精确过滤（不含子分类，含软删分类）；
   * UNCATEGORIZED_ONLY 哨兵 = 仅无分类；null = 不过滤。URL 下钻只读入口，无手动控件。 */
  categoryId: string | null
  /** 交易类型过滤（前端 6 种：income/expense/transfer/refund/buy/sell） */
  kind: TransactionKind | null
  /** 类型集合维度（issue #581，下钻专用）：URL ?kinds= 携带的交易类型集合（逗号分隔
   * 闭集字面量解析为字面量数组），与其余维度 AND 组合。类型为闭集字面量、无参考数据
   * 映射、不涉保留值，挂起补判/让位/复位守卫同规；与单值 kind 手动维度解耦共存。
   * 消费方是报表分类下钻跳转（载荷显式携带与分类柱一致的收支类型集合）；与「仅无分类」
   * 解耦：仅无分类命中一切无分类交易、不限定类型，收支限定由本维度承担。 */
  kinds: readonly TransactionKind[] | null
}

/** 部分过滤意图：只声明要改的维度，未提及维度保持不变。 */
export type TransactionFilterPatch = {
  [K in keyof TransactionFilters]?: TransactionFilters[K]
}

/** 路由 query 的结构子集（与 vue-router LocationQuery 兼容）：
 * 值可能为 string、数组或 null，非字符串值一律视为参数不在场。 */
export type TransactionUrlQuery = Readonly<Record<string, unknown>>

export interface UseTransactionFilterReturn {
  /** 过滤状态（只读）：改动只能经意图入口，视图与测试均只读消费。 */
  readonly filters: Readonly<TransactionFilters>
  /** 页码（分页归模块所有）：翻页导航由调用方直写并自行重拉，「翻页归零」只发生在模块出口，
   * 「删除后超页回退」只发生在 afterRowDelete 入口（ADR-0045）。 */
  readonly page: Ref<number>
  /** 页大小（分页归模块所有）：切换后调用方经 refresh() 走统一出口。 */
  readonly pageSize: Ref<number>
  /** 重拉版本号：bump 即「需以当前模块状态重新拉取列表」，是唯一重拉信号。 */
  readonly refreshVersion: Ref<number>
  /** 声明部分过滤意图：任一维度实际变化 → 翻页归零 + 版本 bump（立即生效，无 debounce）。 */
  setFilter(patch: TransactionFilterPatch): void
  /** 清除全部过滤（显式动词）：有激活条件才动作，回默认态并走统一出口。 */
  resetFilters(): void
  /** 外部数据变化（记一笔/退款回填等）：重拉 + 翻回第一页，不动筛选。 */
  refresh(): void
  /** 页码回退入口（ADR-0045，删除路径专用）：声明「删除当前页一行后本页剩 N 条」——
   * N 为 0 且当前页非第一页时减一页（回退），然后一律版本 bump 重拉（不回退时保持当前页，
   * 不走 refresh 的「翻回第一页」语义）；筛选不动。 */
  afterRowDelete(remainingOnPage: number): void
  /** 递入最新路由 query（URL 下钻只读入口，issue #234）：模块按参数表逐维度解析、校验、
   * 复位与就绪补判；URL 只读不写回（#96 决策 3/4），视图只负责监听路由并转发。 */
  syncUrlQuery(query: TransactionUrlQuery): void
}

/** 默认过滤态（全量列表）：初始态与 resetFilters 的复位终态共用同一来源，
 * 新增维度只需改这一处。 */
const DEFAULT_FILTERS: TransactionFilters = {
  dateFrom: null,
  dateTo: null,
  involvingAccountId: null,
  merchantId: null,
  categoryId: null,
  kind: null,
  kinds: null,
}

/**
 * URL 下钻参数表条目（issue #234 / ADR-0030 决策 2）：一个维度一条，
 * query 键 → 过滤字段/参考数据映射/补丁构造的映射与消费状态。解析、校验、复位、补判、
 * 让位对每条同规则处理，视图不再感知下钻参数有几个（两条镜像解析链随之消灭），
 * 新增下钻维度只需在此表加一条。
 */
/** 条目校验规则（判别联合，issue #380/#581）：三类校验互斥，「既无映射又无格式」的非法
 * 组合在类型层不可表示——「新增下钻维度只需在此表加一条」由类型结构背书。 */
type UrlParamCheck =
  /** 参考数据映射校验（商户/分类含软删，历史交易口径 issue #191/#377）：
   * 保留参数值命中即有效（不查映射，issue #377，分类保留值 none → 仅无分类哨兵）；
   * 无保留值的维度省略。 */
  | {
      readonly mapKey: 'accountMap' | 'merchantMap' | 'categoryMap'
      readonly reservedValues?: ReadonlyArray<string>
    }
  /** 格式校验（issue #380，无参考数据映射的维度）：命中即有效，原样作为过滤字段值；
   * 不命中回退 null（参数视为不在场）。 */
  | { readonly pattern: RegExp }
  /** 闭集字面量校验（issue #581 类型集合维度，无参考数据映射）：逗号分隔串逐段命中
   * 字面量闭集即整串有效；任一段不命中整串视为不在场（回退不过滤，与日期维度
   * 「非法视为不在场」同规）。 */
  | { readonly literalSet: ReadonlyArray<TransactionKind> }

/** URL 下钻参数表条目（issue #234 / ADR-0030 决策 2）：一个维度一条，
 * query 键 → 过滤字段/校验规则/补丁构造的映射与消费状态。解析、校验、复位、补判、
 * 让位对每条同规则处理，视图不再感知下钻参数有几个（两条镜像解析链随之消灭），
 * 新增下钻维度只需在此表加一条。
 */
interface UrlParamDef {
  /** URL query 键（?account= / ?merchant= / ?category= / ?kinds=（issue #581） /
   * ?dateFrom= / ?dateTo=，issue #380） */
  readonly queryKey: 'account' | 'merchant' | 'category' | 'kinds' | 'dateFrom' | 'dateTo'
  /** 接管的过滤维度字段 */
  readonly field: keyof TransactionFilters
  /** 校验规则（映射查 id 或格式校验，判别联合） */
  readonly check: UrlParamCheck
  /** 校验结果 → 过滤补丁 */
  readonly toPatch: (value: string | null) => TransactionFilterPatch
}

/** 运行时条目 = 声明行 + 消费状态。 */
type UrlParamEntry = UrlParamDef & {
  /** 最近一次递入的原始参数（null = 不在场或非字符串；导航清除亦归此态） */
  raw: string | null
  /** 至多消费一次：应用或让位即结算；结算后参考数据重拉（status 再次 ready）不重放 */
  consumed: boolean
  /** 登记后、补判前用户手动改动同维度 → 让位（结算但不应用、不再重放） */
  manualTouched: boolean
}

/** URL 下钻参数表（声明态）。 */
const URL_PARAM_TABLE: ReadonlyArray<UrlParamDef> = [
  {
    queryKey: 'account',
    field: 'involvingAccountId',
    check: { mapKey: 'accountMap' },
    toPatch: (value) => ({ involvingAccountId: value }),
  },
  {
    queryKey: 'merchant',
    field: 'merchantId',
    check: { mapKey: 'merchantMap' },
    toPatch: (value) => ({ merchantId: value }),
  },
  {
    queryKey: 'category',
    field: 'categoryId',
    check: { mapKey: 'categoryMap', reservedValues: [UNCATEGORIZED_ONLY] },
    toPatch: (value) => ({ categoryId: value }),
  },
  {
    // 类型集合维度（issue #581）：报表分类下钻跳转载荷「分类 + 期间 + 收支类型集合」
    // 的类型集合部分。闭集字面量、无参考数据映射、不涉保留值；挂起补判/让位/复位
    // 守卫对每条同规则处理。
    queryKey: 'kinds',
    field: 'kinds',
    check: { literalSet: TRANSACTION_KINDS },
    toPatch: (value) => ({ kinds: value ? (value.split(',') as TransactionKind[]) : null }),
  },
  {
    // 日期边界维度（issue #380）：报表分类下钻跳转载荷「分类 + 所选年份首尾日期」的
    // 日期部分。无参考数据映射，按格式校验；挂起补判/让位/复位守卫对每条同规则处理。
    queryKey: 'dateFrom',
    field: 'dateFrom',
    check: { pattern: DATE_PARAM_PATTERN },
    toPatch: (value) => ({ dateFrom: value }),
  },
  {
    queryKey: 'dateTo',
    field: 'dateTo',
    check: { pattern: DATE_PARAM_PATTERN },
    toPatch: (value) => ({ dateTo: value }),
  },
]

/**
 * 交易列表过滤深模块（ADR-0030）：「用户意图进、列表状态出」。
 *
 * 工厂形态：每次调用返回独立实例（未来第二个消费者如搜索页需要独立状态，
 * 避免分页与补判串扰）。接口收敛为 setFilter / resetFilters / refresh / afterRowDelete /
 * syncUrlQuery 五个意图入口 + 可观察状态（filters、page、pageSize、refreshVersion），
 * 无其他公开面。
 *
 * URL 下钻参数表（issue #234）：视图只把 route query 变化递给 syncUrlQuery，解析与校验
 * （依赖参考数据映射与保留值表）、复位规则（所有下钻维度均无有效参数时复位日期/类型，#96 决策 3）、
 * 就绪补判（模块内部消费 Reference Data store，不向调用方暴露就绪通知）与字段级让位
 * （补判遇用户手动改动同维度则让位；参数至多消费一次、不重放）全部内化。
 *
 * 统一出口：`apply()` 是全仓唯一「翻页归零 + 刷新」——全部意图入口与 URL 参数应用
 * 都经它生效，「改筛选必翻页」「回填必翻页」由出口唯一性保证；同一同步批次内的
 * 多次 bump 由调用方 watcher 去重为一次请求。
 *
 * 边界（ADR-0030 决策 6）：模块只产出请求参数来源（状态）与版本信号（refreshVersion）；
 * 请求发起、loading、行数据仍归调用方。筛选与分页不持久化（ViewState 边界）。
 */
export function useTransactionFilter(): UseTransactionFilterReturn {
  const reference = useReferenceStore()

  const filters = reactive<TransactionFilters>({ ...DEFAULT_FILTERS })
  const page = ref(1)
  const pageSize = ref(20)
  const refreshVersion = ref(0)

  /** URL 参数表运行时条目（按声明表顺序初始化）。 */
  const urlParams: UrlParamEntry[] = URL_PARAM_TABLE.map((def) => ({
    ...def,
    raw: null,
    consumed: false,
    manualTouched: false,
  }))

  /** 手动意图可触碰的 URL 管理字段集合（让位判定用）。 */
  const urlManagedFields: ReadonlySet<keyof TransactionFilters> = new Set(
    URL_PARAM_TABLE.map((def) => def.field),
  )

  /** 统一出口：翻页归零 + 版本 bump ——「翻页归零 + 刷新」全仓仅此一处。 */
  function apply() {
    page.value = 1
    refreshVersion.value += 1
  }

  /**
   * 过滤写入唯一路径：逐键合并补丁，同值守卫（undefined 视为未声明、同值不动作），
   * 实际变化才走统一出口。manual 标记手动意图：触碰 URL 管理维度 → 挂起参数让位
   * （字段级，URL 参数内部应用不经此标记）。
   */
  function mutate(patch: TransactionFilterPatch, manual: boolean) {
    let changed = false
    ;(Object.keys(patch) as Array<keyof TransactionFilters>).forEach((key) => {
      const value = patch[key]
      if (value === undefined || filters[key] === value) return
      // 逐键写入：值类型已由 TransactionFilterPatch 的键值对应约束，
      // 此处仅为绕开联合键索引的宽化收窄
      ;(filters as Record<keyof TransactionFilters, unknown>)[key] = value
      changed = true
      if (manual && urlManagedFields.has(key)) {
        const entry = urlParams.find((e) => e.field === key)
        if (entry) entry.manualTouched = true
      }
    })
    if (changed) apply()
  }

  function setFilter(patch: TransactionFilterPatch) {
    mutate(patch, true)
  }

  function resetFilters() {
    // 无激活条件时幂等不动作（清除按钮禁用态的双重保险，保持既有 clearFilters 语义）
    if (!Object.values(filters).some((v) => v !== null)) return
    Object.assign(filters, DEFAULT_FILTERS)
    // 显式清空 = 对全部维度的手动改动：挂起中的 URL 参数一并让位（#234 字段级让位）
    urlParams.forEach((e) => {
      e.manualTouched = true
    })
    apply()
  }

  function refresh() {
    apply()
  }

  /** 页码回退入口（ADR-0045）：声明「删除当前页一行后本页剩 N 条」。回退判定用
   * 「删前本页仅 1 条」（N === 0 ⇔ 删后超页：offset 分页单条删除下严格等价，ADR-0008），
   * 免去回退前的第二次请求；并发新增导致的漂移沿用 ADR-0008 已知边界。
   * 「翻页归零」仍只在统一出口；本入口只回退不归零，是 ADR-0030 代价 3 预留的
   * 「不翻页的静默重拉」接口扩展，而非复用 refresh 语义。 */
  function afterRowDelete(remainingOnPage: number) {
    if (remainingOnPage === 0 && page.value > 1) {
      page.value -= 1
    }
    refreshVersion.value += 1
  }

  /** 条目原始参数解析为过滤字段值：保留值命中 → 原样（哨兵即字段值）；
   * 格式校验命中 → 原样（issue #380 日期维度）；字面量闭集整串命中 → 原样
   * （issue #581 类型集合维度，toPatch 再拆分）；映射命中 → 原样（分类 id 校验含软删，
   * 历史交易口径）；其余（不在场/未知/格式非法）→ null。 */
  function resolveValue(entry: UrlParamEntry): string | null {
    if (entry.raw === null) return null
    const check = entry.check
    if ('mapKey' in check) {
      if (check.reservedValues?.includes(entry.raw)) return entry.raw
      return reference[check.mapKey].has(entry.raw) ? entry.raw : null
    }
    if ('literalSet' in check) {
      return entry.raw.split(',').every((p) => check.literalSet.includes(p as TransactionKind))
        ? entry.raw
        : null
    }
    return check.pattern.test(entry.raw) ? entry.raw : null
  }

  /** 条目原始参数是否有效（保留值或参考数据映射命中）。 */
  function isValidRaw(entry: UrlParamEntry): boolean {
    return resolveValue(entry) !== null
  }

  /** 另一维度是否存在有效 URL 参数（复位守卫）：另一维度的直达参数在场时，
   * 本维度回退不得越界复位日期/类型，避免误清组合下钻的另一参数。 */
  function otherHasValidParam(entry: UrlParamEntry): boolean {
    return urlParams.some((e) => e !== entry && isValidRaw(e))
  }

  /**
   * 消费一个条目（至多一次）：
   * - 参数在场且参考数据未就绪 → 挂起（不误判为无效），待就绪补判；
   * - 用户已手动改动同维度 → 让位：结算但不应用、不再重放（其他维度不受牵连）；
   * - 参数不在场或已就绪：校验后应用——无效回退 null；回退且另一维度无有效参数时
   *   复位日期/类型（#96 决策 3）。复位与字段写入各经统一出口，同一同步批次内
   *   被调用方 watcher 去重为一次重拉。
   */
  function settleEntry(entry: UrlParamEntry) {
    if (entry.consumed) return
    if (entry.raw !== null && reference.status !== 'ready') return // 挂起，待就绪补判
    entry.consumed = true
    if (entry.manualTouched) return
    const next = resolveValue(entry)
    if (
      next === null &&
      !otherHasValidParam(entry) &&
      (filters.dateFrom !== null || filters.dateTo !== null || filters.kind !== null)
    ) {
      mutate({ dateFrom: null, dateTo: null, kind: null }, false)
    }
    mutate(entry.toPatch(next), false)
  }

  function syncUrlQuery(query: TransactionUrlQuery) {
    // 两阶段：先整表登记本趟 query，再逐条结算——复位守卫读到的是同一趟的
    // 完整参数表，不依赖条目顺序（与旧实现读「当前 query」的语义一致）；
    // 该维度参数未变化（Object.is，含同为不在场）不重登记、不重放——
    // 无关导航与参考数据重拉都不覆盖用户手动改动。
    const changed = urlParams.filter((entry) => {
      const value = query[entry.queryKey]
      const raw = typeof value === 'string' ? value : null
      if (raw === entry.raw) return false
      entry.raw = raw
      entry.consumed = false
      entry.manualTouched = false
      return true
    })
    changed.forEach(settleEntry)
  }

  // 参考数据就绪补判（ADR-0030 决策 5）：冷启动深链时参数挂起，status → ready 后
  // 逐条目补判一次（已结算条目幂等跳过）；error 状态不补判。不向调用方暴露就绪通知。
  watch(
    () => reference.status,
    (status) => {
      if (status !== 'ready') return
      urlParams.forEach(settleEntry)
    },
  )

  return {
    filters: readonly(filters),
    page,
    pageSize,
    refreshVersion,
    setFilter,
    resetFilters,
    refresh,
    afterRowDelete,
    syncUrlQuery,
  }
}
