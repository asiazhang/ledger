// 多端同步命令面类型（issue #862 / ADR-0091）：同步状态、通道配置与同步轮次
// 报告。字段命名与 Rust 侧 serde 默认（snake_case）保持一致。
// 通道配置是本机设备配置（不同步，同步边界见多端同步域 SyncBoundary）。

/// 同步状态（设置页同步卡片回显）。
export interface SyncStatus {
  /// 本机设备标识（首用生成并持久化，参与全序 tiebreak）
  device_id: string
  /// 通道是否已配置（WebDAV 凭据已保存）
  channel_configured: boolean
  /// 上次成功同步时刻（UTC ISO；从未同步为 null）
  last_sync_at: string | null
  /// 挂起 op 数量（不可重放、待用户裁决的操作）
  parked_count: number
  /// 本库是否为密文形态（密文库凭主口令封包；明文库明文上通道需显著提示）
  library_encrypted: boolean
}

/// 同步通道后端判别（缺省 `webdav`：判别字段落地晚于既有配置，#1217 兼容验收）。
export type ChannelBackend = 'webdav' | 's3'

/// 通道配置回显（未配置时各字段为空串/假值、configured 为 false）。
/// 两组地址字段并存：`base_url`/`username`/`password` 归 WebDAV（随 #1221 退役），
/// `endpoint` 起归 S3；消费哪组由 `backend` 判别。
export interface SyncChannelConfig {
  backend: ChannelBackend
  base_url: string
  username: string
  password: string
  /// 同步空间（跨端共识的世界身份，`book-<space>` 目录）
  space_id: string
  /// S3 兼容端点（MVP 只接受 https）
  endpoint: string
  /// S3 签名区域
  region: string
  /// S3 桶名
  bucket: string
  /// S3 对象键前缀（空串 = 桶根）
  prefix: string
  /// S3 Access Key ID（公开标识，非密钥）
  access_key: string
  /// S3 Secret Access Key（后端与主口令同级脱敏，不落日志）
  secret_key: string
  /// 寻址方式：true = path-style
  path_style: boolean
  configured: boolean
}

/// 通道配置写入参数（表单提交形态）。
/// 新字段在 Rust 侧带 serde 缺省：只发 WebDAV 组字段的旧调用方照常被接受
/// （缺 backend 回 webdav）；`space_id` 缺省回 default。
export interface SyncChannelConfigInput {
  backend?: ChannelBackend
  base_url?: string
  username?: string
  password?: string
  space_id?: string
  endpoint?: string
  region?: string
  bucket?: string
  prefix?: string
  access_key?: string
  secret_key?: string
  path_style?: boolean
}

/// 一次同步轮次的报告（后端 SyncRoundReport）：发布/拉取/重放的逐项计数。
export interface SyncRoundReport {
  uploaded_segments: number
  uploaded_ops: number
  downloaded_segments: number
  applied: number
  deduped: number
  superseded: number
  skipped: number
  parked: number
  /// 明文模式标记（界面显著提示依据）
  plaintext_mode: boolean
}

/// 挂起操作（issue #863 挂起通知数据面）：不可重放 op 的身份与码化原因。
/// 挂起通知可见是 #863 验收项——数量经 SyncStatus.parked_count 回显，
/// 明细经 get_parked_ops 按需拉取。
export interface ParkedOpInfo {
  /// op 标识（信封不可读时为合成 id）
  op_id: string
  /// 来源设备标识（信封不可读时为空串）
  device_id: string
  /// 实体判别键（载荷不可解时为空串）
  entity: string
  /// 实体 id（不可知时为空串）
  entity_id: string
  /// 码化挂起原因（前端按 errors.<code> 模板本地化）
  code: string
  /// 码化挂起原因的插值参数（按消息中动态值出现顺序；ADR-0050 / issue #957）
  params: string[]
  /// 挂起原因详情（已渲染完整句；码未命中模板或 params 不足时降级透传）
  message: string
  /// 挂起时刻（本机簿记事实）
  parked_at: string
}

/// 通道上的检查点指针（预检回显，issue #864；不含快照体）。
export interface SyncCheckpointInfo {
  /// 检查点代数（每次发布单调递增）
  generation: number
  /// 密文字节数（快照体大小）
  size: number
  /// 产出时刻（产出端本地事实，供展示）
  created_at: string
}

/// 检查点发布结果（issue #864）。
export interface SyncCheckpointPublished {
  /// 本次发布的代数
  generation: number
  /// 密文字节数
  size: number
  /// 明文模式标记（快照明文上通道，界面显著提示依据）
  plaintext_mode: boolean
}

/// 新端从通道检查点引导的结果（issue #864）。
export interface SyncBootstrapOutcome {
  /// 采纳的检查点代数
  generation: number
  /// 快照密文字节数
  size: number
  /// 引导后本库已从明文转换为本机密文库（重启后需凭主口令解锁）
  reencrypted: boolean
}
