<script setup lang="ts">
import { ref, watch } from "vue";
import { NButton, NForm, NFormItem, NInput, NSpace, useMessage } from "naive-ui";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import AppDatePicker from "@ledger/ui-kit/AppDatePicker.vue";
import { t } from "@ledger/i18n";
import { errorMessage } from "@ledger/utils/errors";
import { yuanToCents, centsToYuan } from "@ledger/money";
import { useSavingsGoalsStore } from "@/savings-goal/savingsGoals";
import type { SavingsGoal, SavingsGoalInput, SavingsGoalUpdateInput } from "@ledger/types";

/**
 * 储蓄目标新建/编辑弹窗（spec #1750 / issue #1751 建档 / issue #1752 编辑）：
 * 名称与目标金额必填、截止日期可选（不填即无截止日）。专属账户由后端同事务
 * 自动创建 / 改名联动——表单不出现账户字段（目标名权威、账户名随动只读）。
 *
 * 编辑模式（issue #1752，PhysicalAssetFormModal 先例）：四字段全量替换
 * （名称 / 目标金额 / 截止日期 / 手填「计划月存」）；计划月存字段结构性只在
 * 编辑模式出现（新建走 create 入参，不携带该字段）。保存成功后关弹窗，列表经
 * store 重拉刷新；后端校验错误原样展示，弹窗不关、内容不丢。
 */
const props = defineProps<{
  show: boolean;
  /** 待编辑目标；null = 新建模式 */
  editing: SavingsGoal | null;
}>();
const emit = defineEmits<{ "update:show": [value: boolean] }>();

const message = useMessage();
const savingsGoalsStore = useSavingsGoalsStore();

// —— 表单状态 ——
const name = ref("");
const targetYuan = ref("");
const deadline = ref<string | null>(null);
/** 手填「计划月存」（元；空 = 清除，仅编辑模式出现）。 */
const plannedMonthlyYuan = ref("");

/** 打开时回填/复位：编辑模式预填四字段（金额分 → 元），新建复位空白建单
 *  （immediate 兼容初始 show）。 */
watch(
  () => [props.show, props.editing] as const,
  () => {
    if (!props.show) return;
    const p = props.editing;
    name.value = p?.name ?? "";
    targetYuan.value = p ? String(centsToYuan(p.target_amount_cents)) : "";
    deadline.value = p?.deadline ?? null;
    plannedMonthlyYuan.value =
      p?.planned_monthly_cents == null ? "" : String(centsToYuan(p.planned_monthly_cents));
  },
  { immediate: true },
);

function close() {
  emit("update:show", false);
}

async function save() {
  // 客户端必填校验（消息与后端码化错误同源语义，双保险防呆）
  if (!name.value.trim()) {
    message.warning(t("savingsGoals.form.msg.nameRequired"));
    return;
  }
  const targetCents = yuanToCents(targetYuan.value);
  if (targetCents === null || targetCents <= 0) {
    message.warning(t("savingsGoals.form.msg.targetInvalid"));
    return;
  }
  // 计划月存：空 = 清除（null）；填写必须为正数（与后端 planned-monthly 同校验）
  let plannedCents: number | null = null;
  if (plannedMonthlyYuan.value.trim() !== "") {
    plannedCents = yuanToCents(plannedMonthlyYuan.value);
    if (plannedCents === null || plannedCents <= 0) {
      message.warning(t("savingsGoals.form.msg.plannedInvalid"));
      return;
    }
  }

  try {
    if (props.editing) {
      // 编辑模式：四字段全量替换（改名联动与计划月存清除由后端同一事务承载）
      const input: SavingsGoalUpdateInput = {
        name: name.value.trim(),
        target_amount_cents: targetCents,
        deadline: deadline.value || null,
        planned_monthly_cents: plannedCents,
      };
      await savingsGoalsStore.update(props.editing.id, input);
      message.success(t("savingsGoals.msg.updated"));
    } else {
      const input: SavingsGoalInput = {
        name: name.value.trim(),
        target_amount_cents: targetCents,
        deadline: deadline.value || null,
      };
      await savingsGoalsStore.create(input);
      message.success(t("savingsGoals.msg.created"));
    }
    close();
  } catch (e) {
    // 后端校验错误原样展示（如「目标金额必须为正数」），弹窗不关、内容不丢
    message.error(errorMessage(e));
  }
}

defineExpose({ save });
</script>

<template>
  <AppModal
    :show="show"
    preset="card"
    :title="editing ? t('savingsGoals.form.titleEdit') : t('savingsGoals.form.title')"
    card-size="md"
    data-testid="savings-goal-form-modal"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <!-- 行距由 NSpace 12 统一提供（对话框排版规范，ADR-0079 决策 4） -->
      <NSpace vertical :size="12">
        <NFormItem :label="t('savingsGoals.form.label.name')">
          <NInput
            v-model:value="name"
            :placeholder="t('savingsGoals.form.placeholder.name')"
            data-testid="savings-goal-name"
          />
        </NFormItem>
        <NFormItem :label="t('savingsGoals.form.label.target')">
          <NInput
            v-model:value="targetYuan"
            :placeholder="t('savingsGoals.form.placeholder.target')"
            style="width: 160px"
            data-testid="savings-goal-amount"
          />
        </NFormItem>
        <NFormItem :label="t('savingsGoals.form.label.deadline')">
          <AppDatePicker
            v-model:formatted-value="deadline"
            type="date"
            value-format="yyyy-MM-dd"
            clearable
            :placeholder="t('savingsGoals.form.placeholder.deadline')"
            style="width: 160px"
            data-testid="savings-goal-deadline"
          />
        </NFormItem>
        <!-- 手填「计划月存」（issue #1752）：可设置与清除，结构性只在编辑模式出现
             （新建走 create 入参不携带该字段——节奏显示归 issue #1753 双向推算）。 -->
        <NFormItem v-if="editing" :label="t('savingsGoals.form.label.plannedMonthly')">
          <NInput
            v-model:value="plannedMonthlyYuan"
            :placeholder="t('savingsGoals.form.placeholder.plannedMonthly')"
            style="width: 160px"
            data-testid="savings-goal-planned-monthly"
          />
        </NFormItem>

        <NSpace justify="end">
          <NButton @click="close">{{ t("savingsGoals.form.cancel") }}</NButton>
          <NButton type="primary" data-testid="savings-goal-save" @click="save">
            {{ t("savingsGoals.form.save") }}
          </NButton>
        </NSpace>
      </NSpace>
    </NForm>
  </AppModal>
</template>
