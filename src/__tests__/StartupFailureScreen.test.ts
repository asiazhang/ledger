import { describe, it, expect, vi, beforeEach } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";

import StartupFailureScreen from "@/backup/StartupFailureScreen.vue";
import { useEncryptionGate } from "@/backup/useEncryptionGate";
import { RELEASE_PAGE_URL } from "@/composables/useAppUpdate";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  save: vi.fn(),
  confirm: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
const restartAppShortly = vi.fn();
vi.mock("@/backup/restart", () => ({ restartAppShortly: () => restartAppShortly() }));
const mockOpen = vi.mocked(open);
const mockOpenUrl = vi.mocked(openUrl);

beforeEach(() => {
  mockOpen.mockReset();
  restartAppShortly.mockClear();
  const gate = useEncryptionGate();
  gate.locked.value = null;
  gate.bootFailed.value = false;
  gate.bootErrorCode.value = null;
});

function findButton(wrapper: ReturnType<typeof mount>, testid: string) {
  return wrapper.find(`[data-testid="${testid}"]`);
}

async function waitModal() {
  await flushPromises();
  await new Promise((resolve) => setTimeout(resolve, 50));
  await flushPromises();
}

async function mountFailedScreen(
  overrides: Record<string, (args?: Record<string, unknown>) => unknown> = {},
) {
  wireInvokeSeam({ overrides });
  const gate = useEncryptionGate();
  gate.bootFailed.value = true;
  const wrapper = mount(StartupFailureScreen, {
    global: { stubs: { teleport: true } },
  });
  await flushPromises();
  return { wrapper, gate };
}

function stubRestoreChannel(
  backupPath: string,
  overrides: Record<string, (args?: Record<string, unknown>) => unknown> = {},
) {
  mockOpen.mockResolvedValue(backupPath);
  wireInvokeSeam({
    overrides: {
      get_backup_meta: () => Promise.resolve({ kind: "manual", encrypted: false }),
      get_encryption_status: () => Promise.resolve({ locked: false, file_encrypted: false }),
      restore_backup: () =>
        Promise.resolve({ schema_version: 42, restored_at: "2026-09-06T00:00:00Z" }),
      ...overrides,
    },
  });
}

describe("StartupFailureScreen.vue（启动失败恢复屏·issue #601）", () => {
  it("失败态挂载恢复屏：标题、说明与「重置为空库」通道入口就位", async () => {
    const { wrapper } = await mountFailedScreen();
    const html = wrapper.html();
    expect(html).toContain("无法打开账本数据库");
    expect(html).toContain("重置为空库");
    expect(html).not.toContain("仪表盘");
  });

  it("重置需二次确认：先展示不可逆后果说明，确认才调用重置命令", async () => {
    const { wrapper, gate } = await mountFailedScreen({
      reset_after_startup_failure: () => Promise.resolve(),
    });

    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "reset_after_startup_failure")).toBe(
      false,
    );

    await findButton(wrapper, "failure-reset-open")!.trigger("click");
    await waitModal();
    const html = wrapper.html();
    expect(html).toContain("重置为空库？");
    expect(html).toContain("不可逆");
    expect(html).toContain("无法在本应用中访问");
    expect(html).toContain("ledger.db.bak");

    await findButton(wrapper, "failure-reset-confirm")!.trigger("click");
    await flushPromises();
    expect(mockInvoke).toHaveBeenCalledWith("reset_after_startup_failure");
    expect(gate.bootFailed.value).toBe(false);
    expect(gate.locked.value).toBe(false);
  });

  it("二次确认取消：留在失败恢复屏，不发起重置", async () => {
    const { wrapper, gate } = await mountFailedScreen({
      reset_after_startup_failure: () => Promise.resolve(),
    });

    await findButton(wrapper, "failure-reset-open")!.trigger("click");
    await waitModal();
    await findButton(wrapper, "failure-reset-cancel")!.trigger("click");
    await flushPromises();
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "reset_after_startup_failure")).toBe(
      false,
    );
    expect(gate.bootFailed.value).toBe(true);
    expect(wrapper.html()).toContain("无法打开账本数据库");
  });

  it("重置失败：错误文案透传，保持失败态可重试", async () => {
    const { wrapper, gate } = await mountFailedScreen({
      reset_after_startup_failure: () =>
        Promise.reject({
          kind: "Invalid",
          message: "数据库文件不存在或为空",
          code: "encryption.db-missing",
        }),
    });

    await findButton(wrapper, "failure-reset-open")!.trigger("click");
    await waitModal();
    await findButton(wrapper, "failure-reset-confirm")!.trigger("click");
    await flushPromises();
    expect(wrapper.html()).toContain("数据库文件不存在或为空");
    expect(gate.bootFailed.value).toBe(true);
  });

  it("探测返回 failed：门状态翻转，失败恢复屏应由 App 挂载", async () => {
    wireInvokeSeam({
      overrides: {
        get_boot_status: () =>
          Promise.resolve({ phase: "failed", error_code: "boot.db-unreadable" }),
      },
    });
    const gate = useEncryptionGate();
    await gate.probe();
    await flushPromises();
    expect(gate.bootFailed.value).toBe(true);
    expect(gate.locked.value).toBe(null);
  });

  it("探测返回 ready：明文库正常进入主界面（明文日常启动零改动）", async () => {
    wireInvokeSeam({
      overrides: {
        get_boot_status: () => Promise.resolve({ phase: "ready", error_code: null }),
      },
    });
    const gate = useEncryptionGate();
    await gate.probe();
    await flushPromises();
    expect(gate.bootFailed.value).toBe(false);
    expect(gate.locked.value).toBe(false);
  });

  it("探测返回 locked：密文库进入解锁屏路径（#570 既有行为零改动）", async () => {
    wireInvokeSeam({
      overrides: {
        get_boot_status: () => Promise.resolve({ phase: "locked", error_code: null }),
      },
    });
    const gate = useEncryptionGate();
    await gate.probe();
    await flushPromises();
    expect(gate.bootFailed.value).toBe(false);
    expect(gate.locked.value).toBe(true);
  });
});

describe("StartupFailureScreen.vue（schema 漂移按码呈现·issue #994）", () => {
  function channelPos(wrapper: ReturnType<typeof mount>, testid: string) {
    return wrapper.html().indexOf(testid);
  }

  it("探测返回 failed 携带漂移码：门记录错误码，恢复屏按码取材", async () => {
    wireInvokeSeam({
      overrides: {
        get_boot_status: () =>
          Promise.resolve({ phase: "failed", error_code: "boot.schema-drift" }),
      },
    });
    const gate = useEncryptionGate();
    await gate.probe();
    await flushPromises();
    expect(gate.bootFailed.value).toBe(true);
    expect(gate.bootErrorCode.value).toBe("boot.schema-drift");
  });

  it("漂移恢复屏：出「数据库结构异常」文案（不宣称库打不开），从备份恢复首选在前、重置次选在后", async () => {
    const gate = useEncryptionGate();
    gate.bootErrorCode.value = "boot.schema-drift";
    const { wrapper } = await mountFailedScreen();
    const html = wrapper.html();
    expect(html).toContain("数据库结构异常");
    expect(html).not.toContain("无法打开账本数据库");
    const restorePos = channelPos(wrapper, "failure-restore-open");
    const resetPos = channelPos(wrapper, "failure-reset-open");
    expect(restorePos).toBeGreaterThanOrEqual(0);
    expect(resetPos).toBeGreaterThan(restorePos);
    const restoreClasses = findButton(wrapper, "failure-restore-open")!.classes().join(" ");
    const resetClasses = findButton(wrapper, "failure-reset-open")!.classes().join(" ");
    expect(restoreClasses).toContain("n-button--primary-type");
    expect(resetClasses).not.toContain("n-button--error-type");
  });

  it("库不可读恢复屏（默认码）：重置通道在前且 error 实心，恢复通道在后（#601 既有顺序不回退）", async () => {
    const { wrapper } = await mountFailedScreen();
    const html = wrapper.html();
    expect(html).toContain("无法打开账本数据库");
    expect(html).not.toContain("数据库结构异常");
    const resetPos = channelPos(wrapper, "failure-reset-open");
    const restorePos = channelPos(wrapper, "failure-restore-open");
    expect(resetPos).toBeGreaterThanOrEqual(0);
    expect(restorePos).toBeGreaterThan(resetPos);
    expect(findButton(wrapper, "failure-reset-open")!.classes().join(" ")).toContain(
      "n-button--error-type",
    );
  });

  it("漂移场景重置仍为 error 级二次确认（ADR-0078 语义不变）", async () => {
    const gate = useEncryptionGate();
    gate.bootErrorCode.value = "boot.schema-drift";
    const { wrapper } = await mountFailedScreen({
      reset_after_startup_failure: () => Promise.resolve(),
    });
    await findButton(wrapper, "failure-reset-open")!.trigger("click");
    await waitModal();
    expect(wrapper.html()).toContain("重置为空库？");
    const alert = wrapper.find(".n-modal .n-alert");
    expect(alert.exists(), "红色警示块应存在").toBe(true);
  });

  it("库版本超前恢复屏（boot.schema-ahead）：按码取「升级」文案，只出升级通道、无重置/恢复", async () => {
    const gate = useEncryptionGate();
    gate.bootErrorCode.value = "boot.schema-ahead";
    const { wrapper } = await mountFailedScreen();
    const html = wrapper.html();
    expect(html).toContain("账本由更新版本的应用创建");
    expect(html).not.toContain("无法打开账本数据库");
    expect(findButton(wrapper, "failure-upgrade-open").exists()).toBe(true);
    expect(findButton(wrapper, "failure-reset-open").exists()).toBe(false);
    expect(findButton(wrapper, "failure-restore-open").exists()).toBe(false);
  });

  it("升级通道点击打开发布页（URL 与「通用 → 更新」卡片同源单点）", async () => {
    mockOpenUrl.mockClear();
    const gate = useEncryptionGate();
    gate.bootErrorCode.value = "boot.schema-ahead";
    const { wrapper } = await mountFailedScreen();
    await findButton(wrapper, "failure-upgrade-open")!.trigger("click");
    await flushPromises();
    expect(mockOpenUrl).toHaveBeenCalledWith(RELEASE_PAGE_URL);
  });
});

describe("StartupFailureScreen.vue（备份恢复通道·issue #602）", () => {
  function modalEl(wrapper: ReturnType<typeof mount>, testid: string) {
    return wrapper.find(`[data-testid="${testid}"]`);
  }

  async function typePassphrase(wrapper: ReturnType<typeof mount>, value: string) {
    const { NInput } = await import("naive-ui");
    const input = wrapper.findComponent(NInput);
    input.vm.$emit("update:value", value);
    await flushPromises();
  }

  async function mountWithRestore(
    overrides: Record<string, (args?: Record<string, unknown>) => unknown> = {},
  ) {
    stubRestoreChannel("/tmp/plain.db.zip", overrides);
    const gate = useEncryptionGate();
    gate.bootFailed.value = true;
    const wrapper = mount(StartupFailureScreen, {
      global: { stubs: { teleport: true } },
    });
    await flushPromises();
    return { wrapper, gate };
  }

  async function openRestoreModal(wrapper: ReturnType<typeof mount>) {
    await findButton(wrapper, "failure-restore-open")!.trigger("click");
    await waitModal();
  }

  it("恢复通道入口就位：标题、说明与「从备份文件恢复…」按钮可见", async () => {
    const { wrapper } = await mountWithRestore();
    const html = wrapper.html();
    expect(html).toContain("从备份文件恢复");
    expect(html).toContain("备份时的状态");
    expect(findButton(wrapper, "failure-restore-open")).toBeTruthy();
  });

  it("明文备份同模式：无跨模式警告、无需口令，确认后恢复并自动重启", async () => {
    const { wrapper } = await mountWithRestore();
    await openRestoreModal(wrapper);

    expect(mockInvoke.mock.calls.some(([c]) => c === "get_backup_meta")).toBe(true);
    expect(mockInvoke.mock.calls.some(([c]) => c === "get_encryption_status")).toBe(true);
    expect(wrapper.text()).not.toContain("加密模式不一致");
    expect(mockInvoke.mock.calls.some(([c]) => c === "restore_backup")).toBe(false);

    await modalEl(wrapper, "restore-confirm")!.trigger("click");
    await flushPromises();
    expect(mockInvoke).toHaveBeenCalledWith("restore_backup", {
      backupPath: "/tmp/plain.db.zip",
      passphrase: null,
    });
    expect(restartAppShortly).toHaveBeenCalledTimes(1);
  });

  it("密文备份跨模式（当前明文库）：显著警告 + 口令必填（无上下文口令直接弹口令框）", async () => {
    const { wrapper } = await mountWithRestore({
      get_backup_meta: () => Promise.resolve({ kind: "manual", encrypted: true }),
    });
    await openRestoreModal(wrapper);

    expect(wrapper.text()).toContain("加密模式不一致");
    expect(wrapper.text()).toContain("恢复后此库将变为加密库，应用重启后需凭该备份的主口令解锁");
    expect(modalEl(wrapper, "restore-passphrase").exists()).toBe(true);
    expect((modalEl(wrapper, "restore-confirm").element as HTMLButtonElement).disabled).toBe(true);
    expect(mockInvoke.mock.calls.some(([c]) => c === "restore_backup")).toBe(false);
  });

  it("密文备份口令重试：错误口令不关弹窗，重输后恢复成功并重启", async () => {
    let failFirst = true;
    const { wrapper } = await mountWithRestore({
      get_backup_meta: () => Promise.resolve({ kind: "manual", encrypted: true }),
      restore_backup: () =>
        failFirst
          ? Promise.reject({
              kind: "Coded",
              code: "encryption.passphrase-incorrect",
              message: "口令错误或文件损坏，请重试",
            })
          : Promise.resolve({ schema_version: 42, restored_at: "2026-09-06T00:00:00Z" }),
    });
    await openRestoreModal(wrapper);

    await typePassphrase(wrapper, "wrong");
    await modalEl(wrapper, "restore-confirm").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("口令错误或文件损坏，请重试");
    expect(modalEl(wrapper, "restore-confirm").exists()).toBe(true);
    expect(restartAppShortly).not.toHaveBeenCalled();

    failFirst = false;
    await typePassphrase(wrapper, "right-pw");
    await modalEl(wrapper, "restore-confirm").trigger("click");
    await flushPromises();
    expect(mockInvoke).toHaveBeenLastCalledWith("restore_backup", {
      backupPath: "/tmp/plain.db.zip",
      passphrase: "right-pw",
    });
    expect(restartAppShortly).toHaveBeenCalledTimes(1);
  });

  it("备份元数据读取失败：报错中止，不打开确认弹窗、不发恢复命令", async () => {
    const { wrapper } = await mountWithRestore({
      get_backup_meta: () =>
        Promise.reject({ kind: "Invalid", message: "不是有效的备份包", code: "backup.corrupt" }),
    });
    await openRestoreModal(wrapper);
    expect(mockInvoke.mock.calls.some(([c]) => c === "get_backup_meta")).toBe(true);
    expect(modalEl(wrapper, "restore-confirm").exists()).toBe(false);
    expect(mockInvoke.mock.calls.some(([c]) => c === "restore_backup")).toBe(false);
  });

  it("当前模式探测失败：报错中止，不打开确认弹窗（宁可不弹窗不跳过警告）", async () => {
    const { wrapper } = await mountWithRestore({
      get_encryption_status: () =>
        Promise.reject({ kind: "Invalid", message: "探测失败", code: "boot.db-unreadable" }),
    });
    await openRestoreModal(wrapper);
    expect(modalEl(wrapper, "restore-confirm").exists()).toBe(false);
    expect(mockInvoke.mock.calls.some(([c]) => c === "restore_backup")).toBe(false);
  });
});
