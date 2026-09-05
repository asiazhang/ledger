// 启动状态（issue #601 / ADR-0075 决策 5 修订）：前端启动首屏选择的唯一依据。
// 字段命名与 Rust 侧 serde 默认（snake_case）保持一致。

/** 启动相位（闭集，与后端 `BootStatus.phase` 同值域）。 */
export type BootPhase = 'ready' | 'locked' | 'failed'

/** 启动状态：`ready` 挂主界面、`locked` 挂解锁屏、`failed` 挂启动失败恢复屏。 */
export interface BootStatus {
  phase: BootPhase
  /** 失败时的稳定错误码（按码本地化失败恢复屏文案）；非 failed 为 null。 */
  error_code: string | null
}
