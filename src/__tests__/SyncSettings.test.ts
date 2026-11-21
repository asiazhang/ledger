import { beforeAll, describe, it, expect, vi } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import {
  findButtonByTestId,
  findInputByTestId,
  findBodyButtonByTestId,
} from "@ledger/test-support/dom";
import { DOMWrapper, mount, flushPromises } from "@vue/test-utils";
import { makeParkedOp, makeSyncChannelConfig, makeSyncStatus } from "./factories";

import SyncSettings from "@/settings/SyncSettings.vue";

beforeAll(() => {
  Element.prototype.scrollTo = () => {};
});

async function openVendorMenu(wrapper: ReturnType<typeof mount>): Promise<string[]> {
  await wrapper.find('[data-testid="sync-vendor"] .n-base-selection').trigger("click");
  await flushPromises();
  return Array.from(document.body.querySelectorAll(".n-base-select-option")).map(
    (el) => el.textContent?.trim() ?? "",
  );
}

async function pickVendor(wrapper: ReturnType<typeof mount>, labelPart: string) {
  const option = (await openVendorMenu(wrapper)).findIndex((text) => text.includes(labelPart));
  expect(option, `下拉里应能看到「${labelPart}」选项`).toBeGreaterThanOrEqual(0);
  const optionEl = document.body.querySelectorAll(".n-base-select-option")[option];
  await new DOMWrapper(optionEl).trigger("click");
  await flushPromises();
}

function selectedVendorText(wrapper: ReturnType<typeof mount>): string {
  return wrapper.find('[data-testid="sync-vendor"]').text();
}

describe("SyncSettings.vue 渲染冒烟", () => {
  it("明文库：明文警示 Alert 在场（ADR-0091 决策 8 界面义务），密文提示与口令框不在场", async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
    });
    const wrapper = mount(SyncSettings);
    await flushPromises();
    const html = wrapper.html();
    expect(html).toContain("2026-01-15 08:30");
    expect(html).toContain("明文存放于对象存储");
    expect(html).not.toContain("密文形态");
    expect(findInputByTestId(wrapper, "sync-passphrase").exists()).toBe(false);
  });

  it("密文库：密文提示 Alert 与主口令输入在场，不再警示明文", async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus({ library_encrypted: true }),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
    });
    const wrapper = mount(SyncSettings);
    await flushPromises();
    const html = wrapper.html();
    expect(html).toContain("密文形态");
    expect(html).not.toContain("明文存放于对象存储");
    expect(findInputByTestId(wrapper, "sync-passphrase").exists()).toBe(true);
  });

  it("从未同步：渲染「从未同步」占位而非空时间", async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus({ last_sync_at: null }),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
    });
    const wrapper = mount(SyncSettings);
    await flushPromises();
    expect(wrapper.html()).toContain("从未同步");
  });

  it("密钥输入恒空串起填、已保存密钥不上屏，占位提示「留空则保持不变」；其余 S3 字段照常回显", async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
    });
    const wrapper = mount(SyncSettings);
    await flushPromises();

    const secret = findInputByTestId(wrapper, "sync-secret-key");
    expect(secret.exists()).toBe(true);
    expect((secret.element as HTMLInputElement).value).toBe("");
    expect(wrapper.html()).not.toContain("secret-value");
    expect(secret.attributes("placeholder")).toContain("留空则保持不变");
    expect((findInputByTestId(wrapper, "sync-endpoint").element as HTMLInputElement).value).toBe(
      "https://s3.example.com",
    );
    expect((findInputByTestId(wrapper, "sync-region").element as HTMLInputElement).value).toBe(
      "us-east-1",
    );
    expect((findInputByTestId(wrapper, "sync-bucket").element as HTMLInputElement).value).toBe(
      "ledger-bucket",
    );
    expect((findInputByTestId(wrapper, "sync-prefix").element as HTMLInputElement).value).toBe(
      "sync",
    );
    expect((findInputByTestId(wrapper, "sync-access-key").element as HTMLInputElement).value).toBe(
      "AKIAEXAMPLE",
    );
  });

  it("未配置通道：S3 空表单起填，密钥占位为普通字段名（不暗示已有密钥在位）", async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus({ channel_configured: false }),
        get_sync_channel_config: makeSyncChannelConfig({
          endpoint: "",
          region: "",
          bucket: "",
          prefix: "",
          access_key: "",
          secret_key: "",
          path_style: false,
          configured: false,
        }),
      },
    });
    const wrapper = mount(SyncSettings);
    await flushPromises();

    expect((findInputByTestId(wrapper, "sync-endpoint").element as HTMLInputElement).value).toBe(
      "",
    );
    expect(findInputByTestId(wrapper, "sync-secret-key").attributes("placeholder")).toBe(
      "Secret Access Key",
    );
  });

  it("testid 冒烟：状态行、动作按钮、通道字段与寻址开关在位；无挂起时明细不渲染", async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
    });
    const wrapper = mount(SyncSettings);
    await flushPromises();

    for (const testid of [
      "sync-last-time",
      "sync-parked-count",
      "sync-now",
      "sync-vendor",
      "sync-vendor-hint",
      "sync-endpoint",
      "sync-region",
      "sync-bucket",
      "sync-prefix",
      "sync-access-key",
      "sync-secret-key",
      "sync-path-style",
      "sync-space",
      "sync-test-connection",
      "sync-save-channel",
      "sync-publish-checkpoint",
      "sync-bootstrap",
    ]) {
      expect(
        findButtonByTestId(wrapper, testid).exists() || findInputByTestId(wrapper, testid).exists(),
        testid,
      ).toBe(true);
      expect(wrapper.find(`[data-testid="${testid}"]`).exists(), testid).toBe(true);
    }
    expect(wrapper.find('[data-testid="sync-parked-list"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="sync-parked-hint"]').exists()).toBe(false);
  });

  it("挂起明细渲染：逐条按码化原因本地化呈现，带参码插出动态值（issue #863 / #957 渲染面）", async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus({ parked_count: 2 }),
        get_sync_channel_config: makeSyncChannelConfig(),
        get_parked_ops: [
          makeParkedOp({ message: "RAW" }),
          makeParkedOp({
            op_id: "op-2",
            code: "account.not-found",
            params: ["acc-1"],
            message: "RAW",
          }),
        ],
      },
    });
    const wrapper = mount(SyncSettings);
    await flushPromises();

    const list = wrapper.find('[data-testid="sync-parked-list"]');
    expect(list.exists()).toBe(true);
    expect(list.text()).toContain("该操作来自更新版本的应用");
    expect(list.text()).toContain("acc-1");
    expect(list.text()).not.toContain("{0}");
    expect(list.text()).not.toContain("RAW");
  });

  it("未配置通道：发布与引导入口均禁用（引导依赖通道在位）", async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus({ channel_configured: false }),
        get_sync_channel_config: makeSyncChannelConfig({ configured: false }),
      },
    });
    const wrapper = mount(SyncSettings);
    await flushPromises();
    expect(
      findButtonByTestId(wrapper, "sync-publish-checkpoint").attributes("disabled"),
    ).toBeDefined();
    expect(findButtonByTestId(wrapper, "sync-bootstrap").attributes("disabled")).toBeDefined();
  });

  it("引导弹窗 teleport 交互走通：预检回显在 body 弹窗内，取消即关闭", async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
        get_sync_channel_checkpoint: {
          generation: 3,
          size: 2 * 1024 * 1024,
          created_at: "2026-01-15T08:00:00Z",
        },
      },
    });
    const wrapper = mount(SyncSettings);
    await flushPromises();

    await findButtonByTestId(wrapper, "sync-bootstrap").trigger("click");
    await flushPromises();

    expect(mockInvoke).toHaveBeenCalledWith("get_sync_channel_checkpoint");
    const modalHtml = document.body.querySelector(".n-modal")?.innerHTML ?? "";
    expect(modalHtml).toContain("整库替换");
    expect(modalHtml).toContain("第 3 代");
    expect(modalHtml).toContain("2.0 MB");
    expect(document.body.querySelector('.n-modal input[type="password"]')).not.toBeNull();

    await findBodyButtonByTestId("sync-bootstrap-cancel")!.trigger("click");
    await flushPromises();
    await vi.waitFor(() => {
      expect(document.body.querySelector<HTMLElement>(".n-modal")?.style.display).toBe("none");
    });
  });

  it("引导弹窗：通道上没有检查点时渲染指引且确认禁用（不发起引导）", async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
        get_sync_channel_checkpoint: null,
      },
    });
    const wrapper = mount(SyncSettings);
    await flushPromises();

    await findButtonByTestId(wrapper, "sync-bootstrap").trigger("click");
    await flushPromises();

    expect(document.body.querySelector(".n-modal")?.innerHTML).toContain("通道上还没有检查点");
    expect(findBodyButtonByTestId("sync-bootstrap-confirm")!.attributes("disabled")).toBeDefined();
    expect(mockInvoke).not.toHaveBeenCalledWith("bootstrap_sync_from_channel", expect.anything());
  });
});

describe("SyncSettings.vue 厂商预设冒烟（issue #1220）", () => {
  it("下拉包含国内主流厂商预设，末尾固定「其他（自定义）」", async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
    });
    const wrapper = mount(SyncSettings);
    await flushPromises();

    expect(wrapper.find('[data-testid="sync-vendor"]').exists()).toBe(true);

    const options = await openVendorMenu(wrapper);
    expect(options.length).toBeGreaterThan(1);
    expect(options.at(-1)).toBe("其他（自定义）");
    expect(options.filter((o) => o === "其他（自定义）")).toHaveLength(1);
    for (const name of ["阿里云 OSS", "腾讯云 COS", "华为云 OBS", "火山引擎 TOS", "七牛云 Kodo"]) {
      expect(
        options.some((o) => o.includes(name) && o.includes("未实测")),
        name,
      ).toBe(true);
    }
  });

  it("选中厂商：预填端点上屏、档位标注与官方文档外链渲染、寻址开关双向绑定走通", async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig(),
      },
    });
    const wrapper = mount(SyncSettings);
    await flushPromises();

    await pickVendor(wrapper, "阿里云 OSS");
    expect((findInputByTestId(wrapper, "sync-endpoint").element as HTMLInputElement).value).toBe(
      "https://s3.oss-cn-hangzhou.aliyuncs.com",
    );
    expect(wrapper.find('[data-testid="sync-vendor-tier"]').text()).toBe("未实测");
    const docs = wrapper.find('[data-testid="sync-vendor-docs"]');
    expect(docs.attributes("href")).toBe(
      "https://help.aliyun.com/zh/oss/developer-reference/use-amazon-s3-sdks-to-access-oss",
    );
    expect(docs.attributes("target")).toBe("_blank");
    const beijing = wrapper
      .findAll('[data-testid="sync-vendor-region"]')
      .find((b) => b.text() === "cn-beijing");
    expect(beijing).toBeTruthy();
    await beijing?.trigger("click");
    await flushPromises();
    expect((findInputByTestId(wrapper, "sync-endpoint").element as HTMLInputElement).value).toBe(
      "https://s3.oss-cn-beijing.aliyuncs.com",
    );
    expect((findInputByTestId(wrapper, "sync-region").element as HTMLInputElement).value).toBe(
      "cn-beijing",
    );

    await pickVendor(wrapper, "华为云 OBS");
    expect(wrapper.find('[data-testid="sync-path-style"]').attributes("aria-checked")).toBe(
      "false",
    );

    await pickVendor(wrapper, "七牛云 Kodo");
    expect(wrapper.find('[data-testid="sync-path-style"]').attributes("aria-checked")).toBe("true");
  });

  it("再次打开按端点反查回显厂商；未命中显示「其他（自定义）」", async () => {
    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig({
          endpoint: "https://bucket-1250000000.cos.ap-guangzhou.myqcloud.com",
        }),
      },
    });
    const hit = mount(SyncSettings);
    await flushPromises();
    expect(selectedVendorText(hit)).toContain("腾讯云 COS");
    expect(hit.find('[data-testid="sync-vendor-tier"]').text()).toBe("未实测");

    wireInvokeSeam({
      defaults: {
        get_sync_status: makeSyncStatus(),
        get_sync_channel_config: makeSyncChannelConfig({ endpoint: "https://minio.internal:9000" }),
      },
    });
    const miss = mount(SyncSettings);
    await flushPromises();
    expect(selectedVendorText(miss)).toContain("其他（自定义）");
    expect(miss.find('[data-testid="sync-vendor-meta"]').exists()).toBe(false);
  });
});
