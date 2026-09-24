<script setup lang="ts">
import { errorMessage } from "@ledger/utils/errors";
import { centsToYuan, formatAmount, yuanToCents } from "@ledger/money";
import { computed, h, onMounted, ref } from "vue";
import {
  NCard,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NSpace,
  NText,
  useMessage,
  useThemeVars,
  type DataTableColumns,
} from "naive-ui";
import { api } from "@ledger/api";
import { t } from "@ledger/i18n";
import { useReferenceStore } from "@/stores/reference";
import { useSavingsGoalsStore } from "@/savings-goal/savingsGoals";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import AppDropdown from "@ledger/ui-kit/AppDropdown.vue";
import AppDatePicker from "@ledger/ui-kit/AppDatePicker.vue";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import { useAppDialog } from "@/composables/useAppDialog";
import { useModalIntent } from "@ledger/modal-intent";
import { useRowContextMenu } from "@ledger/row-context-menu";
import { useWindowTier } from "@ledger/window-tier";
import AccountLink from "@/accounts/AccountLink.vue";
import { buildAccountRowMenuOptions } from "@/accounts/account-row-menu";
import {
  CREDIT_UTILIZATION_WARNING_PERCENT,
  creditProfile,
  listUtilizationPercent,
} from "@/accounts/credit-card";
import { ACCOUNT_TYPES } from "@ledger/types";
import type { AccountBalance, AccountInput, AccountType, AccountUpdateInput } from "@ledger/types";

const reference = useReferenceStore();
const savingsGoalsStore = useSavingsGoalsStore();
const message = useMessage();
const dialog = useAppDialog();
const themeVars = useThemeVars();
const balances = ref<AccountBalance[]>([]);

// 移动档适配（issue #847 / ADR-0088 决策 11 票⑦）：账户列表三分列 + 新增表单
// 纵向堆叠 + 「⋯」48px 触控目标。断点口径接窗口分级 composable 唯一事实源，
// 不自立断点；桌面档列结构与表单布局一字不动（回归红线）。
// composable 在 setup 顶层调用一次（监听注册与 onScopeDispose 注销归其内聚），
// 档位派生只读返回值。
const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

const name = ref("");
const type = ref<AccountType>("cash");
const currencyCode = ref("CNY");
const initial = ref<number | null>(0);

// 信用卡档案字段（spec #1327 / ADR-0119）：仅类型选「信用卡」时出现在新增表单。
// 额度以元录入、经统一换算接缝转整数分（与期初余额同款，消浮点误差口径）；
// 账单日 / 还款日 是 1–31 的**声明值**。越界不在此拦截——后端以码化错误显式拒绝。
const creditLimit = ref<number | null>(null);
const statementDay = ref<number | null>(null);
const dueDay = ref<number | null>(null);
const isCreditType = computed(() => type.value === "credit");

// computed：标签经 t() 随界面语言即时切换（ADR-0049）
const typeOptions = computed(() =>
  ACCOUNT_TYPES.map((k) => ({
    label: t(`accounts.type.${k}`),
    value: k,
  })),
);
const currencyOptions = () =>
  reference.currencies.map((c) => ({ label: `${c.name} (${c.code})`, value: c.code }));

// ---------------------------------------------------------------------------
// 账户页分组（issue #1755 / ADR-0133 决策 2）：目标账户特殊性不靠类型值表达——
// 分组按绑定派生（绑定集选择器归储蓄目标 store，#1752 同源消费），无绑定的
// 普通 `other` 账户仍在原列表原位；目标组空集不渲染空卡（模板 v-if）。
// 同一 balances 快照拆分展示、不做任何加减，余额与净资产读数不受分组影响。
// ---------------------------------------------------------------------------
/** 目标绑定账户行（进「储蓄目标」分组卡）。 */
const goalBalances = computed(() =>
  balances.value.filter((row) => savingsGoalsStore.goalAccountIds.has(row.account.id)),
);
/** 非目标账户行（留在原「账户」列表卡）。 */
const normalBalances = computed(() =>
  balances.value.filter((row) => !savingsGoalsStore.goalAccountIds.has(row.account.id)),
);
/** 类型标签：目标绑定账户覆写为「储蓄目标」（三处消费：桌面类型列、移动档副行、编辑弹窗只读 type）。 */
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
    // 档案字段仅信用卡携带；未填即不携带（缺省 = 未设置，后端不写空值）。
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
    // 参考数据由 ledger:changed 信号自动重拉；此处仅刷新交易派生余额
    await refresh();
  } catch (e) {
    message.error(t("accounts.message.createFailed", { message: errorMessage(e) }));
  }
}

async function remove(id: string) {
  try {
    await api.deleteAccount(id);
    message.success(t("accounts.message.deleted"));
    // 参考数据由 ledger:changed 信号自动重拉；此处仅刷新交易派生余额
    await refresh();
  } catch (e) {
    message.error(t("accounts.message.deleteFailed", { message: errorMessage(e) }));
  }
}

/** 删除走 useAppDialog 二次确认（与交易行菜单同语义）：取消不删，确认后才删除。
 * 遮罩点击不构成关闭意图（issue #252 弹层关闭语义）：确认/取消须显式点击。 */
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

// ---------------------------------------------------------------------------
// 编辑账户弹窗（name + currency_code；type 不可改——参与余额符号归属；
// initial_balance_cents 归「调整余额」管，两处不同改同一字段）；目标绑定账户
//（issue #1752）名称随动只读——输入禁用且提交不发 name 键（改名唯一入口是目标编辑）
// 开启/目标/关闭编排归弹窗意图工厂 ModalIntent（ADR-0072）：意图闭集单成员
// （携带目标账户行），显示由「意图非空」派生、序号随开启递增驱动表单重建、
// 关闭清回 null 终态。现状已带序号守卫（序号驱动表单重建），迁移为纯方言
// 替换：行为完全等价，无缺陷修复。
// ---------------------------------------------------------------------------

/** 编辑账户弹窗意图（单成员闭集）：携带目标账户行。 */
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
// 信用卡档案输入（仅信用卡账户可编辑；空 = 未设置，提交时以 `null` 明示清空）。
const editCreditLimit = ref<number | null>(null);
const editStatementDay = ref<number | null>(null);
const editDueDay = ref<number | null>(null);
/** 编辑中的账户是否为信用卡（决定三个档案输入与只读摘要是否出现）。 */
const isCreditEdit = computed(() => editIntent.value?.row.account.type === "credit");
/**
 * 编辑中的账户是否为储蓄目标专属账户（名称输入禁用 + 提交不发 name 键）：
 *  账户身份由绑定派生（ADR-0133 决策 2，issue #1752）——目标名权威、账户名
 *  随动只读（账户侧无独立改名入口），绑定集选择器归口储蓄目标 store（#1755 同源）。
 */
const isGoalBoundEdit = computed(
  () =>
    editIntent.value !== null &&
    savingsGoalsStore.goalAccountIds.has(editIntent.value.row.account.id),
);

function openEdit(row: AccountBalance) {
  editName.value = row.account.name;
  editCurrency.value = row.account.currency_code;
  // 档案字段回填：额度从分转元展示（统一换算接缝），日子为声明值原样。
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
    // 信用卡档案字段三态：给值 = 落定、`null` = 清空（未填即空）；非信用卡账户
    // **根本不发这三键**（不发 = 不改，避免把「不适用」误报成后端守卫错误）。
    const payload: AccountUpdateInput = {
      currency_code: editCurrency.value,
    };
    // 目标绑定账户（issue #1752）：不发 name 键（缺席 = 不改）——账户名随目标
    // 联动，账户侧任何路径都不回写名称，防把陈旧名写回去造成两处名字对不上。
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
    // 参考数据由 ledger:changed 信号自动重拉；此处仅刷新余额
    await refresh();
  } catch (e) {
    message.error(t("accounts.message.saveFailed", { message: errorMessage(e) }));
  }
}

/** 只读摘要的额度视图（派生纯函数模块单点）：取**已保存**的行值，不随上方输入框
 * 实时变化——摘要回答「这张卡现在用了多少额度」，未提交的编辑值不属于它。 */
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

/** 额度用量摘要：额度未设置时给引导文案，不显示占位数字（避免被读成 0 额度）。 */
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

/** 下次账单节点摘要：两个具体日期（ISO 短格式；绝对日期不随界面语言变化）。 */
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

// ---------------------------------------------------------------------------
// 调整余额弹窗（ADR-0026）：校准到目标值，后端生成一笔与黑洞账户的转账
// （Δ>0 从「无」转入、Δ<0 转出至「无」，删除该转账即撤销调整）
// 开启/目标/关闭编排归弹窗意图工厂 ModalIntent（ADR-0072）：意图闭集单成员
// （携带目标账户行），显示由「意图非空」派生、序号随开启递增驱动表单重建、
// 关闭清回 null 终态。现状已带序号守卫（序号驱动表单重建），迁移为纯方言
// 替换：行为完全等价，无缺陷修复。
// ---------------------------------------------------------------------------

/** 调整余额弹窗意图（单成员闭集）：携带目标账户行。 */
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

/** 本地时区日期 → YYYY-MM-DD（不用 toISOString：避免时区偏移使日期漂移一天）。 */
function formatLocalDate(d: Date): string {
  const m = `${d.getMonth() + 1}`.padStart(2, "0");
  const day = `${d.getDate()}`.padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

/** 目标余额（分）：输入以元为单位，经 yuanToCents 统一口径转整数分（非法输入 → null，禁用提交）。 */
const adjustTargetCents = computed(() =>
  adjustTarget.value === null ? null : yuanToCents(adjustTarget.value),
);

/** 差额 Δ = 目标 − 当前：>0 从黑洞转入，<0 转出至黑洞，=0 无需调整。 */
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
    // 参考数据由 ledger:changed 信号自动重拉（若按需新建了黑洞账户）；此处仅刷新余额
    await refresh();
  } catch (e) {
    message.error(t("accounts.message.adjustFailed", { message: errorMessage(e) }));
  }
}

// ---------------------------------------------------------------------------
// 行菜单（编辑 / 调整余额 / 删除）：操作列「⋯」按钮 + 行右键两入口共用同一
// options（buildAccountRowMenuOptions 纯函数，删除项着主题 error 色）与同一
// open 入口。打开、重定位、关闭、选中的全部时序收进行菜单编排工厂
// RowContextMenu（issue #550 / #551，ADR-0077）：业务动作分派留视图（工厂
// 入参回调，选中即收起并交付收起瞬间的目标行）。
// ---------------------------------------------------------------------------

const menuOptions = computed(() =>
  buildAccountRowMenuOptions({ errorColor: themeVars.value.errorColor }),
);

const rowMenu = useRowContextMenu<AccountBalance>((key, row) => {
  if (key === "edit") openEdit(row);
  else if (key === "adjust-balance") openAdjust(row);
  else if (key === "delete") confirmDelete(row);
});

// 可见性由单判别状态派生（非空即显示）；定位坐标取工厂保留值（open 同步更新、
// close 不清零）：naive-ui 离场动画期间仍按 x/y 重定位弹层，视图侧清零会让
// 淡出中的菜单跳到视口左上角闪现一次（issue #798）。
const menuShow = computed(() => rowMenu.state.value !== null);
const menuX = computed(() => rowMenu.position.value.x);
const menuY = computed(() => rowMenu.position.value.y);

/** 表格行属性：绑定行右键菜单（open 内化「收起 → 下一帧重开」重定位舞步；
 * 原生菜单拦截单点归窗口行为守卫，视图不再 preventDefault）。 */
const rowProps = (row: AccountBalance) => ({
  onContextmenu: (e: MouseEvent) => rowMenu.open(e, row),
});

/** 余额单元格渲染（移动/桌面两分支共用，格式化接缝 formatAmount 单点含隐私掩码）。
 * 信用卡行追加一行 12px 使用率小字（仅已用 > 0 且额度已设置；≥90% 用警示色）——
 * 不新增列、不动行结构，只在既有余额单元格内多一行（spec #1327 / ADR-0119）。
 * 百分比不随金额隐私模式掩码：比例不是金额，泄露不到账户规模。 */
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

/** 移动档名称单元格布局：名称与「类型 · 币种」副行纵排；副行弱化小字（内联样式收口
 * 在列配置单点，同 transaction-columns 渲染函数先例）。 */
const MOBILE_NAME_CELL_STYLE = "display: flex; flex-direction: column; gap: 2px; min-width: 0;";
const MOBILE_NAME_SUB_STYLE = "font-size: 12px; opacity: 0.65;";
/** 信用卡行余额单元格内的使用率小字：金额与百分比纵排，小字同移动档副行弱化口径。 */
const CREDIT_CELL_STYLE = "display: flex; flex-direction: column; gap: 2px;";
const CREDIT_SUB_STYLE = "font-size: 12px; opacity: 0.65;";
/** 移动档名称链接：换行不截断（悬停替代原则「空间够则常驻」，触屏无悬停全文）、
 * 文本左对齐（button 拉满单元格宽后默认居中会与桌面行错位，交易列先例）。 */
const MOBILE_NAME_LINK_STYLE = "white-space: normal; text-align: left;";
/** 移动档「⋯」按钮：显式 48×48 触控目标（ADR-0088 全局验收基线；按钮自身达标，
 * 不用伪元素外扩——操作列内相邻行的热区互不侵入）。桌面档不挂，尺寸零变化。 */
const MOBILE_MORE_BUTTON_STYLE = { width: "48px", height: "48px", fontSize: "18px" };

const columns = computed<DataTableColumns<AccountBalance>>(() => {
  /** 操作列：「⋯」与行右键共用同一工厂 open 入口（以点击坐标弹出）；两轴同一
   * 列配置（入口全平台常显），仅移动档加大触控目标。 */
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

  // 移动档三分列（issue #847）：名称（类型/币种并入副行）、余额、操作——328px
  // 内容宽内无横向滚动、逐行可读；桌面档五列一字不动（回归红线）。
  if (isMobileTier.value) {
    return [
      {
        title: t("accounts.list.colName"),
        key: "account.name",
        // 名称下钻：点击跳转交易页并按涉及账户过滤（issue #97）；副行携带类型与币种
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
      // 账户名下钻：点击跳转交易页并按涉及账户过滤（issue #97）
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
  // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll
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
