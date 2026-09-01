// en-US 文案聚合：与 zh-CN/index.ts 保持同一域清单（key 集合全等由
// scripts/check-i18n-keys.js 守门）。本模块仅在切换到 en-US 时被动态 import。
import common from './common.json'
import dashboard from './dashboard.json'
import transactions from './transactions.json'
import search from './search.json'
import accounts from './accounts.json'
import reports from './reports.json'
import investments from './investments.json'
import items from './items.json'
import scheduled from './scheduled.json'
import budget from './budget.json'
import ai from './ai.json'
import settings from './settings.json'
import errors from './errors.json'

export default {
  common,
  dashboard,
  transactions,
  search,
  accounts,
  reports,
  investments,
  items,
  scheduled,
  budget,
  ai,
  settings,
  errors,
}
