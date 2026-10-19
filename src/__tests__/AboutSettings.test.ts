import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { registerToastSink } from "@ledger/loadable";
import { makeFakeSink, resetToastSink } from "./factories";
import AboutSettings from "@/settings/AboutSettings.vue";
import { formatPlatform, resetAppBuildInfoForTest } from "@/settings/useAppBuildInfo";
import { ISSUES_URL, PROJECT_HOME_URL } from "@/settings/app-links";

// 「关于」Tab 是只读元信息 + 读型动作 + 静态简介（issue #930 / #1957 / #1959 /
// ADR-0022 修订注记）：应用名称、版本、Git 版本、运行平台、许可证、版权，加复制与
// 两个外链，以及卡片顶部的定位语与简介（纯静态只读，不进复制文本）。
// 日志等级与打开日志目录用例住 LogSettings.test.ts（issue #930 迁出）。

const writeText = vi.fn().mockResolvedValue(undefined);
const openUrl = vi.fn().mockResolvedValue(undefined);
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn((url: string) => openUrl(url)) }));

// 固定值注入全局常量：生产环境由 vite define 注入，测试用 stubGlobal 等价注入
const FULL_SHA = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0";

/** 壳层 `app_build_info` 的契约快照（编译期平台/架构原值 + 安装包版本）。 */
const BUILD_INFO = { version: "9.9.9", platform: "macos", arch: "aarch64" };

function mountWithGit(opts: { sha?: string; dirty?: boolean } = {}) {
  vi.stubGlobal("__GIT_SHA__", opts.sha ?? FULL_SHA);
  vi.stubGlobal("__GIT_DIRTY__", opts.dirty ?? false);
  wireInvokeSeam({ defaults: { app_build_info: BUILD_INFO } });
  return mount(AboutSettings);
}

beforeEach(() => {
  writeText.mockClear();
  openUrl.mockClear();
  resetAppBuildInfoForTest();
  Object.assign(navigator, { clipboard: { writeText } });
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetToastSink();
});

describe("AboutSettings.vue — 应用名称行（ADR-0076 显示名 i18n 化）", () => {
  it("应用名称行消费 i18n 应用名键，显示中文显示名而非硬编码旧名", () => {
    const wrapper = mountWithGit();
    expect(wrapper.text()).toContain("应用名称");
    expect(wrapper.text()).toContain("开源记账");
    expect(wrapper.text()).not.toContain("Ledger");
  });
});

describe("AboutSettings.vue — 版本与运行平台行（版本真源 + 平台名实，issue #1957）", () => {
  it("版本号取自壳层 app_build_info（删接线即红的负向条目）", async () => {
    const wrapper = mountWithGit();
    await flushPromises();

    // 调用事实：版本不是前端 package.json 的镜像，而是经命令取数
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "app_build_info")).toBe(true);
    // 渲染效果：屏幕上的版本号 = 安装包版本（注入的契约快照）
    expect(wrapper.find('[data-testid="about-row-version"]').text()).toBe(BUILD_INFO.version);
  });

  it("运行平台行显示编译期平台与架构，不再是技术栈叙述", async () => {
    const wrapper = mountWithGit();
    await flushPromises();

    expect(wrapper.find('[data-testid="about-row-platform"]').text()).toBe("macOS · arm64");
    expect(wrapper.text()).not.toContain("Tauri + Vue 3 + TypeScript");
  });

  it("构建元信息取数失败时版本与平台两行不渲染（宁缺不填假值）", async () => {
    vi.stubGlobal("__GIT_SHA__", FULL_SHA);
    vi.stubGlobal("__GIT_DIRTY__", false);
    wireInvokeSeam({
      overrides: { app_build_info: () => Promise.reject(new Error("壳层不可用")) },
    });
    const wrapper = mount(AboutSettings);
    await flushPromises();

    expect(wrapper.find('[data-testid="about-row-version"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="about-row-platform"]').exists()).toBe(false);
    // 其余行不受影响
    expect(wrapper.find('[data-testid="about-row-license"]').text()).toBe("MIT");
  });
});

describe("formatPlatform — 编译期常量到显示名", () => {
  it("macOS / arm64 与 Windows / x64 的平台显示名", () => {
    expect(formatPlatform({ version: "1.0.0", platform: "macos", arch: "aarch64" })).toBe(
      "macOS · arm64",
    );
    expect(formatPlatform({ version: "1.0.0", platform: "windows", arch: "x86_64" })).toBe(
      "Windows · x64",
    );
    expect(formatPlatform({ version: "1.0.0", platform: "android", arch: "aarch64" })).toBe(
      "Android · arm64",
    );
  });

  it("未登记取值原样透出（平台/架构是报障判据，不静默吞）", () => {
    expect(formatPlatform({ version: "1.0.0", platform: "haiku", arch: "riscv64" })).toBe(
      "haiku · riscv64",
    );
  });
});

describe("AboutSettings.vue — 许可证与版权行", () => {
  it("取 LICENSE 原文：MIT 与 © 2026 asiazhang", () => {
    const wrapper = mountWithGit();
    expect(wrapper.find('[data-testid="about-row-license"]').text()).toBe("MIT");
    expect(wrapper.find('[data-testid="about-row-copyright"]').text()).toBe("© 2026 asiazhang");
  });
});

describe("AboutSettings.vue — 复制版本信息（所见即所复制）", () => {
  it("复制出的每一行都对应屏幕上的同一行，行数一致", async () => {
    const wrapper = mountWithGit();
    await flushPromises();

    await wrapper.find('[data-testid="about-copy-info"]').trigger("click");
    expect(writeText).toHaveBeenCalledTimes(1);
    const copied = writeText.mock.calls[0]![0] as string;
    const lines = copied.split("\n");

    // 屏幕行（data-testid 锚定的值文本）逐行出现在复制文本里
    const screenValues = wrapper.findAll('[data-testid^="about-row-"]').map((node) => node.text());
    expect(screenValues.length).toBeGreaterThan(0);
    expect(lines).toHaveLength(screenValues.length);
    for (const value of screenValues) {
      expect(lines.some((line) => line.endsWith(`：${value}`))).toBe(true);
    }

    // 完整期望文本（标签 + 值，逐行）：报告 issue 时可直接粘贴
    expect(copied).toBe(
      [
        `应用名称：开源记账`,
        `版本号：${BUILD_INFO.version}`,
        `Git 版本：${FULL_SHA.slice(0, 7)}`,
        `运行平台：macOS · arm64`,
        `许可证：MIT`,
        `版权：© 2026 asiazhang`,
      ].join("\n"),
    );
  });
});

describe("AboutSettings.vue — 复制失败归统一错误通道（#1008 单点 sink）", () => {
  it("剪贴板拒绝时经 toast sink 报错（不自带 message.error）", async () => {
    const sink = makeFakeSink();
    registerToastSink(sink);
    writeText.mockRejectedValueOnce(new Error("剪贴板不可用"));

    const wrapper = mountWithGit();
    await flushPromises();
    await wrapper.find('[data-testid="about-copy-info"]').trigger("click");
    await flushPromises();

    expect(sink.error).toHaveBeenCalledTimes(1);
    expect(sink.error.mock.calls[0]![0]).toContain("剪贴板不可用");
  });
});

describe("AboutSettings.vue — Git 版本行", () => {
  it("显示短 sha（完整 sha 前 7 位）", () => {
    const wrapper = mountWithGit();
    const row = wrapper.find('[data-testid="about-row-git"]');
    expect(row.exists()).toBe(true);
    expect(row.text()).toContain(FULL_SHA.slice(0, 7));
  });

  it("脏树追加 -dirty 后缀", () => {
    const wrapper = mountWithGit({ dirty: true });
    expect(wrapper.find('[data-testid="about-row-git"]').text()).toContain(
      `${FULL_SHA.slice(0, 7)}-dirty`,
    );
  });

  it("干净树不出现 -dirty 后缀", () => {
    const wrapper = mountWithGit({ dirty: false });
    expect(wrapper.find('[data-testid="about-row-git"]').text()).not.toContain("-dirty");
  });

  it("sha 为空（无法读取 Git 信息）时不显示 Git 版本行", () => {
    const wrapper = mountWithGit({ sha: "" });
    expect(wrapper.find('[data-testid="about-row-git"]').exists()).toBe(false);
  });

  it("点击 Git 版本行复制完整 40 位 sha", async () => {
    const wrapper = mountWithGit({ dirty: true });
    await wrapper.find('[data-testid="about-row-git"]').trigger("click");
    expect(writeText).toHaveBeenCalledTimes(1);
    expect(writeText).toHaveBeenCalledWith(FULL_SHA);
    expect(writeText).toHaveBeenCalledWith(expect.not.stringContaining("-dirty"));
  });
});

describe("AboutSettings.vue — 软件简介（静态只读，issue #1959）", () => {
  it("卡片顶部渲染定位语与简介（删除渲染块即红）", () => {
    const wrapper = mountWithGit();
    const intro = wrapper.find('[data-testid="about-intro"]');
    expect(intro.exists()).toBe(true);
    expect(intro.text()).toContain("本地优先的开源个人账本");
    expect(intro.text()).toContain("数据只存在你的设备上");
  });

  it("简介不进复制版本信息（混入即红：报障粘贴格式保持纯元信息行）", async () => {
    const wrapper = mountWithGit();
    await flushPromises();

    await wrapper.find('[data-testid="about-copy-info"]').trigger("click");
    const copied = writeText.mock.calls[0]![0] as string;
    expect(copied).not.toContain("本地优先的开源个人账本");
    expect(copied).not.toContain("数据只存在你的设备上");
  });
});

describe("AboutSettings.vue — 项目主页与反馈入口（读型动作）", () => {
  it("项目主页入口以仓库地址调用 opener（断言实参 URL）", async () => {
    const wrapper = mountWithGit();
    await wrapper.find('[data-testid="about-link-home"]').trigger("click");
    await flushPromises();
    expect(openUrl).toHaveBeenCalledWith(PROJECT_HOME_URL);
  });

  it("反馈问题入口以 Issues 地址调用 opener（断言实参 URL）", async () => {
    const wrapper = mountWithGit();
    await wrapper.find('[data-testid="about-link-issues"]').trigger("click");
    await flushPromises();
    expect(openUrl).toHaveBeenCalledWith(ISSUES_URL);
  });
});
