import type { EncryptionStatus, RememberPassphraseSupport, UnlockOutcome } from "@ledger/types";
import { str } from "./state";

/**
 * 加密域与记住主口令替身（issue #1966 / spec #1964 / ADR-0143 决策 1）：
 * 环境壳不模拟真实加密行为——mock 库恒已解锁明文态，全部命令答常量态。
 *
 * - `get_encryption_status` 恒答「明文库未锁定」：启动门答 ready，解锁屏
 *   不会出现，设置页加密卡片展示明文态；
 * - 解锁与转换三形态（enable / disable / change）及两类重置（忘记口令 /
 *   启动失败）答 void——不翻转任何状态，mock 库始终明文；
 * - 记住主口令按**不支持平台形态**应答（真源 passphrase_cache 非 macOS 桩）：
 *   能力查询答不支持（前端据此隐藏选项、回退手输），存储报码化错误，
 *   清除幂等成功，凭缓存解锁报无缓存（前端回退手输）。
 *
 * 码化错误以真后端 AppError 序列化形状抛出（kind/message/code），供前端
 * 按码本地化；这些拒绝路径在 mock 模式经 UI 不可达（库未锁定 + 不支持），
 * 仅钉契约形状。
 */

/** mock 库加密状态的常量：明文库、未锁定（唯一的真值）。 */
const PLAINTEXT_UNLOCKED: EncryptionStatus = { locked: false, file_encrypted: false };

/** 码化错误载荷（真后端 AppError 序列化形状：kind/message/code）。 */
function codedError(
  code: string,
  message: string,
): { kind: "Invalid"; code: string; message: string } {
  return { kind: "Invalid", code, message };
}

export function getEncryptionStatus(): EncryptionStatus {
  return { ...PLAINTEXT_UNLOCKED };
}

export function unlockEncryption(args: Record<string, unknown>): UnlockOutcome {
  str(args, "passphrase"); // 参数窄化：缺参报错，与真后端必需参数同语义
  return { relocated: false }; // 明文库无搬迁，恒答未补做
}

export function enableEncryption(args: Record<string, unknown>): void {
  str(args, "passphrase");
}

export function disableEncryption(args: Record<string, unknown>): void {
  str(args, "passphrase");
}

export function changeEncryptionPassphrase(args: Record<string, unknown>): void {
  str(args, "passphrase");
  str(args, "newPassphrase");
}

export function resetAfterForgottenPassphrase(): void {
  // 真语义是重置为全新明文空库并回到明文模式——mock 库本就是明文态，直接 void
}

export function resetAfterStartupFailure(): void {
  // 失败恢复屏的重置通道；mock 启动门恒 ready，此屏不可达，保形状直答
}

// —— 记住主口令：不支持平台形态（真源 infra passphrase_cache 非 macOS 桩） ——

export function getRememberPassphraseSupport(): RememberPassphraseSupport {
  // mode 为占位：supported=false 时前端只读 supported 隐藏全部选项
  return { supported: false, mode: "biometry" };
}

export function setRememberPassphrase(args: Record<string, unknown>): void {
  str(args, "passphrase");
  throw codedError("encryption.remember-unsupported", "当前平台不支持本机记住主口令");
}

export function clearRememberPassphrase(): void {
  // 不支持平台的删除桩幂等成功
}

export function unlockWithRememberedPassphrase(): Promise<UnlockOutcome> {
  // 不支持平台读取桩视为无缓存 → 前端回退手输的码化拒绝
  return Promise.reject(
    codedError("encryption.remember-no-cache", "本机没有缓存的主口令，请手动输入"),
  );
}
