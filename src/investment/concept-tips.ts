/**
 * 投资域口径说明的闭集（issue #1369）：概念键与作用域变体都是**已知闭集**，
 * 因此以联合类型建模而非裸露 string——拼错的概念键会拼出一个任何 locale 都没有的
 * key，vue-i18n 会把 key 原文渲染进句子中间，违反 ADR-0049「绝不显示 key 代号」。
 *
 * 不变量：每个 `CONCEPT_KEYS` 成员在两份 locale 的 `investments.concepts` 下都必须有
 * `<key>Tip` 文案（由 `ConceptLabel.test.ts` 逐项断言，漏写即红）。
 */

/** 有口径说明文案的概念闭集（键即 `investments.concepts.<key>Tip` 的 `<key>`） */
export const CONCEPT_KEYS = [
  "marketValue",
  "unrealizedPnl",
  "cumulativePnl",
  "cost",
  "price",
  "mwr",
  "mwrCumulative",
  "realizedPnl",
  "portfolioTrend",
  "instrumentTrend",
  "investableAssets",
] as const;

export type ConceptKey = (typeof CONCEPT_KEYS)[number];

/**
 * 作用域变体闭集（键即 `investments.concepts.scope<X>` 的 `<X>`）：
 * 概念文案只写「这个数是什么」，作用域差异（随不随筛选收窄、本内还是跨本折算）
 * 归变体句——同一份概念文案在持仓页是过滤子集、在首页是全量、在跨账本页是逐本
 * 折算合并，写进概念文案必有一处失真。
 *
 * 只覆盖**语境相关**的挂点：单行值不随筛选变化、或口径本身自带「不随筛选收窄」
 * 属性时（如资金加权收益率按完整历史计算）不挂变体，故本 prop 可选。
 */
export const CONCEPT_SCOPES = ["filtered", "wholeLedger", "crossBook"] as const;

export type ConceptScope = (typeof CONCEPT_SCOPES)[number];

/** 变体 → i18n 键名后缀（闭集映射，调用方不拼字符串） */
export const CONCEPT_SCOPE_KEY_SUFFIX: Record<ConceptScope, string> = {
  filtered: "Filtered",
  wholeLedger: "WholeLedger",
  crossBook: "CrossBook",
};
