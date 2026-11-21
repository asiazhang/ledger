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

const props = defineProps<{
  show: boolean;
  editing: SavingsGoal | null;
}>();
const emit = defineEmits<{ "update:show": [value: boolean] }>();

const message = useMessage();
const savingsGoalsStore = useSavingsGoalsStore();

const name = ref("");
const targetYuan = ref("");
const deadline = ref<string | null>(null);
const plannedMonthlyYuan = ref("");

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
  if (!name.value.trim()) {
    message.warning(t("savingsGoals.form.msg.nameRequired"));
    return;
  }
  const targetCents = yuanToCents(targetYuan.value);
  if (targetCents === null || targetCents <= 0) {
    message.warning(t("savingsGoals.form.msg.targetInvalid"));
    return;
  }
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
