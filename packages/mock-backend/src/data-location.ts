import type { DataLocationChangeOutcome, DataLocationInfo } from "@ledger/types";
import { bool, str } from "./state";

/**
 * 数据位置域替身（issue #1966 / spec #1964 / ADR-0143 决策 1）：环境壳不模拟
 * 指针文件与启动引导搬迁——信息恒答「默认目录、无待重启、无回退」（与账本
 * 注册表的单账本默认目录常量同源）；更改意图与恢复默认答「已落盘」常量回执
 * （目标目录回显、无需二选一），真实搬迁发生在下次启动引导，mock 不模拟。
 */

/** mock 默认数据目录（账本注册表单账本目录的父目录，占位路径）。 */
const MOCK_DATA_DIR = "/mock/data";

export function getDataLocationInfo(): DataLocationInfo {
  return {
    active_dir: MOCK_DATA_DIR,
    configured_dir: null, // 未配置自定义位置：默认目录即生效目录
    pending_restart: false,
    fallback_reason: null,
  };
}

export function submitDataLocationChange(args: Record<string, unknown>): DataLocationChangeOutcome {
  const targetDir = str(args, "targetDir");
  bool(args, "adoptExisting"); // mock 无文件系统，不存在「目标已有同名库」的二选一
  return { requires_choice: false, committed: true, target_dir: targetDir };
}

export function restoreDefaultDataLocation(
  args: Record<string, unknown>,
): DataLocationChangeOutcome {
  bool(args, "adoptExisting");
  return { requires_choice: false, committed: true, target_dir: MOCK_DATA_DIR };
}
