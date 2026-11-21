<script setup lang="ts">
import { errorMessage } from "@ledger/utils/errors";
import { computed, h, onMounted, ref } from "vue";
import { t } from "@ledger/i18n";
import {
  NCard,
  NButton,
  NButtonGroup,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSpace,
  useMessage,
  type DataTableColumns,
} from "naive-ui";
import AppDatePicker from "@ledger/ui-kit/AppDatePicker.vue";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import AppTreeSelect from "@ledger/ui-kit/AppTreeSelect.vue";
import { formatAmount, yuanToCents } from "@ledger/money";
import type { ScheduledTransactionOccurrence } from "@ledger/types";
import { api } from "@ledger/api";
import { useReferenceStore } from "@/stores/reference";
import { useModalIntent } from "@ledger/modal-intent";
import { useWindowTier } from "@ledger/window-tier";
import { useScheduledPlanForm } from "@/scheduled/useScheduledPlanForm";
import {
  earliestPendingOccurrence,
  isUnfinishedStatus,
  scheduledRecurrenceLabel,
  scheduledRecurrenceOptions,
  useScheduledPlanList,
  type ScheduledPlanRow,
  type ScheduledPlanRowAction,
} from "@ledger/scheduled-plan-list";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import PinyinSelect from "@ledger/ui-kit/PinyinSelect.vue";
import PlanRowActions from "@/scheduled/PlanRowActions.vue";
import SubscriptionSpendPanel from "@/scheduled/SubscriptionSpendPanel.vue";
import PlanDetailModal from "@/scheduled/PlanDetailModal.vue";
import { MOBILE_CELL_STYLE, MOBILE_SUB_STYLE, mobileSubLine } from "@/components/mobile-cells";
import { usePlanFocusLanding } from "@/scheduled/usePlanFocusLanding";
import { scheduledStatusLabel } from "@ledger/utils/scheduled";

const reference = useReferenceStore();
const message = useMessage();

const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

const editForm = useScheduledPlanForm();
const {
  note: editNote,
  accountId: editAccountId,
  categoryId: editCategoryId,
  merchantRef: editMerchantRef,
  accountOptions: editAccountOptions,
} = editForm;

const spendPanelRef = ref<InstanceType<typeof SubscriptionSpendPanel> | null>(null);
function refreshSpend() {
  void spendPanelRef.value?.reload();
}

interface SubscriptionExt {
  next: ScheduledTransactionOccurrence | null;
}
type SubscriptionRow = ScheduledPlanRow<SubscriptionExt>;

const planDetailRef = ref<InstanceType<typeof PlanDetailModal> | null>(null);

const list = useScheduledPlanList<SubscriptionExt>({
  kind: "subscription",
  expandDetail: (_plan, detail) => ({
    next: detail ? earliestPendingOccurrence(detail) : null,
  }),
  cancelConfirmText: () => t("scheduled.pane.subscriptionCancelConfirm"),
  onStatusChanged: refreshSpend,
  onOpenDetail: (row) => void planDetailRef.value?.open(row.plan.core.id),
});
const { loading, statusFilter, statusFilterOptions, filteredRows } = list;

interface SubscriptionCreateIntent {
  type: "create";
}

const {
  intent: createIntent,
  open: openCreateIntent,
  close: closeCreateIntent,
} = useModalIntent<SubscriptionCreateIntent>();

const amountYuan = ref("");

const createForm = useScheduledPlanForm({
  onSubmitted: async () => {
    closeCreateIntent();
    amountYuan.value = "";
    await list.load();
    refreshSpend();
  },
});
const {
  note,
  accountId,
  categoryId,
  merchantRef,
  currencyCode,
  recurrenceType,
  recurrenceInterval,
  startDate,
  accountOptions,
  currencyOptions,
  categoryTreeOptions,
  merchantOptions,
} = createForm;

async function create() {
  if (!accountId.value) {
    message.warning(t("scheduled.form.selectAccount"));
    return;
  }
  const amountCents = yuanToCents(amountYuan.value);
  if (amountCents === null || amountCents <= 0) {
    message.warning(t("scheduled.form.amountPositive"));
    return;
  }
  await createForm.submitCreate({ kind: "subscription", amountCents });
}

interface SubscriptionEditIntent {
  row: SubscriptionRow;
}

const {
  intent: editIntent,
  seq: editSeq,
  open: openEditIntent,
  close: closeEdit,
} = useModalIntent<SubscriptionEditIntent>();

const editCurrentMerchantId = ref<string | null>(null);

const editMerchantOptions = computed<{ label: string; value: string }[]>(() => {
  const base = reference.merchants.map((m) => ({ label: m.name, value: m.id }));
  const current = editCurrentMerchantId.value;
  if (current && !reference.merchantMap.has(current)) {
    base.unshift({ label: t("scheduled.form.deletedMerchant"), value: current });
  }
  return base;
});

function openEdit(row: SubscriptionRow) {
  editNote.value = row.plan.core.note ?? "";
  editAccountId.value = row.plan.core.account_id;
  editCategoryId.value = row.plan.core.category_id;
  editCurrentMerchantId.value = row.plan.merchant_id;
  editMerchantRef.value = row.plan.merchant_id;
  openEditIntent({ row });
}

async function saveEdit() {
  const target = editIntent.value;
  if (!target) return;
  if (!editAccountId.value) {
    message.warning(t("scheduled.form.selectAccount"));
    return;
  }
  try {
    await api.updateScheduledSubscription({
      id: target.row.plan.core.id,
      account_id: editAccountId.value,
      category_id: editCategoryId.value,
      merchant_id: await editForm.resolveMerchant(editCurrentMerchantId.value),
      note: editNote.value.trim() || null,
    });
    message.success(t("scheduled.toast.saved"));
    closeEdit();
    await list.load();
    refreshSpend();
  } catch (e) {
    message.error(t("scheduled.toast.saveFailed", { message: errorMessage(e) }));
  }
}

async function onDetailChanged() {
  await list.load();
  refreshSpend();
}

function statusLabel(status: string): string {
  return scheduledStatusLabel(status);
}

function merchantName(row: SubscriptionRow): string {
  const m = row.plan.merchant_id ? reference.merchantMap.get(row.plan.merchant_id) : undefined;
  return m?.name ?? "—";
}

function accountName(row: SubscriptionRow): string {
  return reference.accountMap.get(row.plan.core.account_id)?.name ?? row.plan.core.account_id;
}

function subscriptionActions(row: SubscriptionRow): ScheduledPlanRowAction[] {
  const status = row.plan.core.status;
  const actions: ScheduledPlanRowAction[] = [];
  for (const action of list.rowActions(row)) {
    if (action.key === "detail") {
      actions.push(action, {
        key: "edit",
        label: t("scheduled.action.edit"),
        available: isUnfinishedStatus(status),
        confirm: null,
        run: () => openEdit(row),
      });
    } else {
      actions.push(action);
    }
  }
  return actions;
}

function nextChargeText(row: SubscriptionRow): string {
  const currency = reference.getCurrency(row.plan.core.currency_code);
  if (row.ext.next) {
    return `${row.ext.next.scheduled_date} · ${formatAmount(row.ext.next.amount_cents, currency)}`;
  }
  return row.detailFailed ? t("scheduled.list.loadFailed") : "—";
}

const recurrenceOptions = computed(scheduledRecurrenceOptions);

const columns = computed<DataTableColumns<SubscriptionRow>>(() => {
  if (isMobileTier.value) {
    return [
      {
        title: t("scheduled.column.note"),
        key: "note",
        render: (row) =>
          h("div", { style: MOBILE_CELL_STYLE }, [
            h("span", null, row.plan.core.note ?? "—"),
            h(
              "span",
              { style: MOBILE_SUB_STYLE },
              mobileSubLine(
                statusLabel(row.plan.core.status),
                scheduledRecurrenceLabel(
                  row.plan.core.recurrence_type,
                  row.plan.core.recurrence_interval,
                ),
                row.plan.core.start_date,
              ),
            ),
            h(
              "span",
              { style: MOBILE_SUB_STYLE },
              mobileSubLine(
                merchantName(row),
                reference.categoryPath(row.plan.core.category_id),
                accountName(row),
              ),
            ),
          ]),
      },
      {
        title: t("scheduled.column.amount"),
        key: "amount",
        render: (row) =>
          h("div", { style: MOBILE_CELL_STYLE }, [
            h(
              "span",
              null,
              formatAmount(
                row.plan.core.amount_cents,
                reference.getCurrency(row.plan.core.currency_code),
              ),
            ),
            h(
              "span",
              { style: MOBILE_SUB_STYLE, "data-testid": `next-charge-${row.plan.core.id}` },
              nextChargeText(row),
            ),
          ]),
      },
      {
        title: t("scheduled.column.actions"),
        key: "actions",
        render: (row) =>
          h(PlanRowActions, {
            actions: subscriptionActions(row),
            rowId: row.plan.core.id,
            mobile: true,
          }),
      },
    ];
  }
  return [
    {
      title: t("scheduled.column.note"),
      key: "note",
      render: (row) => row.plan.core.note ?? "—",
    },
    {
      title: t("scheduled.column.merchant"),
      key: "merchant",
      render: (row) => merchantName(row),
    },
    {
      title: t("scheduled.column.category"),
      key: "category",
      render: (row) => reference.categoryPath(row.plan.core.category_id) || "—",
    },
    {
      title: t("scheduled.column.account"),
      key: "account",
      render: (row) => accountName(row),
    },
    {
      title: t("scheduled.column.amount"),
      key: "amount",
      render: (row) =>
        formatAmount(
          row.plan.core.amount_cents,
          reference.getCurrency(row.plan.core.currency_code),
        ),
    },
    {
      title: t("scheduled.column.recurrence"),
      key: "recurrence",
      render: (row) =>
        scheduledRecurrenceLabel(row.plan.core.recurrence_type, row.plan.core.recurrence_interval),
    },
    {
      title: t("scheduled.column.startDate"),
      key: "start_date",
      render: (row) => row.plan.core.start_date,
    },
    {
      title: t("scheduled.column.status"),
      key: "status",
      render: (row) => statusLabel(row.plan.core.status),
    },
    {
      title: t("scheduled.column.nextCharge"),
      key: "next",
      render: (row) =>
        h("span", { "data-testid": `next-charge-${row.plan.core.id}` }, nextChargeText(row)),
    },
    {
      title: t("scheduled.column.actions"),
      key: "actions",
      render: (row) =>
        h(PlanRowActions, { actions: subscriptionActions(row), rowId: row.plan.core.id }),
    },
  ];
});

const props = defineProps<{ focusPlanId?: string | null }>();

const emit = defineEmits<{ (e: "focusConsumed"): void }>();

usePlanFocusLanding({
  focusPlanId: () => props.focusPlanId,
  openDetail: (id) => void planDetailRef.value?.open(id),
  onConsumed: () => emit("focusConsumed"),
});

onMounted(() => {
  void list.load();
});
</script>

<template>
  <NSpace vertical :size="16">
    <NCard :title="t('scheduled.pane.subscriptionList')" size="small">
      <template #header-extra>
        <NSpace :size="12">
          <NButtonGroup size="small">
            <NButton
              v-for="f in statusFilterOptions"
              :key="f.key"
              :type="statusFilter === f.key ? 'primary' : 'default'"
              :data-testid="`filter-${f.key}`"
              @click="list.setStatusFilter(f.key)"
            >
              {{ f.label }}
            </NButton>
          </NButtonGroup>
          <NButton
            type="primary"
            size="small"
            data-testid="sub-create-open"
            @click="openCreateIntent({ type: 'create' })"
          >
            {{ t("scheduled.pane.createSubscription") }}
          </NButton>
        </NSpace>
      </template>
      <NDataTable
        :columns="columns"
        :data="filteredRows"
        :loading="loading"
        :bordered="false"
        size="small"
        :row-key="(row: SubscriptionRow) => row.plan.core.id"
      />
    </NCard>

    <!-- 新建订阅弹窗：提交成功关闭并刷新列表（与记一笔弹窗同模式）；
         显示由「意图非空」派生（无独立 show 布尔），关闭统一经工厂清回 null 终态 -->
    <AppModal
      :show="createIntent !== null"
      @update:show="(v: boolean) => (v ? undefined : closeCreateIntent())"
      :title="t('scheduled.pane.createSubscription')"
      preset="card"
      display-directive="if"
      card-size="md"
    >
      <!-- 布局对齐记一笔表单（CategoryForm）：NSpace 提供行距，宽度取 160-280 档；
           金额+币种、周期+间隔 各并一行减少行数 -->
      <NForm label-placement="left" :show-feedback="false" size="small">
        <NSpace vertical :size="12">
          <NFormItem :label="t('scheduled.form.note')">
            <NInput
              v-model:value="note"
              data-testid="sub-note"
              :placeholder="t('scheduled.form.subscriptionNotePlaceholder')"
              style="width: 280px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.account')">
            <PinyinSelect
              v-model:value="accountId"
              :options="accountOptions"
              :placeholder="t('scheduled.form.accountPlaceholder')"
              style="width: 200px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.category')">
            <AppTreeSelect
              v-model:value="categoryId"
              :options="categoryTreeOptions"
              :placeholder="t('scheduled.form.categoryPlaceholder')"
              filterable
              clearable
              :consistent-menu-width="false"
              style="width: 220px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.merchant')">
            <PinyinSelect
              v-model:value="merchantRef"
              :options="merchantOptions"
              tag
              clearable
              :placeholder="t('scheduled.form.merchantPlaceholder')"
              style="width: 220px"
              data-testid="sub-merchant"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.amount')">
            <NInput
              v-model:value="amountYuan"
              data-testid="sub-amount"
              :placeholder="t('scheduled.form.amountPerPeriodPlaceholder')"
              style="width: 160px"
            />
            <AppSelect
              :value="currencyCode"
              :disabled="true"
              :options="currencyOptions"
              style="width: 130px; margin-left: 8px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.recurrence')">
            <NSpace :size="8" align="center" :wrap="false">
              <span>{{ t("scheduled.form.every") }}</span>
              <NInputNumber
                v-model:value="recurrenceInterval"
                :min="1"
                :precision="0"
                style="width: 90px"
              />
              <AppSelect
                v-model:value="recurrenceType"
                :options="recurrenceOptions"
                style="width: 100px"
              />
            </NSpace>
          </NFormItem>
          <NFormItem :label="t('scheduled.form.startDate')">
            <AppDatePicker
              v-model:formatted-value="startDate"
              type="date"
              value-format="yyyy-MM-dd"
              style="width: 200px"
            />
          </NFormItem>
          <NSpace justify="end">
            <NButton data-testid="sub-create-cancel" @click="closeCreateIntent">{{
              t("scheduled.form.cancel")
            }}</NButton>
            <NButton type="primary" data-testid="sub-create" @click="create">{{
              t("scheduled.pane.createSubscriptionSubmit")
            }}</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 编辑订阅弹窗（issue #162）：仅非金额字段（备注/账户/分类），无金额输入。
         显示由「意图非空」派生（无独立 show 布尔），关闭统一经工厂清回 null 终态；
         序号作 key 强制重建（ADR-0072）。 -->
    <AppModal
      :show="editIntent !== null"
      :title="t('scheduled.pane.editSubscription')"
      preset="card"
      display-directive="if"
      card-size="md"
      @update:show="(v: boolean) => (v ? undefined : closeEdit())"
    >
      <NForm
        v-if="editIntent"
        :key="editSeq"
        label-placement="left"
        :show-feedback="false"
        size="small"
      >
        <NSpace vertical :size="12">
          <NFormItem :label="t('scheduled.form.note')">
            <NInput
              v-model:value="editNote"
              data-testid="sub-edit-note"
              :placeholder="t('scheduled.form.subscriptionNoteEditPlaceholder')"
              style="width: 280px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.account')">
            <PinyinSelect
              v-model:value="editAccountId"
              :options="editAccountOptions"
              :placeholder="t('scheduled.form.accountPlaceholder')"
              style="width: 200px"
            />
          </NFormItem>
          <NFormItem :label="t('scheduled.form.category')">
            <AppTreeSelect
              v-model:value="editCategoryId"
              :options="categoryTreeOptions"
              :placeholder="t('scheduled.form.categoryPlaceholder')"
              filterable
              clearable
              :consistent-menu-width="false"
              style="width: 220px"
            />
          </NFormItem>
          <!-- 挂保单的缴费协议不显示商户（issue #713 / ADR-0082 决策 2）：付款对象
               语义由保单的保司承担，计划行不挂商户（后端对非空提交显式拒绝） -->
          <NFormItem v-if="!editIntent?.row.plan.policy_id" :label="t('scheduled.form.merchant')">
            <PinyinSelect
              v-model:value="editMerchantRef"
              :options="editMerchantOptions"
              tag
              clearable
              :placeholder="t('scheduled.form.merchantPlaceholder')"
              style="width: 220px"
              data-testid="sub-edit-merchant"
            />
          </NFormItem>
          <NSpace justify="end">
            <NButton data-testid="sub-edit-cancel" @click="closeEdit">{{
              t("scheduled.form.cancel")
            }}</NButton>
            <NButton type="primary" data-testid="sub-edit-save" @click="saveEdit">{{
              t("scheduled.form.save")
            }}</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <SubscriptionSpendPanel ref="spendPanelRef" />

    <!-- 期次详情弹窗（issue #205）：三页签通用，订阅页签同享 -->
    <PlanDetailModal ref="planDetailRef" @changed="onDetailChanged" />
  </NSpace>
</template>
