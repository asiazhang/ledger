<script setup lang="ts">
import { ref, watch } from "vue";
import { NButton, NForm, NFormItem, NInput, NSpace, useMessage } from "naive-ui";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import AppDatePicker from "@ledger/ui-kit/AppDatePicker.vue";
import { t } from "@ledger/i18n";
import { errorMessage } from "@ledger/utils/errors";
import { yuanToCents } from "@ledger/money";
import { useSavingsGoalsStore } from "@/savings-goal/savingsGoals";
import type { SavingsGoalInput } from "@ledger/types";

/**
 * 储蓄目标新建弹窗（spec #1750 / issue #1751）：名称与目标金额必填、截止日期
 * 可选（不填即无截止日）。专属账户由后端同事务自动创建，表单不出现账户字段。
 * 保存成功后关弹窗，列表经 store 重拉刷新；后端校验错误原样展示，弹窗不关、
 * 内容不丢。编辑目标是后续票（ticket ②），本弹窗当前只承载新建形态。
 */
const props = defineProps<{ show: boolean }>();
const emit = defineEmits<{ "update:show": [value: boolean] }>();

const message = useMessage();
const savingsGoalsStore = useSavingsGoalsStore();

// —— 表单状态 ——
const name = ref("");
const targetYuan = ref("");
const deadline = ref<string | null>(null);

/** 打开时复位为空白建单（immediate 兼容初始 show）。 */
watch(
  () => props.show,
  () => {
    if (!props.show) return;
    name.value = "";
    targetYuan.value = "";
    deadline.value = null;
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

  try {
    const input: SavingsGoalInput = {
      name: name.value.trim(),
      target_amount_cents: targetCents,
      deadline: deadline.value || null,
    };
    await savingsGoalsStore.create(input);
    message.success(t("savingsGoals.msg.created"));
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
    :title="t('savingsGoals.form.title')"
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
