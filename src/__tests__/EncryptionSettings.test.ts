import { describe, it, expect, vi, beforeEach } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { messageApi } from "@ledger/test-support/message-mock";
import { findButton, findBodyButton, findBodyButtonByTestId } from "@ledger/test-support/dom";
import { mount, flushPromises, DOMWrapper } from "@vue/test-utils";
import type { EncryptionStatus } from "@ledger/types";
import zhAll from "@ledger/i18n/locales/zh-CN";
import enAll from "@ledger/i18n/locales/en-US";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
  save: vi.fn(),
}));

import EncryptionSettings from "@/settings/EncryptionSettings.vue";
import { useEncryptionGate } from "@/backup/useEncryptionGate";
import { hasOpenOverlay, resetOverlays } from "@ledger/ui-kit/overlayRegistry";
import { useAppStore } from "@/stores/app";

const plaintextStatus: EncryptionStatus = { locked: false, file_encrypted: false };
const encryptedStatus: EncryptionStatus = { locked: false, file_encrypted: true };

const PASS_OK = "主口令至少八个字";
const PASS_ALT = "不一样的八个字符";
const PASS_SHORT = "短口令";

beforeEach(() => {
  resetOverlays();
  const { rememberSupport } = useEncryptionGate();
  rememberSupport.value = null;
});

function collapseItem(wrapper: ReturnType<typeof mount>, title: string) {
  const item = wrapper
    .findAll(".n-collapse-item")
    .find((el) => el.find(".n-collapse-item__header").text().includes(title));
  if (!item) throw new Error(`未找到折叠项：${title}`);
  return item;
}

async function expandCollapseItem(wrapper: ReturnType<typeof mount>, title: string) {
  await collapseItem(wrapper, title).find(".n-collapse-item__header-main").trigger("click");
  await flushPromises();
}

function collapseContent(wrapper: ReturnType<typeof mount>, title: string) {
  const content = collapseItem(wrapper, title).find(".n-collapse-item__content-wrapper");
  if (!content.exists()) throw new Error(`折叠项未展开或内容未挂载：${title}`);
  return content;
}

function modalAlert(): Element | null {
  return document.body.querySelector(".n-modal .n-alert");
}

function strengthMeters(scope: ReturnType<typeof mount> | ReturnType<typeof collapseContent>) {
  return scope.findAll('[data-testid="passphrase-strength"]');
}

const PASS_WEAK = "password123";
const PASS_VERY_STRONG = "qW7#mKx2$vLp9&zR4";

async function setPasswords(wrapper: ReturnType<typeof mount>, pass: string, confirm: string) {
  const inputs = wrapper.findAll("input");
  await inputs[0].setValue(pass);
  await inputs[1].setValue(confirm);
}

describe("EncryptionSettings.vue（设置页加密卡片）", () => {
  it("明文库：展示开启表单与无后门警示，确认一致后可提交", async () => {
    wireInvokeSeam({ defaults: { get_encryption_status: plaintextStatus } });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();
    const html = wrapper.html();
    expect(html).toContain("开启加密");
    expect(html).toContain("主口令");
    expect(html).toContain("无法读取数据");

    await setPasswords(wrapper, PASS_OK, PASS_OK);
    await flushPromises();
    const button = findButton(wrapper, "开启加密")!;
    expect(button.element.disabled).toBe(false);
  });

  it("两次输入不一致：禁用提交并给出即时反馈", async () => {
    wireInvokeSeam({ defaults: { get_encryption_status: plaintextStatus } });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await setPasswords(wrapper, PASS_OK, PASS_ALT);
    await flushPromises();
    const html = wrapper.html();
    expect(html).toContain("两次输入不一致");
    const button = findButton(wrapper, "开启加密")!;
    expect(button.element.disabled).toBe(true);
  });

  it("确认弹窗取消：不发起转换", async () => {
    wireInvokeSeam({ defaults: { get_encryption_status: plaintextStatus } });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await setPasswords(wrapper, PASS_OK, PASS_OK);
    await findButton(wrapper, "开启加密")!.trigger("click");
    await flushPromises();
    await findBodyButton("取消")!.trigger("click");
    await flushPromises();
    expect(mockInvoke).not.toHaveBeenCalledWith("enable_encryption", expect.anything());
  });

  it("已加密库：日常视图 = 已开启标识 + 自动解锁；修改主口令与关闭加密默认收起（issue #654）", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
        get_remember_passphrase_support: { supported: true },
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();
    const html = wrapper.html();
    expect(html).toContain("已开启加密");
    expect(html).toContain("自动解锁");
    expect(findButton(wrapper, "启用自动解锁")).toBeTruthy();
    expect(findButton(wrapper, "开启加密")).toBeUndefined();
    expect(html).toContain("修改主口令");
    expect(html).toContain("关闭加密");
    expect(html).not.toContain("关闭前请确认");
    expect(wrapper.findAll("input").length).toBe(0);
  });

  it("已加密库·折叠区：展开修改主口令后表单挂载，展开关闭加密后其表单挂载", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await expandCollapseItem(wrapper, "修改主口令");
    expect(wrapper.html()).toContain("新主口令");
    expect(collapseContent(wrapper, "修改主口令").findAll("input").length).toBe(3);
    expect(
      collapseItem(wrapper, "关闭加密").find(".n-collapse-item__content-wrapper").exists(),
    ).toBe(false);

    await expandCollapseItem(wrapper, "关闭加密");
    expect(wrapper.html()).toContain("关闭前请确认");
    expect(collapseContent(wrapper, "关闭加密").findAll("input").length).toBe(1);
  });

  it("已加密库·修改主口令：新旧不一致禁用提交并给出即时反馈", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await expandCollapseItem(wrapper, "修改主口令");
    const inputs = collapseContent(wrapper, "修改主口令").findAll("input");
    await inputs[0].setValue("旧口令");
    await inputs[1].setValue(PASS_OK);
    await inputs[2].setValue(PASS_ALT);
    await flushPromises();
    expect(wrapper.html()).toContain("两次输入不一致");
    expect(findButton(wrapper, "修改主口令")!.element.disabled).toBe(true);
  });

  it("已加密库·修改主口令：新口令与当前口令相同禁用提交并给出警示反馈", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await expandCollapseItem(wrapper, "修改主口令");
    const inputs = collapseContent(wrapper, "修改主口令").findAll("input");
    await inputs[0].setValue("同一口令八个字符");
    await inputs[1].setValue("同一口令八个字符");
    await inputs[2].setValue("同一口令八个字符");
    await flushPromises();
    expect(wrapper.html()).toContain("新主口令与当前主口令相同");
    expect(findButton(wrapper, "修改主口令")!.element.disabled).toBe(true);
  });

  it("已加密库·修改主口令：确认弹窗取消不发起转换，也不再调用系统原生对话框", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await expandCollapseItem(wrapper, "修改主口令");
    const inputs = collapseContent(wrapper, "修改主口令").findAll("input");
    await inputs[0].setValue("旧口令");
    await inputs[1].setValue(PASS_OK);
    await inputs[2].setValue(PASS_OK);
    await findButton(wrapper, "修改主口令")!.trigger("click");
    await flushPromises();
    await findBodyButtonByTestId("danger-cancel")!.trigger("click");
    await flushPromises();
    expect(mockInvoke).not.toHaveBeenCalledWith("change_encryption_passphrase", expect.anything());
  });

  it("已加密库·关闭加密：确认弹窗取消不发起转换", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await expandCollapseItem(wrapper, "关闭加密");
    await collapseContent(wrapper, "关闭加密").find("input").setValue("当前口令");
    await findButton(wrapper, "关闭加密")!.trigger("click");
    await flushPromises();
    await findBodyButtonByTestId("danger-cancel")!.trigger("click");
    await flushPromises();
    expect(mockInvoke).not.toHaveBeenCalledWith("disable_encryption", expect.anything());
  });
});

describe("危险确认分级试点（issue #650 / ADR-0078）", () => {
  it("开启表单：短于 8 位的主口令即时红显、提交禁用、不发起后端调用", async () => {
    wireInvokeSeam({ defaults: { get_encryption_status: plaintextStatus } });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await setPasswords(wrapper, PASS_SHORT, PASS_SHORT);
    await flushPromises();
    expect(wrapper.html()).toContain("主口令至少需要 8 位");
    const button = findButton(wrapper, "开启加密")!;
    expect(button.element.disabled).toBe(true);
    await button.trigger("click");
    await flushPromises();
    expect(mockInvoke).not.toHaveBeenCalledWith("enable_encryption", expect.anything());
    expect(document.body.querySelector('[data-testid="danger-confirm"]')).toBeNull();
    void wrapper;
  });

  it("开启表单：恰 8 位（边界值）不红、可提交", async () => {
    wireInvokeSeam({ defaults: { get_encryption_status: plaintextStatus } });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await setPasswords(wrapper, PASS_OK, PASS_OK);
    await flushPromises();
    expect(wrapper.html()).not.toContain("主口令至少需要 8 位");
    expect(findButton(wrapper, "开启加密")!.element.disabled).toBe(false);
  });

  it("开启确认弹窗：error 级视觉——红色警示块＋加粗后果句＋红色确认按钮，打开期间快捷键抑制", async () => {
    wireInvokeSeam({ defaults: { get_encryption_status: plaintextStatus } });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await setPasswords(wrapper, PASS_OK, PASS_OK);
    await findButton(wrapper, "开启加密")!.trigger("click");
    await flushPromises();

    const alert = modalAlert();
    expect(alert, "警示块应存在").toBeTruthy();
    expect(alert!.querySelector(".n-text--strong"), "后果句应加粗").toBeTruthy();
    expect(document.body.textContent).toContain("一旦忘记，这份账本的数据将无法再打开");
    expect(findBodyButtonByTestId("danger-confirm")!.classes()).toContain("n-button--error-type");
    expect(hasOpenOverlay()).toBe(true);

    await findBodyButtonByTestId("danger-cancel")!.trigger("click");
    await flushPromises();
    expect(hasOpenOverlay()).toBe(false);
    void wrapper;
  });

  it("修改主口令：新口令短于 8 位即时红显、提交禁用、不发起后端调用", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await expandCollapseItem(wrapper, "修改主口令");
    const inputs = collapseContent(wrapper, "修改主口令").findAll("input");
    await inputs[0].setValue("旧口令");
    await inputs[1].setValue(PASS_SHORT);
    await inputs[2].setValue(PASS_SHORT);
    await flushPromises();
    expect(wrapper.html()).toContain("主口令至少需要 8 位");
    const button = findButton(wrapper, "修改主口令")!;
    expect(button.element.disabled).toBe(true);
    await button.trigger("click");
    await flushPromises();
    expect(mockInvoke).not.toHaveBeenCalledWith("change_encryption_passphrase", expect.anything());
    expect(document.body.querySelector('[data-testid="danger-confirm"]')).toBeNull();
    void wrapper;
  });

  it("关闭加密：确认走 warning 级应用内弹窗（不再出现系统原生对话框），承载兜底说明", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await expandCollapseItem(wrapper, "关闭加密");
    await collapseContent(wrapper, "关闭加密").find("input").setValue("当前口令");
    await findButton(wrapper, "关闭加密")!.trigger("click");
    await flushPromises();

    const alert = modalAlert();
    expect(alert, "警示块应存在").toBeTruthy();
    expect(document.body.textContent).toContain("日后可随时重新开启加密");
    expect(findBodyButtonByTestId("danger-confirm")!.classes()).toContain("n-button--warning-type");
    expect(hasOpenOverlay()).toBe(true);

    await findBodyButtonByTestId("danger-cancel")!.trigger("click");
    await flushPromises();
    expect(hasOpenOverlay()).toBe(false);
    void wrapper;
  });

  it("修改主口令：确认走 error 级应用内弹窗（不再出现系统原生对话框），承载无后门后果说明", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await expandCollapseItem(wrapper, "修改主口令");
    const inputs = collapseContent(wrapper, "修改主口令").findAll("input");
    await inputs[0].setValue("旧口令");
    await inputs[1].setValue(PASS_OK);
    await inputs[2].setValue(PASS_OK);
    await findButton(wrapper, "修改主口令")!.trigger("click");
    await flushPromises();

    const alert = modalAlert();
    expect(alert, "警示块应存在").toBeTruthy();
    expect(alert!.querySelector(".n-text--strong"), "后果句应加粗").toBeTruthy();
    expect(document.body.textContent).toContain("一旦忘记，这份账本的数据将无法再打开");
    expect(findBodyButtonByTestId("danger-confirm")!.classes()).toContain("n-button--error-type");
    expect(hasOpenOverlay()).toBe(true);
    void wrapper;
  });
});

describe("EncryptionSettings.vue 自动解锁（issue #654 重做；原 #574）", () => {
  function findRememberCheckbox(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll(".n-checkbox").find((c) => c.text().includes("自动解锁"))!;
  }

  function modalPassInput(): DOMWrapper<HTMLInputElement> {
    const input = document.body.querySelector('.n-modal input[type="password"]');
    if (!input) throw new Error("弹窗内未找到口令输入框");
    return new DOMWrapper(input as HTMLInputElement);
  }

  it("术语统一「自动解锁」：全部 zh/en 资源无别名变体（issue #654）", () => {
    const zh = JSON.stringify(zhAll);
    const en = JSON.stringify(enAll);
    expect(zh).not.toContain("启动时自动解锁");
    expect(zh).not.toContain("本机记住");
    expect(en).not.toContain("Unlock automatically at launch");
    expect(en).not.toContain("enable remember");
    expect(en).not.toContain("remembering the master passphrase");
    expect(en.toLowerCase()).not.toContain("auto-unlock");
  });

  it("平台支持：明文库开启表单出现「自动解锁」复选项（勾选后的缓存流程归模块测试）", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: plaintextStatus,
        get_remember_passphrase_support: { supported: true },
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    expect(findRememberCheckbox(wrapper)).toBeTruthy();
  });

  it("平台不支持：隐藏「自动解锁」复选项与整个自动解锁区块", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: plaintextStatus,
        get_remember_passphrase_support: { supported: false },
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();
    expect(findButton(wrapper, "启用自动解锁")).toBeUndefined();
    expect(findButton(wrapper, "关闭自动解锁")).toBeUndefined();
    expect(wrapper.findAll(".n-checkbox").length).toBe(0);
  });

  it("已加密·未启用：「启用自动解锁…」单按钮入口，弹窗输入当前主口令确认后启用并有提示", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
        get_remember_passphrase_support: { supported: true },
      },
      overrides: {
        set_remember_passphrase: (args) => {
          expect(args?.passphrase).toBe("当前口令");
          return Promise.resolve();
        },
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    expect(wrapper.find(".n-switch").exists()).toBe(false);

    await findButton(wrapper, "启用自动解锁")!.trigger("click");
    await flushPromises();
    await modalPassInput().setValue("当前口令");
    await findBodyButtonByTestId("auto-unlock-confirm")!.trigger("click");
    await flushPromises();

    expect(mockInvoke).toHaveBeenCalledWith("set_remember_passphrase", { passphrase: "当前口令" });
    expect(useAppStore().rememberPassphrase).toBe(true);
    expect(messageApi.success).toHaveBeenCalled();
    expect(findButton(wrapper, "关闭自动解锁")).toBeTruthy();
  });

  it("已加密·口令错误：就地报错且不启用，弹窗保持打开可就地重试", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
        get_remember_passphrase_support: { supported: true },
      },
      overrides: {
        set_remember_passphrase: vi
          .fn()
          .mockRejectedValueOnce({
            kind: "Coded",
            message: "口令错误或文件损坏，请重试",
            code: "encryption.passphrase-incorrect",
          })
          .mockResolvedValueOnce(undefined),
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await findButton(wrapper, "启用自动解锁")!.trigger("click");
    await flushPromises();
    await modalPassInput().setValue("错口令");
    await findBodyButtonByTestId("auto-unlock-confirm")!.trigger("click");
    await flushPromises();

    expect(document.body.textContent).toContain("口令错误或文件损坏，请重试");
    expect(useAppStore().rememberPassphrase).toBe(false);
    expect(messageApi.success).not.toHaveBeenCalled();

    await modalPassInput().setValue("正确口令");
    await findBodyButtonByTestId("auto-unlock-confirm")!.trigger("click");
    await flushPromises();
    expect(useAppStore().rememberPassphrase).toBe(true);
    expect(messageApi.success).toHaveBeenCalled();
    expect(findButton(wrapper, "关闭自动解锁")).toBeTruthy();
  });

  it("已加密·已启用：关闭自动解锁立即生效（清缓存恢复手输）并有提示", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
        get_remember_passphrase_support: { supported: true },
        clear_remember_passphrase: null,
      },
    });
    useAppStore().setRememberPassphrase(true);
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    const disableBtn = findButton(wrapper, "关闭自动解锁")!;
    expect(disableBtn).toBeTruthy();
    await disableBtn.trigger("click");
    await flushPromises();

    expect(mockInvoke).toHaveBeenCalledWith("clear_remember_passphrase");
    expect(useAppStore().rememberPassphrase).toBe(false);
    expect(messageApi.success).toHaveBeenCalled();
    expect(findButton(wrapper, "启用自动解锁")).toBeTruthy();
  });

  it("已加密：自动解锁不再存在开关形态（无 .n-switch）", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
        get_remember_passphrase_support: { supported: true },
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();
    expect(wrapper.find(".n-switch").exists()).toBe(false);
  });

  it("开发回退形态（issue #662）：显示开发构建提示，区别于发布生物门形态", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
        get_remember_passphrase_support: { supported: true, mode: "dev-fallback" },
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();
    expect(wrapper.html()).toContain("当前为开发构建");
  });

  it("发布生物门形态（issue #662）：不显示开发构建提示", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
        get_remember_passphrase_support: { supported: true, mode: "biometry" },
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();
    expect(wrapper.html()).not.toContain("当前为开发构建");
  });
});

describe("口令强度实时显示（issue #685）", () => {
  it("术语纪律：全部 zh 资源无「密码强度」措辞（正式术语为「口令强度」）", () => {
    expect(JSON.stringify(zhAll)).not.toContain("密码强度");
  });

  it("开启表单：初始为空不显示强度条；输入弱口令显示「弱」，换强口令逐键刷新为「极强」", async () => {
    wireInvokeSeam({ defaults: { get_encryption_status: plaintextStatus } });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    expect(strengthMeters(wrapper).length).toBe(0);

    await wrapper.findAll("input")[0].setValue(PASS_WEAK);
    await flushPromises();
    expect(strengthMeters(wrapper).length).toBe(1);
    expect(strengthMeters(wrapper)[0].text()).toContain("弱");

    await wrapper.findAll("input")[0].setValue(PASS_VERY_STRONG);
    await flushPromises();
    expect(strengthMeters(wrapper).length).toBe(1);
    expect(strengthMeters(wrapper)[0].text()).toContain("极强");
  });

  it("开启表单：确认字段不显示强度条（仅新设主口令框显示）", async () => {
    wireInvokeSeam({ defaults: { get_encryption_status: plaintextStatus } });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await setPasswords(wrapper, PASS_WEAK, PASS_WEAK);
    await flushPromises();
    expect(strengthMeters(wrapper).length).toBe(1);
  });

  it("开启表单：短于 8 位时字段错误红显与强度条并存（互不替代）", async () => {
    wireInvokeSeam({ defaults: { get_encryption_status: plaintextStatus } });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await setPasswords(wrapper, PASS_SHORT, PASS_SHORT);
    await flushPromises();
    expect(wrapper.html()).toContain("主口令至少需要 8 位");
    expect(strengthMeters(wrapper).length).toBe(1);
    expect(strengthMeters(wrapper)[0].text()).toContain("弱");
  });

  it("纯展示：弱口令强度显示不改变提交可用性（≥8 位即由既有规则放行）", async () => {
    wireInvokeSeam({ defaults: { get_encryption_status: plaintextStatus } });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await setPasswords(wrapper, PASS_OK, PASS_OK);
    await flushPromises();
    expect(strengthMeters(wrapper).length).toBe(1);
    expect(findButton(wrapper, "开启加密")!.element.disabled).toBe(false);
  });

  it("修改主口令折叠区：仅「新主口令」显示强度条；当前主口令与确认新主口令不显示", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await expandCollapseItem(wrapper, "修改主口令");
    const inputs = collapseContent(wrapper, "修改主口令").findAll("input");
    await inputs[0].setValue("旧口令");
    await flushPromises();
    expect(strengthMeters(wrapper).length).toBe(0);

    await inputs[1].setValue(PASS_WEAK);
    await flushPromises();
    expect(strengthMeters(wrapper).length).toBe(1);
    expect(strengthMeters(wrapper)[0].text()).toContain("弱");

    await inputs[2].setValue(PASS_WEAK);
    await flushPromises();
    expect(strengthMeters(wrapper).length).toBe(1);
  });

  it("其余口令输入框不出现强度条：关闭加密表单与启用自动解锁弹窗", async () => {
    wireInvokeSeam({
      defaults: {
        get_encryption_status: encryptedStatus,
        get_remember_passphrase_support: { supported: true },
      },
    });
    const wrapper = mount(EncryptionSettings);
    await flushPromises();

    await expandCollapseItem(wrapper, "关闭加密");
    await collapseContent(wrapper, "关闭加密").find("input").setValue(PASS_VERY_STRONG);
    await flushPromises();
    expect(strengthMeters(wrapper).length).toBe(0);

    await findButton(wrapper, "启用自动解锁")!.trigger("click");
    await flushPromises();
    const modalInput = document.body.querySelector('.n-modal input[type="password"]')!;
    await new DOMWrapper(modalInput).setValue(PASS_VERY_STRONG);
    await flushPromises();
    expect(strengthMeters(wrapper).length).toBe(0);
  });
});
