// 应用更新域 IPC 载荷类型（issue #1429 / ADR-0124 决策 4/8）：检查路径命令面。

/**
 * 版本比较三态（`check_for_updates` 返回）。
 * - `update`：更新源版本高于当前版本（携版本号与发布说明）；
 * - `same`：与当前版本相同；
 * - `older`：更新源版本低于当前版本。
 */
export type UpdateCheckStatus = "update" | "same" | "older";

/** 更新检查结果：发现新版本时携版本号与发布说明（兼容性警告随发布说明承载）；相同/更旧不携。 */
export interface UpdateCheckOutcome {
  status: UpdateCheckStatus;
  version: string | null;
  notes: string | null;
}

/**
 * 启动延迟静默检查发现新版本时的前端通知事件载荷
 * （事件名 `ledger:update-available`；「已提醒版本」去重记忆归前端）。
 */
export interface UpdateAvailablePayload {
  version: string;
  notes: string | null;
}
