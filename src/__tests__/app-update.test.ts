import { describe, it, expect, beforeEach, vi } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { captureListenHandlers, type CapturedListener } from "@ledger/test-support/listen-mock";
import { useAppStore } from "@/stores/app";
import UpdateReminderHost from "@/components/UpdateReminderHost.vue";
import {
  UPDATE_AVAILABLE_EVENT,
  UPDATE_DOWNLOAD_PROGRESS_EVENT,
  RELEASE_PAGE_URL,
  resetAppUpdateForTest,
  useAppUpdate,
} from "@/composables/useAppUpdate";

// 发布页引导走 opener 插件（系统默认浏览器），测试以模块替身断言调用事实。
const openUrl = vi.fn().mockResolvedValue(undefined);
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn((url: string) => openUrl(url)) }));

let handlers: CapturedListener[] = [];

/** 触发事件监听器（多监听器全部触发；各 handler 自带事件/载荷形状守卫，互不误收）。 */
function fireEvent(name: string, payload: unknown): void {
  expect(handlers.length, "事件监听器应已在位").toBeGreaterThan(0);
  for (const handler of handlers) handler({ event: name, payload });
}

function bodyButton(testid: string): HTMLButtonElement {
  const el = document.body.querySelector(`[data-testid="${testid}"]`);
  expect(el, `body 内应存在 ${testid}`).toBeTruthy();
  return el as HTMLButtonElement;
}

/** 挂载宿主并以 updater 形态完成探测（事件订阅随之在位）。 */
async function mountUpdaterSurface() {
  wireInvokeSeam({ defaults: { get_update_surface: "updater" } });
  const wrapper = mount(UpdateReminderHost);
  await flushPromises();
  return wrapper;
}

beforeEach(() => {
  // 模块级单例跨挂载存活：每测复位（状态 + 订阅登记），先例 resetFxSyncForTest。
  resetAppUpdateForTest();
  handlers = captureListenHandlers();
  openUrl.mockClear();
});

describe("应用更新编排 — 启动提醒与「已提醒版本」记忆（issue #1431）", () => {
  it("启动事件 → 提醒弹层呈现版本号与发布说明，并记录已提醒版本", async () => {
    await mountUpdaterSurface();

    fireEvent(UPDATE_AVAILABLE_EVENT, { version: "1.2.4", notes: "修复了若干问题\n⚠️ 需要迁移" });
    await flushPromises();

    // 渲染效果：弹层可见（teleport 到 body），版本号 + 发布说明在场
    expect(document.body.textContent).toContain("发现新版本");
    expect(
      document.body.querySelector('[data-testid="update-offer-version"]')?.textContent,
    ).toContain("1.2.4");
    expect(document.body.textContent).toContain("修复了若干问题");
    // 记忆效果：弹层展示即记录版本（每版本至多自动提醒一次）
    expect(useAppStore().updateRemindedVersion).toBe("1.2.4");
  });

  it("同一版本再次启动事件不再弹层（每版本至多自动提醒一次）", async () => {
    await mountUpdaterSurface();
    fireEvent(UPDATE_AVAILABLE_EVENT, { version: "1.2.4", notes: null });
    await flushPromises();
    expect(document.body.textContent).toContain("发现新版本");

    fireEvent(UPDATE_AVAILABLE_EVENT, { version: "1.2.4", notes: null });
    await flushPromises();
    // 弹层状态不变（无关闭副作用、无重复记录）——同版本不再重新弹层
    expect(useAppStore().updateRemindedVersion).toBe("1.2.4");
    expect(document.body.querySelector('[data-testid="update-offer-version"]')).toBeTruthy();
  });

  it("新版本号到达时弹层随新版本刷新（latest-wins）", async () => {
    await mountUpdaterSurface();
    fireEvent(UPDATE_AVAILABLE_EVENT, { version: "1.2.4", notes: null });
    await flushPromises();
    fireEvent(UPDATE_AVAILABLE_EVENT, { version: "1.3.0", notes: null });
    await flushPromises();
    expect(
      document.body.querySelector('[data-testid="update-offer-version"]')?.textContent,
    ).toContain("1.3.0");
  });

  it("release-page 形态（deb 与 Android）忽略启动提醒事件", async () => {
    wireInvokeSeam({ defaults: { get_update_surface: "release-page" } });
    mount(UpdateReminderHost);
    await flushPromises();

    fireEvent(UPDATE_AVAILABLE_EVENT, { version: "1.2.4", notes: null });
    await flushPromises();

    expect(document.body.textContent).not.toContain("发现新版本");
    expect(useAppStore().updateRemindedVersion).toBeNull();
  });

  it("关闭提醒弹层不触发下载（全询问式：取消即取消）", async () => {
    await mountUpdaterSurface();
    fireEvent(UPDATE_AVAILABLE_EVENT, { version: "1.2.4", notes: null });
    await flushPromises();

    bodyButton("update-download-btn"); // 下载按钮在场（确认入口）
    // 点「以后再说」关闭
    const buttons = [...document.body.querySelectorAll("button")];
    const later = buttons.find((b) => b.textContent?.includes("以后再说"));
    expect(later).toBeTruthy();
    later!.click();
    await flushPromises();

    // 弹层已关（jsdom 下 NModal 离场后 DOM 残留，可见性以编排状态为准），零下载副作用
    expect(useAppUpdate().modalOpen.value).toBe(false);
    expect(useAppUpdate().stage.value).toBe("idle");
    expect(mockInvoke.mock.calls.some(([c]) => c === "download_update")).toBe(false);
  });
});

describe("应用更新编排 — 下载与安装（全询问式，issue #1430 接缝）", () => {
  it("确认下载：调用 download_update，进度事件驱动弹层进度，完成后进入待安装", async () => {
    let resolveDownload!: (v: unknown) => void;
    await mountUpdaterSurface();
    // 覆盖 download_update 为手工收尾的替身：先断言进行中进度，再落终态。
    wireInvokeSeam({
      defaults: { get_update_surface: "updater" },
      overrides: {
        download_update: () =>
          new Promise((resolve) => {
            resolveDownload = resolve;
          }),
      },
    });

    fireEvent(UPDATE_AVAILABLE_EVENT, { version: "1.2.4", notes: null });
    await flushPromises();
    bodyButton("update-download-btn").click();
    await flushPromises();

    // 调用事实：下载已发起（备份时机归后端安装编排，前端只见单命令）
    expect(mockInvoke.mock.calls.some(([c]) => c === "download_update")).toBe(true);
    // 渲染效果：进度阶段 + 总大小已知时呈现百分比
    fireEvent(UPDATE_DOWNLOAD_PROGRESS_EVENT, { received: 500, total: 1000 });
    await flushPromises();
    expect(document.body.textContent).toContain("正在下载更新");
    expect(document.body.textContent).toContain("50");

    resolveDownload({ version: "1.2.4" });
    await flushPromises();
    // 终态：待安装——版本号在场，安装按钮（warning 级确认）在场
    expect(document.body.textContent).toContain("更新已就绪");
    expect(
      document.body.querySelector('[data-testid="update-downloaded-version"]')?.textContent,
    ).toContain("1.2.4");
    expect(document.body.textContent).toContain("安装将退出当前应用");
    expect(document.body.querySelector('[data-testid="update-install-btn"]')).toBeTruthy();
  });

  it("总大小未知时进度转不确定态，不呈现百分比", async () => {
    await mountUpdaterSurface();
    fireEvent(UPDATE_AVAILABLE_EVENT, { version: "1.2.4", notes: null });
    await flushPromises();
    wireInvokeSeam({
      overrides: {
        download_update: () => new Promise(() => {}), // 悬挂 = 下载进行中
      },
    });
    bodyButton("update-download-btn").click();
    await flushPromises();
    fireEvent(UPDATE_DOWNLOAD_PROGRESS_EVENT, { received: 100, total: null });
    await flushPromises();
    expect(document.body.textContent).toContain("正在下载更新");
    expect(document.body.textContent).not.toContain("%");
  });

  it("下载失败回提醒阶段就地报错，可重试", async () => {
    await mountUpdaterSurface();
    let attempts = 0;
    wireInvokeSeam({
      overrides: {
        download_update: () => {
          attempts += 1;
          return attempts === 1
            ? Promise.reject(new Error("更新下载失败，请检查网络后重试"))
            : Promise.resolve({ version: "1.2.4" });
        },
      },
    });
    fireEvent(UPDATE_AVAILABLE_EVENT, { version: "1.2.4", notes: null });
    await flushPromises();

    bodyButton("update-download-btn").click();
    await flushPromises();
    // 就地报错：错误文本在弹层内，阶段回提醒（版本与发布说明保留可重试）
    expect(document.body.querySelector('[data-testid="update-flow-error"]')?.textContent).toContain(
      "更新下载失败",
    );
    expect(document.body.querySelector('[data-testid="update-download-btn"]')).toBeTruthy();

    bodyButton("update-download-btn").click();
    await flushPromises();
    expect(attempts).toBe(2);
    expect(document.body.textContent).toContain("更新已就绪");
  });

  it("确认安装：调用 install_update（成功路径不返回，进程随安装重启）", async () => {
    await mountUpdaterSurface();
    wireInvokeSeam({
      overrides: {
        download_update: { version: "1.2.4" },
        install_update: Promise.resolve(undefined),
      },
    });
    fireEvent(UPDATE_AVAILABLE_EVENT, { version: "1.2.4", notes: null });
    await flushPromises();
    bodyButton("update-download-btn").click();
    await flushPromises();

    bodyButton("update-install-btn").click();
    await flushPromises();

    expect(mockInvoke.mock.calls.some(([c]) => c === "install_update")).toBe(true);
  });

  it("安装失败就地报错，弹层保持待安装现场", async () => {
    await mountUpdaterSurface();
    wireInvokeSeam({
      overrides: {
        download_update: { version: "1.2.4" },
        install_update: () => Promise.reject(new Error("更新安装失败，请重试")),
      },
    });
    fireEvent(UPDATE_AVAILABLE_EVENT, { version: "1.2.4", notes: null });
    await flushPromises();
    bodyButton("update-download-btn").click();
    await flushPromises();

    bodyButton("update-install-btn").click();
    await flushPromises();

    expect(document.body.querySelector('[data-testid="update-flow-error"]')?.textContent).toContain(
      "更新安装失败",
    );
    expect(document.body.querySelector('[data-testid="update-install-btn"]')).toBeTruthy();
  });

  it("安装在途 submitting：安装按钮禁用消化备份耗时；兜底文案对准安装前时机", async () => {
    await mountUpdaterSurface();
    let resolveInstall!: (v: unknown) => void;
    wireInvokeSeam({
      overrides: {
        download_update: { version: "1.2.4" },
        install_update: () =>
          new Promise((resolve) => {
            resolveInstall = resolve;
          }),
      },
    });
    fireEvent(UPDATE_AVAILABLE_EVENT, { version: "1.2.4", notes: null });
    await flushPromises();
    bodyButton("update-download-btn").click();
    await flushPromises();

    // 兜底文案对准新时机（#1944）：备份在安装确认后创建，不再写「下载前已自动创建」。
    expect(document.body.textContent).toContain("已自动创建更新前备份");
    expect(document.body.textContent).not.toContain("下载前已自动创建");

    bodyButton("update-install-btn").click();
    await flushPromises();

    // 在途 submitting：install_update 在途期间安装按钮禁用（备份+安装在途，
    // 消化耗时并防重复触发）。
    expect(bodyButton("update-install-btn").disabled).toBe(true);

    resolveInstall(undefined);
    await flushPromises();
    expect(bodyButton("update-install-btn").disabled).toBe(false);
  });

  it("下载中收起弹层：下载在后端继续，进度事件照常接受；卡片状态行可重开弹层", async () => {
    await mountUpdaterSurface();
    wireInvokeSeam({
      overrides: { download_update: () => new Promise(() => {}) }, // 悬挂 = 进行中
    });
    fireEvent(UPDATE_AVAILABLE_EVENT, { version: "1.2.4", notes: null });
    await flushPromises();
    bodyButton("update-download-btn").click();
    await flushPromises();

    // 收起弹层（关闭 ≠ 取消：下载继续）
    bodyButton("update-hide-btn").click();
    await flushPromises();
    // 弹层已收起（jsdom 下 NModal 离场后 DOM 残留，可见性以编排状态为准）
    expect(useAppUpdate().modalOpen.value).toBe(false);
    expect(useAppUpdate().stage.value).toBe("downloading");

    // 后台进度继续被接受（迟到事件不复活：此处阶段仍在下载中，应更新）
    fireEvent(UPDATE_DOWNLOAD_PROGRESS_EVENT, { received: 750, total: 1000 });
    await flushPromises();

    // 宿主卡片同消费模块状态：重新挂载一个卡片视角验证再入口——直接重开
    useAppUpdate().reopenFlow();
    await flushPromises();
    expect(document.body.textContent).toContain("正在下载更新");
    expect(document.body.textContent).toContain("75");
  });

  it("非下载阶段的进度事件被忽略（迟到事件不复活进度）", async () => {
    await mountUpdaterSurface();
    fireEvent(UPDATE_DOWNLOAD_PROGRESS_EVENT, { received: 999, total: 1000 });
    await flushPromises();
    expect(useAppUpdate().stage.value).toBe("idle");
    fireEvent(UPDATE_AVAILABLE_EVENT, { version: "1.2.4", notes: null });
    await flushPromises();
    fireEvent(UPDATE_DOWNLOAD_PROGRESS_EVENT, { received: 999, total: 1000 });
    await flushPromises();
    // 提醒阶段的进度事件不改变呈现（仍为提醒弹层，无进度文案）
    expect(document.body.textContent).toContain("发现新版本");
    expect(document.body.textContent).not.toContain("正在下载更新");
  });
});

describe("应用更新编排 — 形态探测与发布页引导", () => {
  it("init 探测更新入口形态一次", async () => {
    await mountUpdaterSurface();
    expect(useAppUpdate().surface.value).toBe("updater");
    expect(mockInvoke.mock.calls.filter(([c]) => c === "get_update_surface").length).toBe(1);
  });

  it("形态探测失败按 release-page 兜底（更新入口降级，不影响其余功能）", async () => {
    wireInvokeSeam({
      overrides: { get_update_surface: () => Promise.reject(new Error("命令缺失")) },
    });
    mount(UpdateReminderHost);
    await flushPromises();
    expect(useAppUpdate().surface.value).toBe("release-page");
  });

  it("打开发布页：经 opener 打开 GitHub Releases 最新页", async () => {
    await mountUpdaterSurface();
    await useAppUpdate().openReleasePage();
    expect(openUrl).toHaveBeenCalledWith(RELEASE_PAGE_URL);
  });
});
