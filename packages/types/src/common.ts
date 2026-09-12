/** 可同步基础字段（本地-远端双向同步） */
export interface Syncable {
  updated_at: string
  version: number
  device_id: string
  is_deleted: boolean
}
