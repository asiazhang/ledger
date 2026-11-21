import { ref } from "vue";
import { api } from "@ledger/api";
import type { AppBuildInfo } from "@ledger/types";

const PLATFORM_LABELS: Record<string, string> = {
  macos: "macOS",
  windows: "Windows",
  linux: "Linux",
  android: "Android",
  ios: "iOS",
  freebsd: "FreeBSD",
};

const ARCH_LABELS: Record<string, string> = {
  aarch64: "arm64",
  x86_64: "x64",
  arm: "arm",
  x86: "x86",
};

export function formatPlatform(info: AppBuildInfo): string {
  const platform = PLATFORM_LABELS[info.platform] ?? info.platform;
  const arch = ARCH_LABELS[info.arch] ?? info.arch;
  return `${platform} · ${arch}`;
}

const buildInfo = ref<AppBuildInfo | null>(null);
let pending: Promise<void> | null = null;

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

export function resetAppBuildInfoForTest(): void {
  buildInfo.value = null;
  pending = null;
}
