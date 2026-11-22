<script setup lang="ts">
import { errorMessage } from "@ledger/utils/errors";
import { h, computed, nextTick, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import {
  NCard,
  NButton,
  NDataTable,
  NSpace,
  NTag,
  useMessage,
  type DataTableColumns,
} from "naive-ui";
import { formatAmount } from "@ledger/money";
import { todayStr } from "@ledger/utils/date";
import { policyStatAmountText } from "@/policy/policy-stats";
import type { Policy, PolicyStats } from "@ledger/types";
import AppPopconfirm from "@ledger/ui-kit/AppPopconfirm.vue";
import PolicyFormModal from "@/policy/PolicyFormModal.vue";
import { useModalIntent } from "@ledger/modal-intent";
import { useFocusParam } from "@/composables/useFocusParam";
import { useWindowTier } from "@ledger/window-tier";
import { sumFixedColumnWidths } from "@ledger/utils/table";
import { useReferenceStore } from "@/stores/reference";
import { usePoliciesStore } from "@/policy/policies";
import { t } from "@ledger/i18n";

const reference = useReferenceStore();
const policiesStore = usePoliciesStore();
const message = useMessage();
const route = useRoute();

const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

type PolicyFormIntent = { mode: "create" } | { mode: "edit"; policy: Policy };

const {
  intent: formIntent,
  seq: formSeq,
  open: openFormIntent,
  close: closeForm,
} = useModalIntent<PolicyFormIntent>();

function openCreate() {
  openFormIntent({ mode: "create" });
}

function openEdit(row: Policy) {
  openFormIntent({ mode: "edit", policy: row });
}

async function removePolicy(id: string) {
  try {
    await policiesStore.remove(id);
    message.success(t("policies.msg.deleted"));
  } catch (e) {
    message.error(t("policies.msg.deleteFailed", { msg: errorMessage(e) }));
  }
}

function isExpired(row: Policy): boolean {
  return row.end_date !== null && row.end_date < todayStr();
}

function expiredState(row: Policy): boolean {
  return policiesStore.statsById.get(row.id)?.is_expired ?? isExpired(row);
}

function periodText(row: Policy): string {
  return row.end_date
    ? t("policies.period.range", { start: row.start_date, end: row.end_date })
    : t("policies.period.lifetime", { start: row.start_date });
}

function coverageText(row: Policy): string {
  if (row.coverage_amount_cents === null || row.coverage_currency_code === null) return "—";
  const currency = reference.getCurrency(row.coverage_currency_code);
  return formatAmount(row.coverage_amount_cents, currency);
}

function statsAmountText(row: Policy, pick: (s: PolicyStats) => number): string {
  return policyStatAmountText(policiesStore.statsById.get(row.id), pick);
}

const columns: DataTableColumns<Policy> = [
  {
    title: () => t("policies.columns.insurer"),
    key: "insurer_id",
    render: (row) => reference.insurerMap.get(row.insurer_id)?.name ?? "—",
  },
  { title: () => t("policies.columns.productName"), key: "product_name" },
  { title: () => t("policies.columns.policyNumber"), key: "policy_number" },
  { title: () => t("policies.columns.period"), key: "period", render: (row) => periodText(row) },
  {
    title: () => t("policies.columns.expiry"),
    key: "expiry",
    width: 90,
    render: (row) =>
      h(
        NTag,
        { size: "small", type: expiredState(row) ? "warning" : "success", bordered: false },
        () => (expiredState(row) ? t("policies.expiry.expired") : t("policies.expiry.active")),
      ),
  },
  {
    title: () => t("policies.columns.coverage"),
    key: "coverage_amount_cents",
    render: (row) => coverageText(row),
  },
  {
    title: () => t("policies.columns.paid"),
    key: "total_paid_native_cents",
    width: 110,
    render: (row) => statsAmountText(row, (s) => s.total_paid_native_cents),
  },
  {
    title: () => t("policies.columns.inflow"),
    key: "total_inflow_native_cents",
    width: 110,
    render: (row) => statsAmountText(row, (s) => s.total_inflow_native_cents),
  },
  {
    title: () => t("policies.columns.nextCharge"),
    key: "next_charge_date",
    width: 110,
    render: (row) => policiesStore.statsById.get(row.id)?.next_charge_date ?? "—",
  },
  {
    title: () => t("policies.columns.actions"),
    key: "actions",
    width: 140,
    render: (row) =>
      h(NSpace, { size: 4 }, () => [
        h(
          NButton,
          { size: "tiny", "data-testid": `policy-edit-${row.id}`, onClick: () => openEdit(row) },
          () => t("policies.rowActions.edit"),
        ),
        h(
          AppPopconfirm,
          { onPositiveClick: () => removePolicy(row.id) },
          {
            default: () => t("policies.deleteConfirm"),
            trigger: () =>
              h(
                NButton,
                {
                  size: "tiny",
                  type: "error",
                  quaternary: true,
                  "data-testid": `policy-delete-${row.id}`,
                },
                () => t("policies.rowActions.delete"),
              ),
          },
        ),
      ]),
  },
];

const tableScrollX = sumFixedColumnWidths(columns);

const highlightedPolicyId = ref<string | null>(null);

const tableRef = ref<{ $el?: HTMLElement } | null>(null);

const focusParam = useFocusParam({
  query: () => route.query,
  onFocus: (policyId) => {
    highlightedPolicyId.value = policyId;
  },
});

function rowProps(row: Policy): Record<string, unknown> {
  return {
    "data-policy-id": row.id,
    class: row.id === highlightedPolicyId.value ? "policy-row-focus" : undefined,
  };
}

async function scrollToHighlighted(): Promise<void> {
  if (!highlightedPolicyId.value) return;
  for (let hop = 0; hop < 5; hop += 1) {
    await nextTick();
    const root = tableRef.value?.$el as HTMLElement | undefined;
    const anchor = root
      ? Array.from(root.querySelectorAll<HTMLElement>("[data-policy-id]")).find(
          (el) => el.dataset.policyId === highlightedPolicyId.value,
        )
      : undefined;
    if (anchor) {
      anchor.scrollIntoView({ block: "center" });
      return;
    }
  }
}

const listTitle = computed(() => t("policies.listTitle"));

onMounted(() => {
  focusParam.consume();
  void policiesStore
    .refresh()
    .catch(() => {})
    .then(scrollToHighlighted);
});
</script>

<template>
  <div class="viewport-table-view viewport-gap-16">
    <NCard size="small">
      <NSpace justify="end">
        <NButton type="primary" data-testid="policy-new" @click="openCreate">
          {{ t("policies.newButton") }}
        </NButton>
      </NSpace>
    </NCard>

    <NCard :title="listTitle" size="small" class="viewport-table-card">
      <NDataTable
        ref="tableRef"
        :columns="columns"
        :data="policiesStore.policies"
        :row-props="rowProps"
        :bordered="false"
        size="small"
        :flex-height="!isMobileTier"
        :scroll-x="isMobileTier ? tableScrollX : undefined"
      >
        <template #empty>
          <span data-testid="policy-empty-guide">{{ t("policies.emptyGuide") }}</span>
        </template>
      </NDataTable>
    </NCard>

    <!-- 新建/编辑弹窗（同一表单组件双模式，遮罩点击不关：AppModal 默认语义，
         ADR-0035）。显示由「意图非空」派生（无独立 show 布尔），关闭（✕ /
         ESC / 取消 / 保存成功）统一经工厂清回 null 终态；序号作 key 强制重建
         （ADR-0072）。 -->
    <PolicyFormModal
      :key="formSeq"
      :show="formIntent !== null"
      :editing="formIntent?.mode === 'edit' ? formIntent.policy : null"
      @update:show="(v: boolean) => (v ? undefined : closeForm())"
    />
  </div>
</template>

<style>
tr.policy-row-focus > td {
  background-color: rgba(245, 158, 11, 0.16);
}
</style>
