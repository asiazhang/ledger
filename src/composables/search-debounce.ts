/**
 * 搜索输入防抖时长（毫秒）单源（issue #1308 决策 5 / #1402 grilling 修订）：跨域
 * UX 不变量「搜索输入防抖」的唯一事实源——远程标的下拉（useInstrumentSearch）、
 * 标的浏览器列表读路径、持仓本地过滤、交易搜索页共用同一时长，改一处生效。
 * 落壳内跨域件：src/composables 不依赖任何域，消费方向单一（域 → 跨域件，
 * latest-wins / push-first-list 同向先例）。
 */
export const SEARCH_DEBOUNCE_MS = 300
