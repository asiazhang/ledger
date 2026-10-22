import { beforeEach, describe, expect, it } from "vitest";
import { installMockBackend } from "./index";
import { invoke } from "./tauri/core";
import type { SyncChannelConfig, SyncRoundReport, SyncStatus } from "@ledger/types";

/**
 * 多端同步域替身测试（issue #1966 / spec #1964 / ADR-0143 决策 1）：环境壳不
 * 模拟同步引擎与通道——状态恒「未配置、无挂起、明文库」，手动同步答零计数
 * 轮次报告，挂起清单空，通道配置按未配置空表单读写，测试连接 void，检查点
 * 预检 null、发布与引导答占位回执，保证设置页同步卡片可导航不报错。
 * 接缝 = tauri 命令面，删除任一登记行即未命中报错、本文件变红。
 */

beforeEach(() => {
  installMockBackend(); // 重装即重播种：每用例从夹具种子起步
});

describe("同步状态：未配置、无挂起、明文库", () => {
  it("get_sync_status 答未配置无挂起的明文库状态", async () => {
    const status = await invoke<SyncStatus>("get_sync_status");
    expect(status.channel_configured).toBe(false);
    expect(status.last_sync_at).toBeNull();
    expect(status.parked_count).toBe(0);
    expect(status.library_encrypted).toBe(false);
    expect(status.device_id.length).toBeGreaterThan(0);
  });

  it("sync_now（口令可空）答零计数轮次报告，明文标记与明文库一致", async () => {
    const report = await invoke<SyncRoundReport>("sync_now", { passphrase: null });
    expect(report).toMatchObject({
      uploaded_segments: 0,
      uploaded_ops: 0,
      downloaded_segments: 0,
      applied: 0,
      deduped: 0,
      superseded: 0,
      skipped: 0,
      parked: 0,
      plaintext_mode: true,
    });
  });

  it("get_parked_ops 答空清单", async () => {
    await expect(invoke("get_parked_ops")).resolves.toEqual([]);
  });
});

describe("通道配置：未配置空表单", () => {
  it("get_sync_channel_config 答回显空表单（configured=false）", async () => {
    const config = await invoke<SyncChannelConfig>("get_sync_channel_config");
    expect(config.configured).toBe(false);
    expect(config.endpoint).toBe("");
    expect(config.secret_key).toBe("");
    // 全字段空表单形态（S3 七字段 + 同步空间 + path-style）
    expect([
      config.space_id,
      config.region,
      config.bucket,
      config.prefix,
      config.access_key,
    ]).toEqual(["", "", "", "", ""]);
    expect(config.path_style).toBe(false);
  });

  it("set_sync_channel_config 收表单答 void，回显保持未配置（常量态）", async () => {
    await expect(
      invoke<void>("set_sync_channel_config", {
        config: { endpoint: "https://s3.example.com", bucket: "ledger" },
      }),
    ).resolves.toBeUndefined();
    const config = await invoke<SyncChannelConfig>("get_sync_channel_config");
    expect(config.configured).toBe(false);
  });

  it("test_sync_channel_connection 收表单答 void（探针可点不报错）", async () => {
    await expect(
      invoke<void>("test_sync_channel_connection", {
        config: { endpoint: "https://s3.example.com" },
      }),
    ).resolves.toBeUndefined();
  });

  it("通道两命令缺 config 以含参数名的错误拒绝", async () => {
    await expect(invoke<void>("set_sync_channel_config", {})).rejects.toThrow(/参数 config/);
    await expect(invoke<void>("test_sync_channel_connection", {})).rejects.toThrow(/参数 config/);
  });
});

describe("检查点发布与引导：常量态应答", () => {
  it("get_sync_channel_checkpoint 预检答 null（无检查点，向导走「不可引导」支路）", async () => {
    await expect(invoke("get_sync_channel_checkpoint")).resolves.toBeNull();
  });

  it("publish_sync_checkpoint 答占位发布回执（形状：代数/字节数/明文标记）", async () => {
    const published = await invoke<{ generation: number; size: number; plaintext_mode: boolean }>(
      "publish_sync_checkpoint",
      { passphrase: null },
    );
    expect(published.generation).toBeGreaterThan(0);
    expect(typeof published.size).toBe("number");
    expect(published.plaintext_mode).toBe(true);
  });

  it("bootstrap_sync_from_channel 答占位引导回执（形状：代数/字节数/未重加密）", async () => {
    const outcome = await invoke<{ generation: number; size: number; reencrypted: boolean }>(
      "bootstrap_sync_from_channel",
      { passphrase: null },
    );
    expect(outcome.generation).toBeGreaterThan(0);
    expect(typeof outcome.size).toBe("number");
    expect(outcome.reencrypted).toBe(false);
  });
});
