// 加密模式（数据文件属性）领域类型（issue #570 / ADR-0075）。
// 字段命名与 Rust 侧 serde 默认（snake_case）保持一致。

/** 加密状态（设置页加密卡片与启动解锁屏消费）。 */
export interface EncryptionStatus {
  /** 进程是否处于锁定（等待解锁）状态：密文库已探测、业务读写不可用。 */
  locked: boolean
  /** 库文件当前是否为密文库（文件即真相）。 */
  file_encrypted: boolean
}

/** 解锁结果：relocated 表示解锁后补做了等待中的搬迁（前端据此触发重启）。 */
export interface UnlockOutcome {
  relocated: boolean
}
