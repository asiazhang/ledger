import type { Syncable } from './common'

/** 商户（Merchant，ADR-0028）：交易付款对象/收入来源的参考数据字典行。 */
export interface Merchant extends Syncable {
  id: string
  name: string
  icon: string | null
  color: string | null
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
