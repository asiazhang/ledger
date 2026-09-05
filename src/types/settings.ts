// 设置域 IPC 载荷类型（spec #611，About 页「关于」Tab 日志等级）。

/**
 * 日志等级当前持久化档位（`get_log_level` 回显）。
 * 只反映**持久化档位**；显式 RUST_LOG 环境变量在本次启动内优先且不写库，
 * 界面展示值与本次实际生效档位可能不一致（由「关于」页静态提示说明）。
 */
export interface LogLevelState {
  /** 闭集五档指令字符串之一：error / warn / info / debug / trace。 */
  level: string
}
