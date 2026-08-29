import type { Syncable } from './common'

/** 商户（Merchant，ADR-0028）：交易付款对象/收入来源的参考数据字典行。 */
export interface Merchant extends Syncable {
  id: string
  name: string
  icon: string | null
  color: string | null
  /** 软删标记：仅随含软删全量列表（includeDeleted=true）返回软删行为 true（issue #191） */
  is_deleted: boolean
}

export interface MerchantInput {
  name: string
  icon?: string | null
  color?: string | null
}

/** 更新入参：字段均可省略（省略即保持原值）；改名须避开在用同名。 */
export interface MerchantUpdateInput {
  name?: string
  icon?: string | null
  color?: string | null
}
