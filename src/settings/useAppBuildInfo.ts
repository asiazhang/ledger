import { ref } from "vue";
import { api } from "@ledger/api";
import type { AppBuildInfo } from "@ledger/types";

/**
 * 应用构建元信息的前端取数面（issue #1957 / ADR-0022 修订注记）：设置页「关于」
 * Tab 与更新卡片的当前版本号共用同一份数据。
 *
 * 为什么是模块级单例而非 Pinia store：一份只读、不写、不随账本/设备偏好迁移的
 * 编译期元信息，生命周期与「一次应用会话」同长（同 `useAppUpdate` 的单例取舍，
 * 临时任务态不符合设备偏好 store 的定位）。
 *
 * 取数失败不阻断页面：`buildInfo` 保持 null，消费方按「行不渲染」降级（Git
 * 版本行同形先例），并留 warn 供排查——这是本地命令，失败面只有壳层回归。
 */

/** 平台显示名：编译期常量原值 → 用户可读名（专有名，不随界面语言翻译）。 */
const PLATFORM_LABELS: Record<string, string> = {
  macos: "macOS",
  windows: "Windows",
  linux: "Linux",
  android: "Android",
  ios: "iOS",
  freebsd: "FreeBSD",
};

/** 架构显示名：`aarch64` → `arm64`（与安装包命名一致），其余原样透出。 */
const ARCH_LABELS: Record<string, string> = {
  aarch64: "arm64",
  x86_64: "x64",
  arm: "arm",
  x86: "x86",
};

/**
 * 平台行显示值（`macOS · arm64`）：未登记的取值原样透出，不静默吞——
 * 平台/架构是报障时的判据，显示错比显示陌生值更糟（本票修的就是这个缺陷）。
 */
export function formatPlatform(info: AppBuildInfo): string {
  const platform = PLATFORM_LABELS[info.platform] ?? info.platform;
  const arch = ARCH_LABELS[info.arch] ?? info.arch;
  return `${platform} · ${arch}`;
}

/** 模块级单例：一次取数、多卡片共享（两处消费方同屏时只发一次 IPC）。 */
const buildInfo = ref<AppBuildInfo | null>(null);
/** 在途请求（成功或失败后清空：失败允许后续挂载重试）。 */
let pending: Promise<void> | null = null;

/** 构建元信息（惰性取数）：首次消费时发起一次，之后复用同一份。 */
export function useAppBuildInfo(): { buildInfo: typeof buildInfo } {
  if (buildInfo.value === null && pending === null) {
    pending = api
      .getAppBuildInfo()
      .then((info) => {
        buildInfo.value = info;
      })
      .catch((e) => {
        console.warn("读取构建元信息失败", e);
      })
      .finally(() => {
        pending = null;
      });
  }
  return { buildInfo };
}

/** 测试用复位（模块级单例的用例间隔离，先例 `resetAppUpdateForTest`）。 */
export function resetAppBuildInfoForTest(): void {
  buildInfo.value = null;
  pending = null;
}
