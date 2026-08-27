// DataLocation（数据存储位置）领域类型（issue #133 / ADR-0018）。
// 字段命名与 Rust 侧 serde 默认（snake_case）保持一致。

/** DataLocation 当前信息（设置页展示用）。 */
export interface DataLocationInfo {
  /** 当前生效的库文件目录（完整路径）。 */
  active_dir: string
  /** 指针文件记录的意图目录；null = 未配置（损坏时的警示见 fallback_reason）。 */
  configured_dir: string | null
  /** 已更改待重启生效：意图目录 ≠ 当前生效目录。 */
  pending_restart: boolean
  /** 上次启动引导发生回退的原因（供界面显著提示）；null = 未回退。 */
  fallback_reason: string | null
}

/** 更改意图提交结果（更改位置与恢复默认共用）。 */
export interface DataLocationChangeOutcome {
  /** 目标已存在同名 ledger.db，需用户二选一（接管该库 / 取消换位）：
   *  前端确认后以 adopt_existing = true 二次提交即接管落盘；取消则不再提交。 */
  requires_choice: boolean
  /** 意图是否已落盘（校验通过并写入指针文件，下次启动生效）。 */
  committed: boolean
  /** 已落盘意图的目标目录（committed 时有值）。 */
  target_dir: string | null
}
