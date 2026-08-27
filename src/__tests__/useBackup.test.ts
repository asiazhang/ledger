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

import { useAppStore } from "@/stores/app";
import { useBackup } from "@/composables/useBackup";
import type { BackupFileInfo } from "@/types";

const mockInvoke = vi.mocked(invoke);
const mockListen = vi.mocked(listen);

const autoBackupFile: BackupFileInfo = {
  file_name: "ledger-auto-20260217-093000.db.zip",
  path: "/Users/me/backups/ledger-auto-20260217-093000.db.zip",
  size_bytes: 4096,
  created_at: "2026-02-17T09:30:00Z",
  kind: "auto",
};

const manualBackupFile: BackupFileInfo = {
  file_name: "ledger-backup-20260101-010101.db.zip",
  path: "/Users/me/backups/ledger-backup-20260101-010101.db.zip",
  size_bytes: 1024,
  created_at: "2026-01-01T01:01:01Z",
  kind: "manual",
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
