import { describe, it, expect } from "vitest";
import { mockInvoke } from "@ledger/test-support/invoke-mock";
import { messageApi } from "@ledger/test-support/message-mock";
import { findButton } from "@ledger/test-support/dom";
import { mount, flushPromises } from "@vue/test-utils";
import { NSelect } from "naive-ui";
import LogSettings from "@/settings/LogSettings.vue";

describe("LogSettings.vue — 日志等级下拉（spec #611）", () => {
  async function mountWithLogLevel(level: string) {
    mockInvoke.mockResolvedValueOnce({ level });
    const wrapper = mount(LogSettings);
    await flushPromises();
    return wrapper;
  }

  it("挂载后读取持久化档位并回显到下拉", async () => {
    const wrapper = await mountWithLogLevel("debug");
    expect(mockInvoke).toHaveBeenCalledWith("get_log_level");
    expect(wrapper.findComponent(NSelect).props("value")).toBe("debug");
  });

  it("渲染日志等级标签、下拉与静态提示（含 RUST_LOG 说明）", async () => {
    const wrapper = await mountWithLogLevel("info");
    expect(wrapper.findComponent(NSelect).exists()).toBe(true);
    expect(wrapper.text()).toContain("日志等级");
    expect(wrapper.text()).toContain("RUST_LOG");
  });

  it("改动下拉触发 set_log_level 并回写当前档位", async () => {
    const wrapper = await mountWithLogLevel("info");
    mockInvoke.mockResolvedValueOnce(undefined);
    wrapper.findComponent(NSelect).vm.$emit("update:value", "warn");
    await flushPromises();
    expect(mockInvoke).toHaveBeenCalledWith("set_log_level", { level: "warn" });
    expect(wrapper.findComponent(NSelect).props("value")).toBe("warn");
  });

  it("改动失败时回显保持原档位并提示", async () => {
    const wrapper = await mountWithLogLevel("info");
    mockInvoke.mockRejectedValueOnce("设置日志等级失败：后端错误");
    wrapper.findComponent(NSelect).vm.$emit("update:value", "trace");
    await flushPromises();
    expect(wrapper.findComponent(NSelect).props("value")).toBe("info");
    expect(messageApi.error).toHaveBeenCalledTimes(1);
    expect(messageApi.error.mock.calls[0][0]).toContain("设置日志等级失败");
  });
});

describe("LogSettings.vue — 打开日志目录（issue #283）", () => {
  function findOpenLogButton(wrapper: ReturnType<typeof mount>) {
    const btn = findButton(wrapper, "打开日志目录", { exact: true });
    expect(btn, "组件应渲染「打开日志目录」按钮").toBeTruthy();
    return btn!;
  }

  it("成功路径：点击按钮以新命令名 open_log_dir 调用 IPC 一次，无错误提示", async () => {
    mockInvoke.mockResolvedValue({ level: "info" });
    const wrapper = mount(LogSettings);
    await flushPromises();
    mockInvoke.mockClear();
    mockInvoke.mockResolvedValue(undefined);
    await findOpenLogButton(wrapper).trigger("click");
    await flushPromises();
    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(mockInvoke).toHaveBeenCalledWith("open_log_dir");
    expect(messageApi.error).not.toHaveBeenCalled();
  });

  it("失败路径：错误提示原样透传后端中文错误，前缀不双层", async () => {
    const backendError = "打开日志目录失败：权限不足";
    mockInvoke.mockResolvedValue({ level: "info" });
    const wrapper = mount(LogSettings);
    await flushPromises();
    mockInvoke.mockClear();
    mockInvoke.mockRejectedValue(backendError);
    await findOpenLogButton(wrapper).trigger("click");
    await flushPromises();
    expect(messageApi.error).toHaveBeenCalledTimes(1);
    expect(messageApi.error).toHaveBeenCalledWith(backendError);
  });
});
