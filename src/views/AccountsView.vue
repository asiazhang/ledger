<script setup lang="ts">
import { errorMessage } from "@ledger/utils/errors";
import { centsToYuan, formatAmount, yuanToCents } from "@ledger/money";
import { computed, h, onMounted, ref } from "vue";
import type { VNode } from "vue";
import {
  NCard,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NIcon,
  NInput,
  NInputNumber,
  NSpace,
  NText,
  useMessage,
  useThemeVars,
  type DataTableColumns,
} from "naive-ui";
import { ChevronDown } from "@vicons/ionicons5";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { useReferenceStore } from "@/stores/reference";
import { useSavingsGoalsStore } from "@/savings-goal/savingsGoals";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import AppDropdown from "@ledger/ui-kit/AppDropdown.vue";
import AppDatePicker from "@ledger/ui-kit/AppDatePicker.vue";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import { useAppDialog } from "@/composables/useAppDialog";
import { useLoadable } from "@ledger/loadable";
import { useModalIntent } from "@ledger/modal-intent";
import { useRowContextMenu } from "@ledger/row-context-menu";
import { useWindowTier } from "@ledger/window-tier";
import AccountLink from "@/accounts/AccountLink.vue";
import {
  buildAccountRowMenuOptions,
  buildArchivedAccountRowMenuOptions,
} from "@/accounts/account-row-menu";
import {
  CREDIT_UTILIZATION_WARNING_PERCENT,
  creditProfile,
  listUtilizationPercent,
} from "@/accounts/credit-card";
import { ACCOUNT_TYPES } from "@ledger/types";
import type { AccountBalance, AccountInput, AccountType, AccountUpdateInput } from "@ledger/types";
import { countUnfinishedPlansUsingAccount } from "@/scheduled/plan-usage";

const reference = useReferenceStore();
const savingsGoalsStore = useSavingsGoalsStore();
const message = useMessage();
const dialog = useAppDialog();
const themeVars = useThemeVars();
const balances = ref<AccountBalance[]>([]);

const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

const name = ref("");
const type = ref<AccountType>("cash");
const currencyCode = ref("CNY");
const initial = ref<number | null>(0);

const creditLimit = ref<number | null>(null);
const statementDay = ref<number | null>(null);
const dueDay = ref<number | null>(null);
const isCreditType = computed(() => type.value === "credit");

const typeOptions = computed(() =>
  ACCOUNT_TYPES.map((k) => ({
    label: t(`accounts.type.${k}`),
    value: k,
  })),
);
const currencyOptions = () =>
  reference.currencies.map((c) => ({ label: `${c.name} (${c.code})`, value: c.code }));

const archivedBalances = computed(() => balances.value.filter((row) => row.account.is_archived));
const goalBalances = computed(() =>
  balances.value.filter(
    (row) => !row.account.is_archived && savingsGoalsStore.goalAccountIds.has(row.account.id),
  ),
);
const normalBalances = computed(() =>
  balances.value.filter(
    (row) => !row.account.is_archived && !savingsGoalsStore.goalAccountIds.has(row.account.id),
  ),
);

const archivedExpanded = ref(false);

const archivedChevronStyle = computed(() => ({
  transition: "transform 0.2s",
  transform: archivedExpanded.value ? "rotate(0deg)" : "rotate(-90deg)",
}));

const archivedSummary = computed(() => {
  const groups = new Map<string, { count: number; totalCents: number }>();
  for (const row of archivedBalances.value) {
    const group = groups.get(row.account.currency_code) ?? { count: 0, totalCents: 0 };
    group.count += 1;
    group.totalCents += row.balance_cents;
    groups.set(row.account.currency_code, group);
  }
  return [...groups]
    .map(([code, { count, totalCents }]) =>
      t("accounts.archived.summary", {
        count,
        total: formatAmount(totalCents, reference.getCurrency(code)),
      }),
    )
    .join(t("accounts.archived.summarySeparator"));
});

function accountTypeLabel(row: AccountBalance): string {
  return savingsGoalsStore.goalAccountIds.has(row.account.id)
    ? t("savingsGoals.accounts.typeLabel")
    : t(`accounts.type.${row.account.type}`);
}

async function refresh() {
  balances.value = await api.listAccountBalances();
}

async function create() {
  if (!name.value.trim()) {
    message.warning(t("accounts.message.nameRequired"));
    return;
  }
  const input: AccountInput = {
    name: name.value,
    type: type.value,
    currency_code: currencyCode.value,
    initial_balance_cents: yuanToCents(initial.value ?? 0) ?? 0,
    ...(type.value === "credit"
      ? {
          credit_limit_cents:
            creditLimit.value === null ? undefined : (yuanToCents(creditLimit.value) ?? undefined),
          statement_day: statementDay.value ?? undefined,
          due_day: dueDay.value ?? undefined,
        }
      : {}),
  };
  try {
    await api.createAccount(input);
    message.success(t("accounts.message.created"));
    name.value = "";
    initial.value = 0;
    creditLimit.value = null;
    statementDay.value = null;
    dueDay.value = null;
    await refresh();
  } catch (e) {
    message.error(t("accounts.message.createFailed", { message: errorMessage(e) }));
  }
}

async function remove(id: string) {
  try {
    await api.deleteAccount(id);
    message.success(t("accounts.message.deleted"));
    await refresh();
  } catch (e) {
    message.error(t("accounts.message.deleteFailed", { message: errorMessage(e) }));
  }
}

function confirmDelete(row: AccountBalance) {
  dialog.warning({
    title: t("accounts.deleteDialog.title"),
    content: t("accounts.deleteDialog.content", { name: row.account.name }),
    positiveText: t("accounts.deleteDialog.positive"),
    negativeText: t("accounts.deleteDialog.negative"),
    maskClosable: false,
    onPositiveClick: () => remove(row.account.id),
  });
}

const ARCHIVE_DIALOG_LINES_STYLE =
  "display: flex; flex-direction: column; gap: 6px; text-align: start;";

function archiveDialogContent(row: AccountBalance, unfinishedPlanCount: number): string | VNode {
  const lines: string[] = [t("accounts.archiveDialog.content", { name: row.account.name })];
  if (row.balance_cents !== 0) {
    lines.push(
      t("accounts.archiveDialog.balance", {
        amount: formatAmount(row.balance_cents, reference.getCurrency(row.account.currency_code)),
      }),
    );
  }
  if (unfinishedPlanCount > 0) {
    lines.push(t("accounts.archiveDialog.plans", { count: unfinishedPlanCount }));
  }
  if (lines.length === 1) return lines[0];
  return h(
    "div",
    { style: ARCHIVE_DIALOG_LINES_STYLE },
    lines.map((line) => h("div", null, line)),
  );
}

async function unfinishedPlanCountFor(accountId: string): Promise<number> {
  try {
    return countUnfinishedPlansUsingAccount(await api.listScheduledTransactions(), accountId);
  } catch {
    return 0;
  }
}

const archiveTarget = ref<AccountBalance | null>(null);

const archiveRun = useLoadable(async () => {
  const row = archiveTarget.value;
  if (!row) return false;
  await api.archiveAccount(row.account.id);
  return true;
});

const restoreRun = useLoadable(async () => {
  const row = archiveTarget.value;
  if (!row) return false;
  await api.unarchiveAccount(row.account.id);
  return true;
});

async function runArchiveChange(
  row: AccountBalance,
  run: { run(): Promise<boolean | null> },
  successMessage: string,
) {
  archiveTarget.value = row;
  if ((await run.run()) !== true) return;
  message.success(successMessage);
  await refresh();
}

async function archive(row: AccountBalance) {
  await runArchiveChange(row, archiveRun, t("accounts.message.archived"));
}

async function confirmArchive(row: AccountBalance) {
  const unfinishedPlanCount = await unfinishedPlanCountFor(row.account.id);
  dialog.warning({
    title: t("accounts.archiveDialog.title"),
    content: () => archiveDialogContent(row, unfinishedPlanCount),
    positiveText: t("accounts.archiveDialog.positive"),
    negativeText: t("accounts.archiveDialog.negative"),
    maskClosable: false,
    onPositiveClick: () => archive(row),
  });
}

async function restore(row: AccountBalance) {
  await runArchiveChange(row, restoreRun, t("accounts.message.restored"));
}

interface AccountEditIntent {
  row: AccountBalance;
}

const {
  intent: editIntent,
  seq: editSeq,
  open: openEditIntent,
  close: closeEdit,
} = useModalIntent<AccountEditIntent>();

const editName = ref("");
const editCurrency = ref("");
const editCreditLimit = ref<number | null>(null);
const editStatementDay = ref<number | null>(null);
const editDueDay = ref<number | null>(null);
const isCreditEdit = computed(() => editIntent.value?.row.account.type === "credit");
const isGoalBoundEdit = computed(
  () =>
    editIntent.value !== null &&
    savingsGoalsStore.goalAccountIds.has(editIntent.value.row.account.id),
);

function openEdit(row: AccountBalance) {
  editName.value = row.account.name;
  editCurrency.value = row.account.currency_code;
  editCreditLimit.value =
    row.account.credit_limit_cents == null
      ? null
      : centsToYuan(
          row.account.credit_limit_cents,
          reference.getCurrency(row.account.currency_code),
        );
  editStatementDay.value = row.account.statement_day ?? null;
  editDueDay.value = row.account.due_day ?? null;
  openEditIntent({ row });
}

async function submitEdit() {
  if (!editIntent.value) return;
  if (!editName.value.trim()) {
    message.warning(t("accounts.message.nameRequired"));
    return;
  }
  try {
    const payload: AccountUpdateInput = {
      currency_code: editCurrency.value,
    };
    if (!isGoalBoundEdit.value) {
      payload.name = editName.value;
    }
    if (editIntent.value.row.account.type === "credit") {
      payload.credit_limit_cents =
        editCreditLimit.value === null ? null : yuanToCents(editCreditLimit.value);
      payload.statement_day = editStatementDay.value;
      payload.due_day = editDueDay.value;
    }
    await api.updateAccount(editIntent.value.row.account.id, payload);
    message.success(t("accounts.message.saved"));
    closeEdit();
    await refresh();
  } catch (e) {
    message.error(t("accounts.message.saveFailed", { message: errorMessage(e) }));
  }
}

const editCredit = computed(() =>
  editIntent.value === null
    ? null
    : creditProfile(editIntent.value.row.account, editIntent.value.row.balance_cents),
);

const editCurrencyObj = computed(() =>
  editIntent.value === null
    ? undefined
    : reference.getCurrency(editIntent.value.row.account.currency_code),
);

const editUsageText = computed(() => {
  const profile = editCredit.value;
  if (profile === null) return "";
  const { usedCents, availableCents, utilizationPercent } = profile;
  if (availableCents === null || utilizationPercent === null) {
    return t("accounts.credit.usageUnset");
  }
  return t("accounts.credit.usageSummary", {
    used: formatAmount(usedCents, editCurrencyObj.value),
    available: formatAmount(availableCents, editCurrencyObj.value),
    percent: utilizationPercent,
  });
});

const editNextNodesText = computed(() => {
  const profile = editCredit.value;
  if (profile === null) return "";
  if (profile.nextStatementDate === null || profile.nextDueDate === null) {
    return t("accounts.credit.nextNodesUnset");
  }
  return t("accounts.credit.nextNodes", {
    statement: profile.nextStatementDate,
    due: profile.nextDueDate,
  });
});

interface AccountAdjustIntent {
  row: AccountBalance;
}

const {
  intent: adjustIntent,
  seq: adjustSeq,
  open: openAdjustIntent,
  close: closeAdjust,
} = useModalIntent<AccountAdjustIntent>();

const adjustTarget = ref<number | null>(null);
const adjustDate = ref<number | null>(Date.now());

function openAdjust(row: AccountBalance) {
  adjustTarget.value = null;
  adjustDate.value = Date.now();
  openAdjustIntent({ row });
}

function todayIso(): string {
  return formatLocalDate(new Date());
}

function formatLocalDate(d: Date): string {
  const m = `${d.getMonth() + 1}`.padStart(2, "0");
  const day = `${d.getDate()}`.padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

const adjustTargetCents = computed(() =>
  adjustTarget.value === null ? null : yuanToCents(adjustTarget.value),
);

const adjustDelta = computed(() => {
  if (adjustIntent.value === null || adjustTargetCents.value === null) return null;
  return adjustTargetCents.value - adjustIntent.value.row.balance_cents;
});

const adjustCurrency = computed(() =>
  adjustIntent.value
    ? reference.getCurrency(adjustIntent.value.row.account.currency_code)
    : undefined,
);

const adjustDeltaText = computed(() => {
  if (adjustDelta.value === null || adjustDelta.value === 0) return "";
  const abs = formatAmount(Math.abs(adjustDelta.value), adjustCurrency.value);
  return adjustDelta.value > 0
    ? t("accounts.adjust.deltaIn", { amount: abs })
    : t("accounts.adjust.deltaOut", { amount: abs });
});

async function submitAdjust() {
  if (!adjustIntent.value) return;
  if (adjustTargetCents.value === null || adjustDelta.value === 0) return;
  try {
    await api.adjustAccountBalance(adjustIntent.value.row.account.id, {
      target_balance_cents: adjustTargetCents.value,
      date: adjustDate.value ? formatLocalDate(new Date(adjustDate.value)) : todayIso(),
    });
    message.success(t("accounts.message.adjusted"));
    closeAdjust();
    await refresh();
  } catch (e) {
    message.error(t("accounts.message.adjustFailed", { message: errorMessage(e) }));
  }
}

const rowMenu = useRowContextMenu<AccountBalance>((key, row) => {
  if (key === "edit") openEdit(row);
  else if (key === "adjust-balance") openAdjust(row);
  else if (key === "archive") void confirmArchive(row);
  else if (key === "restore") void restore(row);
  else if (key === "delete") confirmDelete(row);
});

const menuOptions = computed(() => {
  const errorColor = themeVars.value.errorColor;
  return rowMenu.state.value?.row.account.is_archived
    ? buildArchivedAccountRowMenuOptions()
    : buildAccountRowMenuOptions({ errorColor });
});

const menuShow = computed(() => rowMenu.state.value !== null);
const menuX = computed(() => rowMenu.position.value.x);
const menuY = computed(() => rowMenu.position.value.y);

const rowProps = (row: AccountBalance) => ({
  onContextmenu: (e: MouseEvent) => rowMenu.open(e, row),
});

const renderBalanceCell = (row: AccountBalance) => {
  const text = formatAmount(row.balance_cents, reference.getCurrency(row.account.currency_code));
  const percent = listUtilizationPercent(row.account, row.balance_cents);
  if (percent === null) return text;
  const warning = percent >= CREDIT_UTILIZATION_WARNING_PERCENT;
  return h("div", { style: CREDIT_CELL_STYLE }, [
    h("div", {}, text),
    h(
      "div",
      {
        style: warning
          ? `${CREDIT_SUB_STYLE}; color: ${themeVars.value.warningColor}`
          : CREDIT_SUB_STYLE,
      },
      t("accounts.credit.usedPercent", { percent }),
    ),
  ]);
};

const MOBILE_NAME_CELL_STYLE = "display: flex; flex-direction: column; gap: 2px; min-width: 0;";
const MOBILE_NAME_SUB_STYLE = "font-size: 12px; opacity: 0.65;";
const CREDIT_CELL_STYLE = "display: flex; flex-direction: column; gap: 2px;";
const CREDIT_SUB_STYLE = "font-size: 12px; opacity: 0.65;";
const MOBILE_NAME_LINK_STYLE = "white-space: normal; text-align: left;";
const MOBILE_MORE_BUTTON_STYLE = { width: "48px", height: "48px", fontSize: "18px" };

const columns = computed<DataTableColumns<AccountBalance>>(() => {
  const actionsColumn: DataTableColumns<AccountBalance>[number] = {
    title: t("accounts.list.colActions"),
    key: "actions",
    width: 64,
    render: (row) =>
      h(
        NButton,
        {
          size: "tiny",
          quaternary: true,
          "aria-label": t("accounts.list.moreActions"),
          style: isMobileTier.value ? MOBILE_MORE_BUTTON_STYLE : undefined,
          onClick: (e: MouseEvent) => rowMenu.open(e, row),
        },
        () => "⋯",
      ),
  };

  if (isMobileTier.value) {
    return [
      {
        title: t("accounts.list.colName"),
        key: "account.name",
        render: (row) =>
          h("div", { style: MOBILE_NAME_CELL_STYLE }, [
            h(AccountLink, {
              accountId: row.account.id,
              style: MOBILE_NAME_LINK_STYLE,
            }),
            h(
              "div",
              { style: MOBILE_NAME_SUB_STYLE },
              `${accountTypeLabel(row)} · ${row.account.currency_code}`,
            ),
          ]),
      },
      {
        title: t("accounts.list.colBalance"),
        key: "balance_cents",
        width: 110,
        render: renderBalanceCell,
      },
      actionsColumn,
    ];
  }

  return [
    {
      title: t("accounts.list.colName"),
      key: "account.name",
      render: (row) => h(AccountLink, { accountId: row.account.id }),
    },
    {
      title: t("accounts.list.colType"),
      key: "account.type",
      render: accountTypeLabel,
    },
    { title: t("accounts.list.colCurrency"), key: "account.currency_code" },
    {
      title: t("accounts.list.colBalance"),
      key: "balance_cents",
      render: renderBalanceCell,
    },
    actionsColumn,
  ];
});

onMounted(() => {
  void refresh();
});
</script>

<template>
  <NSpace vertical :size="16">
    <!-- 新增账户表单按窗口分级分档（issue #847）：桌面档行内横排（既有布局一字
         不动）；移动档纵排堆叠（标签上置 + 控件满宽，内联横排 ≈700px 在 360dp
         屏必横向溢出），行距取 ADR-0079 决策 4 的 12px 节奏值（show-feedback 关
         闭后表单项零间距，容器 gap 单点补齐；页面级表单不在弹窗节奏守门范围）。
         「添加」主操作热区扩至 ≥48px（视觉不变）。 -->
    <NCard :title="t('accounts.create.title')" size="small">
      <NForm
        :inline="!isMobileTier"
        :label-placement="isMobileTier ? 'top' : 'left'"
        :show-feedback="false"
        size="small"
        :style="
          isMobileTier
            ? { display: 'flex', flexDirection: 'column', gap: '12px' }
            : isCreditType
              ? { flexWrap: 'wrap' }
              : undefined
        "
      >
        <NFormItem :label="t('accounts.create.name')">
          <NInput
            v-model:value="name"
            :placeholder="t('accounts.create.namePlaceholder')"
            :style="isMobileTier ? { width: '100%' } : { width: '160px' }"
          />
        </NFormItem>
        <NFormItem :label="t('accounts.create.type')">
          <AppSelect
            v-model:value="type"
            :options="typeOptions"
            :style="isMobileTier ? { width: '100%' } : { width: '120px' }"
          />
        </NFormItem>
        <NFormItem :label="t('accounts.create.currency')">
          <AppSelect
            v-model:value="currencyCode"
            :options="currencyOptions()"
            :style="isMobileTier ? { width: '100%' } : { width: '140px' }"
          />
        </NFormItem>
        <NFormItem :label="t('accounts.create.initialBalance')">
          <NInputNumber
            v-model:value="initial"
            :precision="2"
            :placeholder="
              isCreditType ? t('accounts.credit.initialBalanceCreditPlaceholder') : undefined
            "
            :style="isMobileTier ? { width: '100%' } : { width: '140px' }"
          />
        </NFormItem>
        <!-- 信用卡档案字段（spec #1327 / ADR-0119）：仅类型选信用卡时出现；桌面档
             允许换行（三个字段入场后内联行超宽），非信用卡时布局一字不动。 -->
        <NFormItem v-if="isCreditType" :label="t('accounts.credit.limit')">
          <NInputNumber
            v-model:value="creditLimit"
            :precision="2"
            :placeholder="t('accounts.credit.limitPlaceholder')"
            :style="isMobileTier ? { width: '100%' } : { width: '140px' }"
          />
        </NFormItem>
        <NFormItem v-if="isCreditType" :label="t('accounts.credit.statementDay')">
          <NInputNumber
            v-model:value="statementDay"
            :precision="0"
            :placeholder="t('accounts.credit.dayPlaceholder')"
            :style="isMobileTier ? { width: '100%' } : { width: '100px' }"
          />
        </NFormItem>
        <NFormItem v-if="isCreditType" :label="t('accounts.credit.dueDay')">
          <NInputNumber
            v-model:value="dueDay"
            :precision="0"
            :placeholder="t('accounts.credit.dayPlaceholder')"
            :style="isMobileTier ? { width: '100%' } : { width: '100px' }"
          />
        </NFormItem>
        <NButton
          type="primary"
          :class="isMobileTier ? 'touch-hit-area' : undefined"
          :style="isMobileTier ? { '--touch-hit-inset': '-10px -14px' } : undefined"
          @click="create"
        >
          {{ t("accounts.create.add") }}
        </NButton>
      </NForm>
    </NCard>

    <!-- 储蓄目标分组卡（issue #1755 / ADR-0133 决策 2）：目标账户按绑定派生归入独立
         分组，行结构与普通列表同源同构（同 columns / row-props，操作与右键菜单全保留）；
         无绑定时目标组为空集，v-if 不渲染空卡。分组卡列于账户列表之前（目标池是
         用户刻意划出的资金，置顶可见）。 -->
    <NCard
      v-if="goalBalances.length > 0"
      :title="t('savingsGoals.accounts.groupTitle')"
      size="small"
    >
      <NDataTable
        :columns="columns"
        :data="goalBalances"
        :bordered="false"
        size="small"
        :row-props="rowProps"
      />
    </NCard>

    <NCard :title="t('accounts.list.title')" size="small">
      <NDataTable
        :columns="columns"
        :data="normalBalances"
        :bordered="false"
        size="small"
        :row-props="rowProps"
      />
    </NCard>

    <!-- 已归档分区（spec #1952 / ADR-0142 决策 1）：页底、默认折叠、标题行带数量与
         余额合计（与主表同一份余额快照派生，不重算、不加不减）；展开后才渲染表格
         （折叠即不挂载行，不靠样式隐藏）。行只提供「恢复」——编辑 / 调整余额 / 删除
         都要先恢复。空集不渲染空卡（v-if）。 -->
    <NCard v-if="archivedBalances.length > 0" size="small" data-testid="accounts-archived-card">
      <template #header>
        <NButton
          quaternary
          size="small"
          data-testid="accounts-archived-toggle"
          :aria-expanded="archivedExpanded"
          :aria-label="
            archivedExpanded ? t('accounts.archived.collapse') : t('accounts.archived.expand')
          "
          @click="archivedExpanded = !archivedExpanded"
        >
          <NIcon :style="archivedChevronStyle"><ChevronDown /></NIcon>
          <span style="margin-left: 4px">{{ t("accounts.archived.title") }}</span>
          <span
            data-testid="accounts-archived-summary"
            style="margin-left: 8px; font-weight: normal; opacity: 0.75"
          >
            {{ archivedSummary }}
          </span>
        </NButton>
      </template>
      <NDataTable
        v-if="archivedExpanded"
        :columns="columns"
        :data="archivedBalances"
        :bordered="false"
        size="small"
        :row-props="rowProps"
      />
    </NCard>

    <!-- 编辑账户弹窗：type 不可改（参与余额符号归属），币种仅无交易账户可改（后端校验）。
         显示由「意图非空」派生（无独立 show 布尔），关闭（✕ / ESC / 取消 / 提交成功）
         统一经工厂清回 null 终态；序号作表单 key 强制重建（ADR-0072）。 -->
    <AppModal
      :show="editIntent !== null"
      :title="t('accounts.edit.title')"
      preset="card"
      display-directive="if"
      card-size="sm"
      @update:show="
        (show: boolean) => {
          if (!show) closeEdit();
        }
      "
    >
      <NForm
        v-if="editIntent"
        :key="editSeq"
        label-placement="left"
        :show-feedback="false"
        size="small"
      >
        <!-- 行距节奏容器：NFormItem 默认零行距，表单项与按钮行同包（ADR-0079 决策 4 / issue #804） -->
        <NSpace vertical :size="12">
          <NFormItem :label="t('accounts.create.name')">
            <NInput
              v-model:value="editName"
              :placeholder="t('accounts.create.namePlaceholder')"
              :disabled="isGoalBoundEdit"
              data-testid="account-edit-name"
            />
          </NFormItem>
          <NFormItem :label="t('accounts.create.type')">
            <NInput :value="accountTypeLabel(editIntent.row)" disabled />
          </NFormItem>
          <NFormItem :label="t('accounts.create.currency')">
            <AppSelect
              v-model:value="editCurrency"
              :options="currencyOptions()"
              style="width: 100%"
            />
          </NFormItem>
          <!-- 信用卡档案字段与只读摘要（spec #1327 / ADR-0119）：仅信用卡账户出现。
               摘要是**已保存**的额度用量与下次账单节点，不随上方未提交的输入实时变化。 -->
          <NFormItem v-if="isCreditEdit" :label="t('accounts.credit.limit')">
            <NInputNumber
              v-model:value="editCreditLimit"
              :precision="2"
              :placeholder="t('accounts.credit.limitPlaceholder')"
              style="width: 100%"
            />
          </NFormItem>
          <NFormItem v-if="isCreditEdit" :label="t('accounts.credit.statementDay')">
            <NInputNumber
              v-model:value="editStatementDay"
              :precision="0"
              :placeholder="t('accounts.credit.dayPlaceholder')"
              style="width: 100%"
            />
          </NFormItem>
          <NFormItem v-if="isCreditEdit" :label="t('accounts.credit.dueDay')">
            <NInputNumber
              v-model:value="editDueDay"
              :precision="0"
              :placeholder="t('accounts.credit.dayPlaceholder')"
              style="width: 100%"
            />
          </NFormItem>
          <NFormItem v-if="isCreditEdit" :label="t('accounts.credit.usageLabel')">
            <NText>{{ editUsageText }}</NText>
          </NFormItem>
          <NFormItem v-if="isCreditEdit" :label="t('accounts.credit.nextLabel')">
            <NText>{{ editNextNodesText }}</NText>
          </NFormItem>
          <NSpace justify="end" :size="8">
            <NButton @click="closeEdit">{{ t("accounts.edit.cancel") }}</NButton>
            <NButton type="primary" @click="submitEdit">{{ t("accounts.edit.save") }}</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 调整余额弹窗：输入目标余额，实时显示差额与去向；日期默认今天可改（对账常补记）。
         显示由「意图非空」派生（无独立 show 布尔），关闭（✕ / ESC / 取消 / 提交成功）
         统一经工厂清回 null 终态；序号作表单 key 强制重建（ADR-0072）。 -->
    <AppModal
      :show="adjustIntent !== null"
      :title="t('accounts.adjust.title')"
      preset="card"
      display-directive="if"
      card-size="sm"
      @update:show="
        (show: boolean) => {
          if (!show) closeAdjust();
        }
      "
    >
      <NForm
        v-if="adjustIntent"
        :key="adjustSeq"
        label-placement="left"
        :show-feedback="false"
        size="small"
      >
        <!-- 行距节奏容器：NFormItem 默认零行距，表单项与按钮行同包（ADR-0079 决策 4 / issue #804） -->
        <NSpace vertical :size="12">
          <NFormItem :label="t('accounts.adjust.currentBalance')">
            <NText>{{ formatAmount(adjustIntent.row.balance_cents, adjustCurrency) }}</NText>
          </NFormItem>
          <NFormItem :label="t('accounts.adjust.targetBalance')">
            <NInputNumber
              v-model:value="adjustTarget"
              :precision="2"
              :placeholder="t('accounts.adjust.targetPlaceholder')"
              style="width: 100%"
            />
          </NFormItem>
          <NFormItem :label="t('accounts.adjust.date')">
            <AppDatePicker v-model:value="adjustDate" type="date" style="width: 100%" />
          </NFormItem>
          <NFormItem :label="t('accounts.adjust.delta')" :show-label="adjustDeltaText === ''">
            <NText v-if="adjustDelta === 0">{{ t("accounts.adjust.deltaZero") }}</NText>
            <NText v-else-if="adjustDeltaText" :type="adjustDelta! > 0 ? 'success' : 'warning'">
              {{ adjustDeltaText }}{{ t("accounts.adjust.hint") }}
            </NText>
          </NFormItem>
          <NSpace justify="end" :size="8">
            <NButton @click="closeAdjust">{{ t("accounts.edit.cancel") }}</NButton>
            <NButton
              type="primary"
              :disabled="adjustTargetCents === null || adjustDelta === 0"
              @click="submitAdjust"
            >
              {{ t("accounts.adjust.confirm") }}
            </NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 行菜单（操作列「⋯」与行右键共用）：手动定位弹出；开合上报经薄封装
         attrs watch 自动生效（`:show` 绑定照旧） -->
    <AppDropdown
      trigger="manual"
      placement="bottom-start"
      :show="menuShow"
      :x="menuX"
      :y="menuY"
      :options="menuOptions"
      style="max-width: 140px"
      @select="rowMenu.select"
      @clickoutside="rowMenu.close"
    />
  </NSpace>
</template>
