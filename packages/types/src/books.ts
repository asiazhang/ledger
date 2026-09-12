// 账本（Book）领域类型（issue #833 / ADR-0089）。
// 字段命名与 Rust 侧 serde 默认（snake_case）保持一致。

/** 一本账：身份即库目录（内含固定名 ledger.db），id 与展示名是注册表元数据。 */
export interface Book {
  /** 账本标识（注册表内的稳定句柄；UUID）。 */
  id: string
  /** 展示名。 */
  name: string
  /** 库目录（完整路径）。 */
  dir: string
}

/** 账本清单信息（列表命令返回）。 */
export interface BookListInfo {
  /** 全部已登记账本（注册表损坏时为空——清单不可信，不展示残片）。 */
  books: Book[]
  /** 活动账本 id；注册表损坏时 null（后端已回退默认目录建连）。 */
  active_id: string | null
  /** 登记变更（新建/切换/改名/移除）当前是否可用；false 时禁用变更入口。 */
  mutable: boolean
  /** 引导期回退原因（供界面显著提示）；null = 未回退。 */
  fallback_reason: string | null
}
