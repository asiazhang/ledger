import type { AppBuildInfo, BaseCurrencyState, LogLevelState } from "@ledger/types";
import { db, reqBool, str } from "./state";

/**
 * 启动链路与设置域替身（issue #1965 / spec #1964 / ADR-0143）：启动即调用的
 * 开关推送 no-op 应答 void；本位币 / 日志等级为账本级设置的内存态（读投影
 * `state` 的设置字典、写改内存、重装重播种）；构建信息与 AI 提示答字面量；
 * 壳命令（日志目录 / 前端错误回传 / 文件定位 / 重启）no-op——环境壳类命令
 * 不模拟真实环境行为，保设置页「通用」「关于」可导航不报错。
 *
 * 设置写入不发 `ledger:changed`：该信号是参考数据 store 的重拉触发源
 * （ADR-0012），设置域命令真后端亦不发，前端直接消费命令返回值。
 */

// —— 启动链路开关推送（issue #307 / ADR-0042、#1429 / ADR-0124）：真源在
// 前端 localStorage 设备偏好，替身只收推送不作运行时镜像 ——

export function setAutoExecutionEnabled(args: Record<string, unknown>): void {
  reqBool(args, "enabled"); // 参数窄化：缺参报错，与真后端必需参数同语义
}

export function setAutoUpdateEnabled(args: Record<string, unknown>): void {
  reqBool(args, "enabled");
}

// —— 本位币基准（issue #858，设置页「通用」Tab）：内存态，缺省 CNY ——

export function getBaseCurrency(): BaseCurrencyState {
  return { code: db().settings.base_currency };
}

export function setBaseCurrency(args: Record<string, unknown>): BaseCurrencyState {
  const code = str(args, "code");
  // 存在性校验（spec #1964 取向）：真后端经币种字典校验，替身对夹具字典查存在
  if (!db().currencies.some((c) => c.code === code)) {
    throw new Error(`替身后端：未知币种 ${code}`);
  }
  db().settings.base_currency = code;
  return { code };
}

// —— 日志等级（spec #611，「关于」Tab）：内存态，缺省 info ——

export function getLogLevel(): LogLevelState {
  return { level: db().settings.log_level };
}

export function setLogLevel(args: Record<string, unknown>): void {
  db().settings.log_level = str(args, "level");
}

// —— 构建信息（issue #1957）与 AI 提示词：字面量应答 ——

/** 替身版本字面量（手写对齐根 package.json，当前 0.10.0）：「关于」显示值与
 *  更新回执（app-update.ts）共用；发版漂移无害（ADR-0143「代价」节：夹具与
 *  字面量漂移由消费报错暴露）。 */
export const MOCK_APP_VERSION = "0.10.0";

/** 构建信息字面量应答：平台/架构保留 `std::env::consts` 原值形态（显示名映射归前端）。 */
export function appBuildInfo(): AppBuildInfo {
  return { version: MOCK_APP_VERSION, platform: "macos", arch: "aarch64" };
}

/** 真源是 src-tauri/prompts/ledger-api.md（AI 记账入口提示词）；替身答短字面量钉形状。 */
export function getAiPrompt(): string {
  return (
    "# Ledger API 入口提示词（浏览器开发模式替身）\n\n" +
    "替身后端不提供本地 HTTP API，AI 记账与数据迁移链路在 mock 模式不可用；" +
    "本字面量仅钉 get_ai_prompt 命令形状。"
  );
}

// —— 壳命令：no-op 应答（环境壳不模拟，spec #1964） ——

export function openLogDir(): void {
  // 无系统文件管理器可开；设置页「日志」入口可点不报错
}

export function logFrontendError(args: Record<string, unknown>): void {
  str(args, "message"); // 真后端落 tracing ERROR；替身即发即忘，不落日志
}

export function revealInFileManager(args: Record<string, unknown>): void {
  str(args, "path"); // 备份列表行「定位」入口可点不报错
}

export function restartApp(): void {
  // 不模拟进程重启（真语义是退出后由壳重启；mock 模式刷新页面即重播种）
}
