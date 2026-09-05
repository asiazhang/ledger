// zh-CN（源语言）文案聚合：所有域 JSON 在此汇成一棵消息树，顶层键即域名
// （消息 key 形如 `common.language.label`）。源语言 eager 加载，其他 locale
// 由 i18n/index.ts 按需动态 import 本目录（懒加载）。
import common from './common.json'
import quickTimeRange from './quickTimeRange.json'
import dashboard from './dashboard.json'
import transactions from './transactions.json'
import search from './search.json'
import accounts from './accounts.json'
import reports from './reports.json'
import investments from './investments.json'
import items from './items.json'
import policies from './policies.json'
import physicalAssets from './physicalAssets.json'
import scheduled from './scheduled.json'
import budget from './budget.json'
import ai from './ai.json'
import settings from './settings.json'
import unlock from './unlock.json'
import errors from './errors.json'

export default {
  common,
  quickTimeRange,
  dashboard,
  transactions,
  search,
  accounts,
  reports,
  investments,
  items,
  policies,
  physicalAssets,
  scheduled,
  budget,
  ai,
  settings,
  unlock,
  errors,
}
