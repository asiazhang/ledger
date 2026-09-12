import type { Syncable } from './common'

/** 保司（Insurer，issue #712 / ADR-0082）：保险合同的承保机构在账本中的参考字典行，
 *  保险域自有独立字典，不复用核心交易域商户。名字字典：无 icon/color 等视觉字段。
 *  全新库迁移幂等预置 30 家常用国内保司（种子为普通字典行）。 */
export interface Insurer extends Syncable {
  id: string
  name: string
  /** 软删标记：仅随含已删全量列表（includeDeleted=true）返回软删行为 true */
  is_deleted: boolean
}

export interface InsurerInput {
  name: string
}

/** 更新入参：name 可省略（省略即保持原值）；改名须避开在用同名。 */
export interface InsurerUpdateInput {
  name?: string
}
