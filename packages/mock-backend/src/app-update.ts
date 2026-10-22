import type { UpdateCheckOutcome, UpdateDownloadOutcome, UpdateSurface } from "@ledger/types";

/**
 * 应用更新域替身（issue #1966 / spec #1964 / ADR-0143 决策 1）：环境壳不模拟
 * 更新源与更新编排——手动检查恒答「已是最新」（same，不携版本号与说明），
 * 更新入口形态答桌面完整更新面（updater），下载答占位版本回执（与
 * app_build_info 同一版本字面量），安装 void；更新前备份、下载进度事件
 * （ledger:update-download-progress）与进程重启都不模拟。
 */

/** 与 settings.ts appBuildInfo 同源的手写版本字面量（发版漂移无害，ADR-0143「代价」）。 */
const MOCK_APP_VERSION = "0.10.0";

export function checkForUpdates(): UpdateCheckOutcome {
  return { status: "same", version: null, notes: null };
}

export function getUpdateSurface(): UpdateSurface {
  return "updater";
}

export function downloadUpdate(): UpdateDownloadOutcome {
  return { version: MOCK_APP_VERSION };
}

export function installUpdate(): void {
  // 真语义是安装已下载工件并自动重启；mock 不模拟安装，直接 void
}
