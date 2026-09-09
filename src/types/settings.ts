// 设置域 IPC 载荷类型（spec #611 日志等级 / issue #858 本位币基准）。

/**
 * 日志等级当前持久化档位（`get_log_level` 回显）。
 * 只反映**持久化档位**；显式 RUST_LOG 环境变量在本次启动内优先且不写库，
 * 界面展示值与本次实际生效档位可能不一致（由「关于」页静态提示说明）。
 */
export interface LogLevelState {
  /** 闭集五档指令字符串之一：error / warn / info / debug / trace。 */
  level: string
}

/**
 * 本位币基准当前值（`get_base_currency` / `set_base_currency` 回显，issue #858）：
 * 账本级设置（LedgerLevelSetting 首个成员），随多端同步全设备一致。
 */
export interface BaseCurrencyState {
  /** 当前基准币种代码（缺 key 回默认 CNY）。 */
  code: string
}
