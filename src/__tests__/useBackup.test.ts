import { describe, it, expect, vi, beforeEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { defineComponent } from "vue";
import { setActivePinia, createPinia } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  save: vi.fn(),
  confirm: vi.fn(),
}));
// 重启钩子 mock：confirmRestore 成功后触发，断言恢复后重启语义用。
vi.mock("@/utils/restart", () => ({ restartAppShortly: vi.fn() }));

import { useAppStore } from "@/stores/app";
import {
  restoreCrossModeWarningKey,
  useBackup,
} from "@/composables/useBackup";
import { restartAppShortly } from "@/utils/restart";
import type { BackupFileInfo } from "@/types";

const mockInvoke = vi.mocked(invoke);
const mockListen = vi.mocked(listen);

const autoBackupFile: BackupFileInfo = {
  file_name: "ledger-auto-20260217-093000.db.zip",
  path: "/Users/me/backups/ledger-auto-20260217-093000.db.zip",
  size_bytes: 4096,
  created_at: "2026-02-17T09:30:00Z",
  kind: "auto",
  encrypted: true,
};

const manualBackupFile: BackupFileInfo = {
  file_name: "ledger-backup-20260101-010101.db.zip",
  path: "/Users/me/backups/ledger-backup-20260101-010101.db.zip",
  size_bytes: 1024,
  created_at: "2026-01-01T01:01:01Z",
  kind: "manual",
  encrypted: false,
};

function makeStub(initialList: BackupFileInfo[]) {
  let list: BackupFileInfo[] = initialList;
  let autoState = { enabled: true, last_backup_at: null };
  const listCalls = () =>
    mockInvoke.mock.calls.filter(([cmd]) => cmd === "list_backups").length;

  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === "list_backups") {
      return Promise.resolve(list);
    }
    if (cmd === "get_auto_backup_state") {
      return Promise.resolve(autoState);
    }
    if (cmd === "set_auto_backup_enabled" || cmd === "prune_backups") {
      return Promise.resolve({ kept: 0, deleted: [], failed: [] });
    }
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`));
  });

  return {
    listCalls,
    /** 模拟后端状态变化：下一次事件触发的重拉会拿到新数据。 */
    setList(next: BackupFileInfo[]) {
      list = next;
    },
    setAutoState(next: typeof autoState) {
      autoState = next;
    },
  };
}

/** 承载 composable 生命周期的宿主组件：setup 中捕获返回值供断言。 */
function mountHost() {
  let backup!: ReturnType<typeof useBackup>;
  const Host = defineComponent({
    setup() {
      backup = useBackup();
      return () => null;
    },
  });
  const wrapper = mount(Host);
  return { backup, wrapper };
}

beforeEach(() => {
  setActivePinia(createPinia());
  mockInvoke.mockReset();
  localStorage.clear();
  // 备份列表拉取以配置的目录为前提（未配置时列表恒空、不发 IPC）。
  useAppStore().setBackupDir("/Users/me/backups");
});

describe("useBackup 备份产物变更信号（issue #129）", () => {
  it("挂载时订阅 ledger:backups-changed（每次实例注册一次）", async () => {
    makeStub([]);
    mockListen.mockReset();
    mockListen.mockResolvedValue(vi.fn() as unknown as UnlistenFn);

    const { wrapper } = mountHost();
    await flushPromises();

    expect(mockListen).toHaveBeenCalledTimes(1);
    expect(mockListen).toHaveBeenCalledWith(
      "ledger:backups-changed",
      expect.any(Function),
    );
    wrapper.unmount();
  });

  it("卸载时注销监听", async () => {
    makeStub([]);
    mockListen.mockReset();
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten as unknown as UnlistenFn);

    const { wrapper } = mountHost();
    await flushPromises();
    wrapper.unmount();
    expect(unlisten).toHaveBeenCalled();
  });

  it("信号到达后自动刷新备份列表，无需手动刷新", async () => {
    const stub = makeStub([]);
    mockListen.mockReset();
    let fireBackupsChanged: () => void = () => {};
    mockListen.mockImplementation((_event: string, handler: never) => {
      fireBackupsChanged = handler;
      return Promise.resolve(vi.fn());
    });

    const { backup } = mountHost();
    await flushPromises();
    expect(backup.backups.value).toEqual([]);
    expect(stub.listCalls()).toBe(1);

    // 后端自动备份完成 → 产物出现 → 发出信号。
    stub.setList([manualBackupFile, autoBackupFile]);
    fireBackupsChanged();
    await flushPromises();

    expect(backup.backups.value.map((b) => b.file_name)).toEqual([
      "ledger-backup-20260101-010101.db.zip",
      "ledger-auto-20260217-093000.db.zip",
    ]);
    expect(stub.listCalls()).toBe(2);
  });

  it("信号到达后同步刷新自动备份状态展示", async () => {
    const stub = makeStub([]);
    mockListen.mockReset();
    let fireBackupsChanged: () => void = () => {};
    mockListen.mockImplementation((_event: string, handler: never) => {
      fireBackupsChanged = handler;
      return Promise.resolve(vi.fn());
    });

    const { backup } = mountHost();
    await flushPromises();
    expect(backup.autoBackupLastText.value).toBe("从未");

    stub.setAutoState({ enabled: true, last_backup_at: "2026-02-17T09:30:00Z" });
    fireBackupsChanged();
    await flushPromises();

    expect(backup.autoBackupLastText.value).toBe("2026-02-17 09:30");
  });
});

describe("useBackup 来源列映射（issue #129）", () => {
  beforeEach(() => {
    mockListen.mockReset();
    mockListen.mockResolvedValue(vi.fn() as unknown as UnlistenFn);
  });

  it("auto/manual 分别映射为 自动/手动 文案", async () => {
    makeStub([autoBackupFile, manualBackupFile]);
    const { backup } = mountHost();
    await flushPromises();

    expect(backup.backupRows.value.map((r) => r.source_text)).toEqual([
      "自动",
      "手动",
    ]);
  });

  it("旧数据缺 kind 字段按手动回落（与后端兼容语义一致）", async () => {
    const legacy = manualBackupFile as Partial<BackupFileInfo>;
    delete legacy.kind;
    makeStub([legacy as BackupFileInfo]);
    const { backup } = mountHost();
    await flushPromises();

    expect(backup.backupRows.value[0].source_text).toBe("手动");
  });
});

describe("useBackup 加密语义（issue #572 / ADR-0075 决策 7）", () => {
  beforeEach(() => {
    mockListen.mockReset();
    mockListen.mockResolvedValue(vi.fn() as unknown as UnlistenFn);
    vi.mocked(restartAppShortly).mockClear();
  });

  it("backupRows 透传 encrypted 标记（备份列表锁形标记的数据源）", async () => {
    makeStub([autoBackupFile, manualBackupFile]);
    const { backup } = mountHost();
    await flushPromises();

    expect(backup.backupRows.value.map((r) => r.encrypted)).toEqual([true, false]);
  });

  it("restoreCrossModeWarningKey：跨模式返回对应警告文案 key，同模式为 null", () => {
    expect(restoreCrossModeWarningKey(false, true)).toBe(
      "settings.data.msg.restoreToPlaintextWarn",
    );
    expect(restoreCrossModeWarningKey(true, false)).toBe(
      "settings.data.msg.restoreToEncryptedWarn",
    );
    expect(restoreCrossModeWarningKey(false, false)).toBeNull();
    expect(restoreCrossModeWarningKey(true, true)).toBeNull();
  });

  it("pickRestore：密文备份开启恢复意图并携带跨模式载荷", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "list_backups") return Promise.resolve([]);
      if (cmd === "get_auto_backup_state")
        return Promise.resolve({ enabled: true, last_backup_at: null });
      if (cmd === "get_backup_meta")
        return Promise.resolve({ kind: "manual", encrypted: true });
      if (cmd === "get_encryption_status")
        return Promise.resolve({ locked: false, file_encrypted: true });
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`));
    });
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValue("/Users/me/backups/enc.db.zip");

    const { backup } = mountHost();
    await flushPromises();
    await backup.pickRestore();
    await flushPromises();

    expect(
      mockInvoke.mock.calls.filter(([c]) => c === "get_backup_meta"),
    ).toHaveLength(1);
    expect(backup.restoreIntent.value).toEqual({
      path: "/Users/me/backups/enc.db.zip",
      backupEncrypted: true,
      currentEncrypted: true,
    });
  });

  it("pickRestore：读取备份元数据失败报错且不开启弹窗", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "list_backups") return Promise.resolve([]);
      if (cmd === "get_auto_backup_state")
        return Promise.resolve({ enabled: true, last_backup_at: null });
      if (cmd === "get_backup_meta") return Promise.reject(new Error("bad zip"));
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`));
    });
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValue("/Users/me/backups/broken.zip");

    const { backup } = mountHost();
    await flushPromises();
    await backup.pickRestore();
    await flushPromises();

    expect(backup.restoreIntent.value).toBeNull();
  });

  it("pickRestore：加密状态读取失败中止不开弹窗（不静默回落为明文）", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "list_backups") return Promise.resolve([]);
      if (cmd === "get_auto_backup_state")
        return Promise.resolve({ enabled: true, last_backup_at: null });
      if (cmd === "get_backup_meta")
        return Promise.resolve({ kind: "manual", encrypted: false });
      if (cmd === "get_encryption_status")
        return Promise.reject(new Error("status unavailable"));
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`));
    });
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValue("/Users/me/backups/plain.db.zip");

    const { backup } = mountHost();
    await flushPromises();
    await backup.pickRestore();
    await flushPromises();

    // 跨模式警告是销毁性操作前的安全面：当前库模式未知时宁可不弹窗。
    expect(backup.restoreIntent.value).toBeNull();
  });

  it("confirmRestore：密文备份附带主口令，成功后关闭意图并重启", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "list_backups") return Promise.resolve([]);
      if (cmd === "get_auto_backup_state")
        return Promise.resolve({ enabled: true, last_backup_at: null });
      if (cmd === "get_backup_meta")
        return Promise.resolve({ kind: "manual", encrypted: true });
      if (cmd === "get_encryption_status")
        return Promise.resolve({ locked: false, file_encrypted: false });
      if (cmd === "restore_backup")
        return Promise.resolve({ schema_version: 12, restored_at: "2026-02-17T00:00:00Z" });
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`));
    });
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValue("/Users/me/backups/enc.db.zip");

    const { backup } = mountHost();
    await flushPromises();
    await backup.pickRestore();
    await flushPromises();
    await backup.confirmRestore("pw");
    await flushPromises();

    const restoreCall = mockInvoke.mock.calls.find(([c]) => c === "restore_backup");
    expect(restoreCall?.[1]).toEqual({
      backupPath: "/Users/me/backups/enc.db.zip",
      passphrase: "pw",
    });
    expect(backup.restoreIntent.value).toBeNull();
    // 恢复成功后应用重启，由启动探测接管实际模式（ADR-0075 决策 4/7）。
    expect(restartAppShortly).toHaveBeenCalled();
  });

  it("confirmRestore：明文备份不消费口令（passphrase 传 null）", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "list_backups") return Promise.resolve([]);
      if (cmd === "get_auto_backup_state")
        return Promise.resolve({ enabled: true, last_backup_at: null });
      if (cmd === "get_backup_meta")
        return Promise.resolve({ kind: "manual", encrypted: false });
      if (cmd === "get_encryption_status")
        return Promise.resolve({ locked: false, file_encrypted: false });
      if (cmd === "restore_backup")
        return Promise.resolve({ schema_version: 12, restored_at: "2026-02-17T00:00:00Z" });
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`));
    });
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValue("/Users/me/backups/plain.db.zip");

    const { backup } = mountHost();
    await flushPromises();
    await backup.pickRestore();
    await flushPromises();
    await backup.confirmRestore("");
    await flushPromises();

    const restoreCall = mockInvoke.mock.calls.find(([c]) => c === "restore_backup");
    expect(restoreCall?.[1]).toEqual({
      backupPath: "/Users/me/backups/plain.db.zip",
      passphrase: null,
    });
  });

  it("confirmRestore：明文谎报实库为密文（后端报需口令）时，重输口令随请求上送", async () => {
    // 元数据缺标记视为明文（intent.backupEncrypted=false），弹窗显出口令框
    // 后用户重输：口令非空即上送，后端凭它打开实库为密文的备份（不再空转）。
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "list_backups") return Promise.resolve([]);
      if (cmd === "get_auto_backup_state")
        return Promise.resolve({ enabled: true, last_backup_at: null });
      if (cmd === "get_backup_meta")
        return Promise.resolve({ kind: "manual", encrypted: false });
      if (cmd === "get_encryption_status")
        return Promise.resolve({ locked: false, file_encrypted: false });
      if (cmd === "restore_backup")
        return Promise.resolve({ schema_version: 12, restored_at: "2026-02-17T00:00:00Z" });
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`));
    });
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValue("/Users/me/backups/lied-plain.db.zip");

    const { backup } = mountHost();
    await flushPromises();
    await backup.pickRestore();
    await flushPromises();
    await backup.confirmRestore("real-pw");
    await flushPromises();

    const restoreCall = mockInvoke.mock.calls.find(([c]) => c === "restore_backup");
    expect(restoreCall?.[1]).toEqual({
      backupPath: "/Users/me/backups/lied-plain.db.zip",
      passphrase: "real-pw",
    });
  });

  it("confirmRestore：失败不关弹窗（口令错误可就地重试）", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "list_backups") return Promise.resolve([]);
      if (cmd === "get_auto_backup_state")
        return Promise.resolve({ enabled: true, last_backup_at: null });
      if (cmd === "get_backup_meta")
        return Promise.resolve({ kind: "manual", encrypted: true });
      if (cmd === "get_encryption_status")
        return Promise.resolve({ locked: false, file_encrypted: false });
      if (cmd === "restore_backup")
        return Promise.reject({ kind: "Coded", code: "encryption.passphrase-incorrect", message: "口令错误或文件损坏，请重试" });
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`));
    });
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValue("/Users/me/backups/enc.db.zip");

    const { backup } = mountHost();
    await flushPromises();
    await backup.pickRestore();
    await flushPromises();
    await expect(backup.confirmRestore("wrong")).rejects.toBeTruthy();
    await flushPromises();

    expect(backup.restoreIntent.value).not.toBeNull();
  });
});
