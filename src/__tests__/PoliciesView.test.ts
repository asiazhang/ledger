import { describe, it, expect, vi, beforeEach, beforeAll } from "vitest";
import { assertMobileTierScrollX } from "@ledger/test-support/mobile-scroll-x";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises, DOMWrapper } from "@vue/test-utils";
import { NPopconfirm } from "naive-ui";
import PoliciesView from "@/views/PoliciesView.vue";
import PolicyFormModal from "@/policy/PolicyFormModal.vue";
import { makePolicy, makePolicyStats } from "./factories";
import { formatAmount } from "@ledger/money";
import { refCurrencies } from "@ledger/test-support/reference-stubs";
import type { Insurer, Policy, PolicyStats } from "@ledger/types";
import { componentVm } from "@ledger/test-support/component-vm";

const cny = refCurrencies[0];

const mockRoute = { query: {} as Record<string, string> };
vi.mock("vue-router", () => ({
  useRoute: () => mockRoute,
  useRouter: () => ({ push: vi.fn() }),
}));

function bodyQuery(selector: string): HTMLElement | null {
  return document.body.querySelector(selector);
}

const mockInsurers: Insurer[] = [
  {
    id: "ins-1",
    name: "平安保险",
    is_deleted: false,
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
  },
  {
    id: "ins-2",
    name: "太平洋保险",
    is_deleted: false,
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
  },
];

function basePolicy(over: Partial<Policy> = {}): Policy {
  return makePolicy({ id: "policy-1", ...over });
}

let policies: Policy[];
let policyStats: PolicyStats[];

function baseStats(policy: Policy): PolicyStats {
  return makePolicyStats({
    policy_id: policy.id,
    total_paid_native_cents: 600_000,
    total_inflow_native_cents: 50_000,
    next_charge_date: "2027-01-01",
    is_expired: policy.end_date !== null && policy.end_date < "2026-06-01",
  });
}

function setupInvoke() {
  wireInvokeSeam({
    overrides: {
      list_insurers: mockInsurers,
      list_policies: () => policies.filter((p) => !p.is_deleted),
      list_policy_stats: () =>
        policyStats.filter((s) => policies.some((p) => p.id === s.policy_id && !p.is_deleted)),
      create_policy: (args) => {
        const { input } = args as { input: { policy_number: string; insurer_id: string } };
        const id = `policy-new-${input.policy_number}`;
        policies = [
          ...policies,
          basePolicy({
            id,
            ...input,
            coverage_amount_cents: null,
            coverage_currency_code: null,
            end_date: null,
          }),
        ];
        return id;
      },
      create_insurer: (args) => {
        const { input } = args as { input: { name: string } };
        const id = `ins-new-${input.name}`;
        mockInsurers.push({
          id,
          name: input.name,
          is_deleted: false,
          updated_at: "",
          version: 1,
          device_id: "test",
        });
        return id;
      },
      create_merchant: () => Promise.reject(new Error("unexpected create_merchant")),
      update_policy: (args) => {
        const { id, input } = args as { id: string; input: Partial<Policy> };
        policies = policies.map((p) => (p.id === id ? { ...p, ...input } : p));
      },
      delete_policy: (args) => {
        const { id } = args as { id: string };
        policies = policies.map((p) => (p.id === id ? { ...p, is_deleted: true } : p));
      },
    },
  });
}

beforeEach(async () => {
  mockRoute.query = {};
  policies = [basePolicy()];
  policyStats = [baseStats(policies[0])];
  setupInvoke();
  if (Element.prototype.scrollIntoView) vi.mocked(Element.prototype.scrollIntoView).mockClear();
  await flushPromises();
});

beforeAll(() => {
  Element.prototype.scrollIntoView = vi.fn();
  vi.mocked(Element.prototype.scrollIntoView).mockClear();
});

type ViewWrapper = ReturnType<typeof mount>;

function saveButton(): DOMWrapper<HTMLElement> {
  return new DOMWrapper(bodyQuery('[data-testid="policy-save"]'));
}

function formInput(testid: string): DOMWrapper<HTMLInputElement> {
  const modal = bodyQuery('[data-testid="policy-form-modal"]')!;
  return new DOMWrapper(modal.querySelector(`[data-testid="${testid}"] input`));
}

function setDate(wrapper: ViewWrapper, testid: string, value: string | null) {
  componentVm(wrapper.findComponent(`[data-testid="${testid}"]`)).$emit(
    "update:formatted-value",
    value,
  );
}

describe("PoliciesView 保单列表（issue #360）", () => {
  it("渲染列表：保司名 / 险种 / 保单号 / 保障期间 / 状态 / 保额", async () => {
    const wrapper = mount(PoliciesView);
    await flushPromises();
    const text = wrapper.text();
    expect(text).toContain("平安保险");
    expect(text).toContain("重疾险");
    expect(text).toContain("P2026-001");
    expect(text).toContain("2024-01-01 ~ 2036-01-01");
    expect(text).toContain("保障中");
    expect(text).toContain(formatAmount(30_000_000, cny));
  });

  it("止日为空显示「长期」；止日已过显示「已到期」", async () => {
    policies = [
      basePolicy({ id: "p-1", end_date: null }),
      basePolicy({ id: "p-2", end_date: "2020-01-01" }),
    ];
    const wrapper = mount(PoliciesView);
    await flushPromises();
    const text = wrapper.text();
    expect(text).toContain("2024-01-01 ~ 长期");
    expect(text).toContain("已到期");
    expect(text).toContain("保障中");
  });

  it("空列表显示建档引导", async () => {
    policies = [];
    const wrapper = mount(PoliciesView);
    await flushPromises();
    expect(wrapper.find('[data-testid="policy-empty-guide"]').text()).toContain("暂无保单");
  });

  it("点击删除并确认：delete_policy 收到对应 id，列表移除（软删不进列表）", async () => {
    policies = [basePolicy(), basePolicy({ id: "p-2", policy_number: "P2026-002" })];
    const wrapper = mount(PoliciesView);
    await flushPromises();

    await wrapper.find('[data-testid="policy-delete-policy-1"]').trigger("click");
    await flushPromises();
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "delete_policy")).toBe(false);

    wrapper.findComponent(NPopconfirm).vm.$emit("positiveClick");
    await flushPromises();

    const deleteCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "delete_policy");
    expect(deleteCalls).toHaveLength(1);
    expect((deleteCalls[0][1] as { id: string }).id).toBe("policy-1");
    expect(wrapper.text()).not.toContain("P2026-001");
    expect(wrapper.text()).toContain("P2026-002");
  });
});

describe("PoliciesView 新建保单", () => {
  it("打开新建弹窗，完整填写并保存：create_policy 收到归一化入参，弹窗关闭", async () => {
    const wrapper = mount(PoliciesView);
    await flushPromises();
    await wrapper.find('[data-testid="policy-new"]').trigger("click");
    await flushPromises();
    expect(bodyQuery('[data-testid="policy-form-modal"]')).not.toBeNull();

    componentVm(wrapper.findComponent('[data-testid="policy-insurer"]')).$emit(
      "update:value",
      "ins-2",
    );
    await flushPromises();
    await formInput("policy-number").setValue("P2026-100");
    await formInput("policy-product").setValue("医疗险");
    setDate(wrapper, "policy-start-date", "2026-03-01");
    setDate(wrapper, "policy-end-date", "2031-03-01");
    await formInput("policy-coverage").setValue("100000");
    await formInput("policy-note").setValue("百万医疗");

    await saveButton().trigger("click");
    await flushPromises();

    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "create_policy");
    expect(call).toBeTruthy();
    expect(call![1]).toMatchObject({
      input: {
        insurer_id: "ins-2",
        policy_number: "P2026-100",
        product_name: "医疗险",
        start_date: "2026-03-01",
        end_date: "2031-03-01",
        coverage_amount_cents: 10_000_000,
        note: "百万医疗",
      },
    });
    expect(wrapper.findComponent(PolicyFormModal).emitted("update:show")).toContainEqual([false]);
  });

  it("保司输入新名称：create_insurer 即建后携带新 id（同库同名一致）", async () => {
    const wrapper = mount(PoliciesView);
    await flushPromises();
    await wrapper.find('[data-testid="policy-new"]').trigger("click");
    await flushPromises();

    componentVm(wrapper.findComponent('[data-testid="policy-insurer"]')).$emit(
      "update:value",
      "人保健康",
    );
    await flushPromises();
    await formInput("policy-number").setValue("P2026-200");
    await formInput("policy-product").setValue("意外险");
    setDate(wrapper, "policy-start-date", "2026-04-01");

    await saveButton().trigger("click");
    await flushPromises();

    const insurerCall = mockInvoke.mock.calls.find(([cmd]) => cmd === "create_insurer");
    expect(insurerCall).toBeTruthy();
    expect((insurerCall![1] as { input: { name: string } }).input.name).toBe("人保健康");
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "create_policy");
    expect(call![1]).toMatchObject({ input: { insurer_id: "ins-new-人保健康" } });
  });

  it("保司输入文本精确命中在用保司名：按名复用既有 id，不即建（全库同名一致）", async () => {
    const wrapper = mount(PoliciesView);
    await flushPromises();
    await wrapper.find('[data-testid="policy-new"]').trigger("click");
    await flushPromises();

    componentVm(wrapper.findComponent('[data-testid="policy-insurer"]')).$emit(
      "update:value",
      "平安保险",
    );
    await flushPromises();
    await formInput("policy-number").setValue("P2026-400");
    await formInput("policy-product").setValue("重疾险");
    setDate(wrapper, "policy-start-date", "2026-06-01");

    await saveButton().trigger("click");
    await flushPromises();

    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "create_insurer")).toBe(false);
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "create_policy");
    expect(call![1]).toMatchObject({ input: { insurer_id: "ins-1" } });
  });

  it("止日留空保存为 null（长期/终身）；保额缺省币种一并存空", async () => {
    const wrapper = mount(PoliciesView);
    await flushPromises();
    await wrapper.find('[data-testid="policy-new"]').trigger("click");
    await flushPromises();

    componentVm(wrapper.findComponent('[data-testid="policy-insurer"]')).$emit(
      "update:value",
      "mer-1",
    );
    await flushPromises();
    await formInput("policy-number").setValue("P2026-300");
    await formInput("policy-product").setValue("终身寿险");
    setDate(wrapper, "policy-start-date", "2026-05-01");

    await saveButton().trigger("click");
    await flushPromises();

    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "create_policy");
    expect(call![1]).toMatchObject({
      input: { end_date: null, coverage_amount_cents: null, coverage_currency_code: null },
    });
  });

  it("必填校验：保单号为空不发起 create_policy，弹窗不关", async () => {
    const wrapper = mount(PoliciesView);
    await flushPromises();
    await wrapper.find('[data-testid="policy-new"]').trigger("click");
    await flushPromises();

    componentVm(wrapper.findComponent('[data-testid="policy-insurer"]')).$emit(
      "update:value",
      "mer-1",
    );
    await flushPromises();

    await saveButton().trigger("click");
    await flushPromises();

    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === "create_policy")).toBe(false);
    expect(wrapper.findComponent(PolicyFormModal).emitted("update:show") ?? []).not.toContainEqual([
      false,
    ]);
  });
});

describe("PoliciesView 保单视角统计（issue #363）", () => {
  it("列表展示实时推导统计：累计已缴/累计流入/下期扣款日（本位币同源快照）", async () => {
    const wrapper = mount(PoliciesView);
    await flushPromises();
    const text = wrapper.text();
    expect(text).toContain("累计已缴");
    expect(text).toContain(formatAmount(600_000, cny));
    expect(text).toContain(formatAmount(50_000, cny));
    expect(text).toContain("2027-01-01");
  });

  it("无活跃协议（下期扣款日 null）显示占位，不显示日期", async () => {
    policyStats = [baseStats(policies[0]!)];
    policyStats[0] = { ...policyStats[0]!, next_charge_date: null };
    const wrapper = mount(PoliciesView);
    await flushPromises();
    expect(wrapper.text()).not.toContain("2027-01-01");
  });

  it("到期徽标消费统计同源推导（is_expired=true → 已到期，止日未到也生效）", async () => {
    policies = [basePolicy({ id: "p-1", end_date: "2999-01-01" })];
    policyStats = [makePolicyStats({ policy_id: "p-1", is_expired: true })];
    const wrapper = mount(PoliciesView);
    await flushPromises();
    expect(wrapper.text()).toContain("已到期");
  });

  it("统计行缺失（加载窗口）回落本地到期推导，统计列显示占位", async () => {
    policies = [basePolicy({ id: "p-1", end_date: "2020-01-01" })];
    policyStats = [];
    const wrapper = mount(PoliciesView);
    await flushPromises();
    expect(wrapper.text()).toContain("已到期");
    expect(wrapper.text()).toContain("—");
  });
});

describe("PoliciesView 编辑保单", () => {
  it("编辑弹窗（详情）展示保单视角统计摘要：已缴/流入/下期扣款（issue #363）", async () => {
    const wrapper = mount(PoliciesView);
    await flushPromises();

    await wrapper.find('[data-testid="policy-edit-policy-1"]').trigger("click");
    await flushPromises();
    const summary = bodyQuery('[data-testid="policy-stats-summary"]')!;
    expect(summary.textContent).toContain("累计已缴保费");
    expect(summary.textContent).toContain(formatAmount(600_000, cny));
    expect(summary.textContent).toContain(formatAmount(50_000, cny));
    expect(summary.textContent).toContain("2027-01-01");
    expect(summary.textContent).toContain("到期状态");
    expect(summary.textContent).toContain("保障中");
  });

  it("编辑弹窗到期态摘要：止日为空显示长期/终身（永不判到期）", async () => {
    policies = [basePolicy({ id: "p-1", end_date: null })];
    policyStats = [makePolicyStats({ policy_id: "p-1", is_expired: false })];
    const wrapper = mount(PoliciesView);
    await flushPromises();
    await wrapper.find('[data-testid="policy-edit-p-1"]').trigger("click");
    await flushPromises();
    const summary = bodyQuery('[data-testid="policy-stats-summary"]')!;
    expect(summary.textContent).toContain("长期/终身");
  });

  it("点编辑打开弹窗并预填当前行字段，保存走 update_policy 且携带 id 与保司", async () => {
    const wrapper = mount(PoliciesView);
    await flushPromises();

    await wrapper.find('[data-testid="policy-edit-policy-1"]').trigger("click");
    await flushPromises();
    expect(bodyQuery('[data-testid="policy-form-modal"]')).not.toBeNull();

    expect(formInput("policy-number").element.value).toBe("P2026-001");
    expect(formInput("policy-product").element.value).toBe("重疾险");
    expect(formInput("policy-coverage").element.value).toBe("300000");
    const pickers = wrapper
      .findComponent(PolicyFormModal)
      .findAllComponents('[data-testid="policy-start-date"], [data-testid="policy-end-date"]');
    expect(pickers.length).toBe(2);

    await formInput("policy-number").setValue("P-EDITED");
    await saveButton().trigger("click");
    await flushPromises();

    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "update_policy");
    expect(call).toBeTruthy();
    expect(call![1]).toMatchObject({
      id: "policy-1",
      input: { policy_number: "P-EDITED", insurer_id: "ins-1" },
    });
  });
});

describe("PoliciesView 来源跳转落点（issue #706）", () => {
  it("focus 在场：列表渲染后对应行获得高亮类，行锚点 data-policy-id 恒在", async () => {
    policies = [basePolicy({ id: "p-1" }), basePolicy({ id: "p-2", policy_number: "P2026-002" })];
    mockRoute.query = { focus: "p-2" };
    const wrapper = mount(PoliciesView);
    await flushPromises();

    const rows = wrapper.findAll("tr[data-policy-id]");
    expect(rows.map((r) => r.attributes("data-policy-id"))).toEqual(["p-1", "p-2"]);
    expect(rows[0].classes()).not.toContain("policy-row-focus");
    expect(rows[1].classes()).toContain("policy-row-focus");
    expect(Element.prototype.scrollIntoView).toHaveBeenCalled();
  });

  it("无 focus：全部行无高亮（安全空转）", async () => {
    const wrapper = mount(PoliciesView);
    await flushPromises();
    expect(wrapper.findAll("tr.policy-row-focus").length).toBe(0);
    expect(Element.prototype.scrollIntoView).not.toHaveBeenCalled();
  });

  it("读一次语义：消费后 query 变化（页签切换 replace 保留残留 focus 场景）不重新高亮", async () => {
    policies = [basePolicy({ id: "p-1" }), basePolicy({ id: "p-9", policy_number: "P2026-009" })];
    mockRoute.query = { focus: "p-1" };
    const wrapper = mount(PoliciesView);
    await flushPromises();

    mockRoute.query = { tab: "policies", focus: "p-9" };
    await flushPromises();
    const highlighted = wrapper.findAll("tr.policy-row-focus");
    expect(highlighted.length).toBe(1);
    expect(highlighted[0].attributes("data-policy-id")).toBe("p-1");
  });
});

describe("PoliciesView 移动档横向滚动下限（issue #849 / ADR-0088 决策 11 票⑨）", () => {
  it("窄屏表格挂 scroll-x = 固定列宽总和（横向滚动吸收窄屏）；桌面档不挂零变化", async () => {
    await assertMobileTierScrollX(() => mount(PoliciesView));
  });
});
