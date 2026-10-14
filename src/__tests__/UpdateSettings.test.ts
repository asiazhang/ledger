import { describe, it, expect, beforeEach, vi } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { captureListenHandlers } from "@ledger/test-support/listen-mock";
import { useAppStore } from "@/stores/app";
import UpdateSettings from "@/settings/UpdateSettings.vue";
import GeneralSettings from "@/settings/GeneralSettings.vue";
import { RELEASE_PAGE_URL, resetAppUpdateForTest, useAppUpdate } from "@/composables/useAppUpdate";

// 发布页引导走 opener 插件，测试以模块替身断言调用事实。
const openUrl = vi.fn().mockResolvedValue(undefined);
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn((url: string) => openUrl(url)) }));

/** 挂载前以指定形态完成探测（卡片分支随形态渲染）。 */
async function mountCard(surface: "updater" | "release-page" | null) {
  if (surface !== null) {
    wireInvokeSeam({ defaults: { get_update_surface: surface } });
  } else {
    wireInvokeSeam({
      overrides: { get_update_surface: () => new Promise(() => {}) }, // 悬挂 = 探测未完成
    });
  }
  const wrapper = mount(UpdateSettings);
  await flushPromises();
  return wrapper;
}

function button(wrapper: ReturnType<typeof mount>, testid: string) {
  return wrapper.find(`[data-testid="${testid}"]`);
}

beforeEach(() => {
  resetAppUpdateForTest();
  captureListenHandlers();
  openUrl.mockClear();
});

describe("UpdateSettings.vue — updater 形态（桌面三平台完整更新面）", () => {
  it("呈现当前版本号、手动检查入口与自动检查开关", async () => {
    const wrapper = await mountCard("updater");
    expect(wrapper.text()).toContain("应用更新");
    expect(wrapper.text()).toContain("当前版本：");
    expect(button(wrapper, "update-check-btn").exists()).toBe(true);
    expect(button(wrapper, "update-auto-switch").exists()).toBe(true);
  });

  it("自动检查开关新装默认开启；切换即持久化并推送后端镜像（双断言）", async () => {
    const wrapper = await mountCard("updater");
    const sw = button(wrapper, "update-auto-switch");
    expect(sw.classes()).toContain("n-switch--active"); // 新装默认开启（ADR-0124 决策 8）

    await sw.trigger("click");
    await flushPromises();

    // 渲染效果：真源持久化到 localStorage（开关→后端镜像的推送归设备偏好推送
    // 宿主，见 useDevicePreferenceSync.test.ts 的同形先例）
    expect(localStorage.getItem("auto_update_enabled")).toBe("false");
  });

  it("手动检查发现新版本：就地呈现版本号与发布说明（双断言）", async () => {
    wireInvokeSeam({
      defaults: {
        get_update_surface: "updater",
        check_for_updates: { status: "update", version: "2.0.0", notes: "全新升级" },
      },
    });
    const wrapper = mount(UpdateSettings);
    await flushPromises();

    await button(wrapper, "update-check-btn").trigger("click");
    await flushPromises();

    expect(mockInvoke.mock.calls.some(([c]) => c === "check_for_updates")).toBe(true);
    expect(wrapper.find('[data-testid="update-check-version"]')?.text()).toContain("2.0.0");
    expect(wrapper.text()).toContain("全新升级");
    expect(button(wrapper, "update-card-download-btn").exists()).toBe(true);
  });

  it("手动检查相同 / 更旧同为「已是最新」（无强制升级门槛）", async () => {
    const wrapper = await mountCard("updater");
    // 矩阵行：三态中非 update 两态同文案
    for (const status of ["same", "older"] as const) {
      wireInvokeSeam({
        defaults: {
          get_update_surface: "updater",
          check_for_updates: { status, version: null, notes: null },
        },
      });
      await button(wrapper, "update-check-btn").trigger("click");
      await flushPromises();
      expect(wrapper.find('[data-testid="update-check-latest"]').exists()).toBe(true);
    }
  });

  it("手动检查失败就地报错（码化错误本地化文本，不走 toast）", async () => {
    const wrapper = await mountCard("updater");
    wireInvokeSeam({
      defaults: { get_update_surface: "updater" },
      overrides: {
        check_for_updates: () => Promise.reject(new Error("更新源暂时不可用，请检查网络后重试")),
      },
    });
    await button(wrapper, "update-check-btn").trigger("click");
    await flushPromises();

    const error = wrapper.find('[data-testid="update-check-error"]');
    expect(error.exists()).toBe(true);
    expect(error.text()).toContain("更新源暂时不可用");
  });

  it("检查进行中按钮禁用（挡重复触发）", async () => {
    const wrapper = await mountCard("updater");
    wireInvokeSeam({
      defaults: { get_update_surface: "updater" },
      overrides: { check_for_updates: () => new Promise(() => {}) }, // 悬挂 = 检查进行中
    });
    await button(wrapper, "update-check-btn").trigger("click");
    await flushPromises();
    expect(button(wrapper, "update-check-btn").attributes("disabled")).toBeDefined();
  });

  it("手动检查不受「已提醒版本」记忆限制（同版本仍就地呈现）", async () => {
    // 启动提醒已弹过并记录的版本，手动检查仍如实呈现（AC：记忆只约束自动弹层）
    useAppStore().markUpdateReminded("2.0.0");
    wireInvokeSeam({
      defaults: {
        get_update_surface: "updater",
        check_for_updates: { status: "update", version: "2.0.0", notes: "记忆外的再确认" },
      },
    });
    const wrapper = mount(UpdateSettings);
    await flushPromises();
    await button(wrapper, "update-check-btn").trigger("click");
    await flushPromises();
    expect(wrapper.find('[data-testid="update-check-version"]')?.text()).toContain("2.0.0");
  });

  it("发现新版本后点卡片下载按钮 → 进入下载编排（download_update 被调用）", async () => {
    wireInvokeSeam({
      defaults: {
        get_update_surface: "updater",
        check_for_updates: { status: "update", version: "2.0.0", notes: null },
      },
      overrides: {
        download_update: () => new Promise(() => {}), // 悬挂 = 下载进行中
      },
    });
    const wrapper = mount(UpdateSettings);
    await flushPromises();
    await button(wrapper, "update-check-btn").trigger("click");
    await flushPromises();
    await button(wrapper, "update-card-download-btn").trigger("click");
    await flushPromises();

    // 调用事实：download_update 已发起；渲染效果：流程进入下载中、提醒弹层打开承接进度
    expect(mockInvoke.mock.calls.some(([c]) => c === "download_update")).toBe(true);
    expect(useAppUpdate().stage.value).toBe("downloading");
    expect(useAppUpdate().modalOpen.value).toBe(true);
  });
});

describe("UpdateSettings.vue — release-page 形态（deb 与 Android 引导分支）", () => {
  it("只见打开发布页引导：无检查入口、无自动检查开关、无下载/安装动作", async () => {
    const wrapper = await mountCard("release-page");
    expect(wrapper.text()).toContain("当前安装渠道不支持应用内更新");
    expect(button(wrapper, "update-open-release-page").exists()).toBe(true);
    expect(button(wrapper, "update-check-btn").exists()).toBe(false);
    expect(button(wrapper, "update-auto-switch").exists()).toBe(false);
    expect(button(wrapper, "update-card-download-btn").exists()).toBe(false);
  });

  it("点「打开发布页」经 opener 打开 GitHub Releases 最新页（双断言）", async () => {
    const wrapper = await mountCard("release-page");
    await button(wrapper, "update-open-release-page").trigger("click");
    await flushPromises();
    expect(openUrl).toHaveBeenCalledWith(RELEASE_PAGE_URL);
  });
});

describe("UpdateSettings.vue — 形态探测未完成（null）", () => {
  it("两分支都不渲染（不误显引导或更新面）", async () => {
    const wrapper = await mountCard(null);
    expect(button(wrapper, "update-check-btn").exists()).toBe(false);
    expect(button(wrapper, "update-open-release-page").exists()).toBe(false);
  });
});

describe("GeneralSettings — 更新卡片收纳接线（「通用」Tab）", () => {
  it("通用 Tab 渲染更新卡片（删除收纳接线即红）", async () => {
    wireInvokeSeam({ defaults: { get_update_surface: "updater" } });
    const wrapper = mount(GeneralSettings);
    await flushPromises();
    expect(wrapper.text()).toContain("应用更新");
  });
});
