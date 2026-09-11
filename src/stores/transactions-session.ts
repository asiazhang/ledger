import { defineStore } from 'pinia'
import { reactive, readonly, ref, toRaw, watch } from 'vue'
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

/** 商户排行下钻跳转的收支类型集合（issue #589）：支出 + 退款，与商户排行聚合的参与
 *  类型同源（退款减除进柱值；income 可携带商户但不进排行，故不含）。URL 编码为逗号
 *  分隔闭集字面量，与后端列表过滤契约的 HTTP 查询串 `kinds=expense,refund` 同一约定；
 *  消费方在 TransactionFilter 类型集合维度按同表解析。字面量经 satisfies 钉在
 *  TransactionKind 闭集内：kind 字面量改名时此处编译报错，而非跳转载荷静默失效。
 *  与 CATEGORY_DRILLDOWN_KIND_TOKENS 同值但独立定义：商户口径（income 不进排行）是
 *  独立于分类口径的领域决策，不复用分类 token，避免分类 token 未来演化静默污染商户口径。 */
const MERCHANT_DRILLDOWN_KIND_TOKENS = ['expense', 'refund'] as const satisfies readonly TransactionKind[]
export const MERCHANT_DRILLDOWN_KINDS = MERCHANT_DRILLDOWN_KIND_TOKENS.join(',')

/** URL 日期参数格式（issue #380）：YYYY-MM-DD，月/日限定在可能范围内（01-12 / 01-31）；
 * 非法格式视为参数不在场（回退不过滤）。不校验日历真实性（如 02-30 可通过）：后端按
 * 字典序比较，此类手工构造的畸形参数得到的是平移的边界而非报错——应用内跳转载荷
 * 恒为合法自然年边界，该形态仅手工构造 URL 可达。 */
const DATE_PARAM_PATTERN = /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])$/

/** 页大小默认档：会话内保留、冷启动回此默认（issue #893）。 */
export const TRANSACTION_PAGE_SIZE_DEFAULT = 20

/**
 * 交易列表过滤维度（会话级 store 是唯一事实源，URL 仅只读初始化入口、不写回）。
 * 字段与 `TransactionListFilter` 请求参数一一对应（见视图 load 装配）。
 */
export interface TransactionFilters {
  /** 日期起止过滤（YYYY-MM-DD，与后端 date 字典序一致，含边界） */
  dateFrom: string | null
  dateTo: string | null
  /** 涉及账户过滤（account_id、to_account_id 或出资账户 funding_account_id 任一命中即算；issue #97/#98、#937） */
  involvingAccountId: string | null
  /** 商户过滤（含软删商户，历史交易口径；issue #191） */
  merchantId: string | null
  /** 分类过滤维度（issue #377，三态）：分类 id = 精确过滤（不含子分类，含软删分类）；
   * UNCATEGORIZED_ONLY 哨兵 = 仅无分类；null = 不过滤。URL 下钻只读入口，无手动控件。 */
  categoryId: string | null
  /** 类型维度（spec #1025 单维化，原 issue #581，手动多选 + 下钻共用）：列表页类型
   * 下拉的多选集合与 URL ?kinds= 携带的下钻载荷（逗号分隔闭集字面量解析为字面量
   * 数组）共用本字段。维度内多值取或、与其余维度 AND 组合；类型为闭集字面量、
   * 无参考数据映射、不涉保留值，挂起补判/让位/复位守卫同规。空集合 ≡ 不过滤 ≡
   * 默认态（选满全部可选类型不归一：类型是可扩闭集）；载荷在场即覆盖手动多选
   * （URL 永远赢，ADR-0094）。与「仅无分类」解耦：仅无分类命中一切无分类交易、
   * 不限定类型，收支限定由本维度承担。原「单值 kind 手动维度 + 下钻专用集合」
   * 两套表示并存、同携取交集的形态已退役（BREAKING，见 CHANGELOG）。 */
  kinds: readonly TransactionKind[] | null
}

/** 部分过滤意图：只声明要改的维度，未提及维度保持不变。 */
export type TransactionFilterPatch = {
  [K in keyof TransactionFilters]?: TransactionFilters[K]
}

/** 路由 query 的结构子集（与 vue-router LocationQuery 兼容）：
 * 值可能为 string、数组或 null，非字符串值一律视为参数不在场。 */
export type TransactionUrlQuery = Readonly<Record<string, unknown>>

/** 默认过滤态（全量列表）：初始态与 resetFilters 的复位终态共用同一来源，
 * 新增维度只需改这一处。 */
const DEFAULT_FILTERS: TransactionFilters = {
  dateFrom: null,
  dateTo: null,
  involvingAccountId: null,
  merchantId: null,
  categoryId: null,
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

/** 运行时条目 = 声明行 + 消费状态（「单次进入」边界的运行时簿记，随访次复位）。 */
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
 * 交易页会话状态 store（issue #893）：TransactionFilter 过滤状态与会话内保留语义的
 * 唯一读写方——筛选全维（账户/商户/分类三态/类型集合/日期边界，spec #1025 起类型
 * 维度单维化）+ 页码 + 页大小提升到会话生命周期（ADR-0094，修订 ADR-0061 决策 2/4
 * 的逐票裁决口径）。
 *
 * 「会话内保留」语义（spec #892，先例 reports-session issue #427）：同一应用会话内，
 * 离开交易页再回来（侧栏切换、下钻往返、回退）= 回到离开时的样子，数据按恢复的
 * 选择现拉；应用冷启动（新 pinia）回默认无筛选态。保留全程零写盘（不进
 * localStorage / SQLite）、不写回 URL（ADR-0030）。
 *
 * 「意图进、状态出」的深模块，规则内化其中（原 useTransactionFilter 工厂逻辑随状态
 * 迁入，工厂退为薄适配，对外接口不变）：
 * - 统一出口 `apply()`：全仓唯一「翻页归零 + 版本 bump」，全部意图入口与 URL 参数
 *   应用都经它生效；
 * - URL 参数表：解析、校验、复位规则、参考数据就绪补判与字段级让位（ADR-0030）；
 *   参数表运行时状态（登记/结算/让位簿记）是「单次进入」边界——随访次（工厂调用）
 *   复位，URL 参数在场即重新消费、永远覆盖保留态对应维度，不抗跨访问的旧手动状态
 *   （就绪补判随会话级 store 常驻：访次结束后才就绪的挂起参数仍会补判落入保留态，
 *   URL 意图迟到兑现，下次进入按保留态呈现）；
 * - 首拉不在本 store：消费方 setup 期 immediate 读当前模块状态拉取（默认态 = 冷启动
 *   默认口径；恢复访次 = 离开时的选择），与「模块只产出状态与版本信号」边界一致。
 */
export const useTransactionsSessionStore = defineStore('transactions-session', () => {
  const reference = useReferenceStore()

  /** 过滤状态（会话内保留；对外以只读投影暴露）。 */
  const filters = reactive<TransactionFilters>({ ...DEFAULT_FILTERS })
  /** 页码（分页归模块所有）：翻页导航由调用方直写并自行重拉，「翻页归零」只发生在
   * 统一出口，「删除后超页回退」只发生在 afterRowDelete 入口（ADR-0045）。 */
  const page = ref(1)
  /** 页大小（分页归模块所有）：切换后调用方经 refresh() 走统一出口。 */
  const pageSize = ref(TRANSACTION_PAGE_SIZE_DEFAULT)
  /** 重拉版本号：bump 即「需以当前模块状态重新拉取列表」，是唯一重拉信号。 */
  const refreshVersion = ref(0)

  /** URL 参数表运行时条目（按声明表顺序初始化；消费状态随访次复位）。 */
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

  /** 保留态是否处于默认（无筛选 + 第 1 页 + 默认页大小）：复位幂等判定
   * （ESC 复位与清除按钮共用出口，「无保留状态无操作」）的单一谓词（issue #893）。 */
  function isAtDefault(): boolean {
    return (
      !Object.values(filters).some((v) => v !== null) &&
      page.value === 1 &&
      pageSize.value === TRANSACTION_PAGE_SIZE_DEFAULT
    )
  }

  /**
   * 访次开始（消费方工厂每次调用即一次进入，issue #893）：URL 参数表运行时簿记复位到
   * 「单次进入」边界——本访次内登记的参数才参与挂起/补判/让位，跨访问的旧手动状态
   * 不抗参数（URL 永远赢，spec #892）。保留态本身不动：首拉由消费方在 setup 期
   * immediate 读当前模块状态发起（默认态以默认态拉取、恢复访次以保留态拉取，
   * 不经 refresh 出口的翻回第一页语义），与「模块只产出状态与版本信号」边界一致。
   */
  function beginVisit() {
    urlParams.forEach((e) => {
      e.raw = null
      e.consumed = false
      e.manualTouched = false
    })
  }

  /** 统一出口：翻页归零 + 版本 bump ——「翻页归零 + 刷新」全仓仅此一处。 */
  function apply() {
    page.value = 1
    refreshVersion.value += 1
  }

  /** 同值判定：对象值（类型集合数组）经 toRaw 解引用——reactive 深代理读取嵌套
   * 对象返回新代理，直接 === 会对同一数组的重复声明误判为变化（重复出口）。 */
  function isSameValue(a: unknown, b: unknown): boolean {
    if (a === b) return true
    return (
      a !== null && b !== null && typeof a === 'object' && typeof b === 'object' && toRaw(a) === b
    )
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
      if (value === undefined || isSameValue((filters as Record<keyof TransactionFilters, unknown>)[key], value)) return
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
    // 无可复位保留态时幂等不动作（ESC 复位的「无保留状态无操作」，issue #893）：
    // 激活判定是「任一保留态偏离默认」（含纯翻页/页大小偏离，同属保留态）——
    // 复位即清除保留态本身（复位后离开再回来 = 默认）。注意清除筛选按钮的禁用态
    // （视图 filtersActive）仅反映过滤维度，与这里的激活判定不同、出口同为此处。
    if (isAtDefault()) return
    Object.assign(filters, DEFAULT_FILTERS)
    pageSize.value = TRANSACTION_PAGE_SIZE_DEFAULT
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
   * 「不翻页的静默重拉」接口扩展，而非复用 refresh 语义。恢复页码超出有效范围的
   * 钳制（issue #893）也走本入口逐页回退，不新增第二出口。 */
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
      (filters.dateFrom !== null || filters.dateTo !== null || filters.kinds?.length)
    ) {
      mutate({ dateFrom: null, dateTo: null, kinds: null }, false)
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
  // watch 随会话级 store 常驻（非组件作用域）：访次结束后才就绪的挂起参数仍会补判
  // 并落入保留态（URL 意图迟到兑现，下次进入按保留态呈现）；期间版本 bump 无监听者、
  // 无副作用。
  watch(
    () => reference.status,
    (status) => {
      if (status !== 'ready') return
      urlParams.forEach(settleEntry)
    },
  )

  return {
    // 只读状态投影（写路径全部内化为本 store 动作）
    filters: readonly(filters),
    // 可写状态仅限分页直写语义（翻页导航由调用方直写，ADR-0008/0030 分页契约）
    page,
    pageSize,
    refreshVersion,
    // 意图入口（useTransactionFilter 工厂对外接口的唯一实现处）
    beginVisit,
    setFilter,
    resetFilters,
    refresh,
    afterRowDelete,
    syncUrlQuery,
  }
})
