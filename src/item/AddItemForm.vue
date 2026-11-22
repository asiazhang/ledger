<script setup lang="ts">
import { errorMessage } from "@ledger/utils/errors";
import { computed, ref } from "vue";
import { NButton, NFormItem, NInput, NForm, NSpace, NText, useMessage } from "naive-ui";
import { formatAmount } from "@ledger/money";
import { useReferenceStore } from "@/stores/reference";
import { useItemsStore } from "@/item/items";
import { t } from "@ledger/i18n";
import type { Transaction } from "@ledger/types";

const props = defineProps<{ transaction: Transaction }>();

const emit = defineEmits<{ created: []; cancel: [] }>();

const itemsStore = useItemsStore();
const reference = useReferenceStore();
const message = useMessage();

const name = ref(props.transaction.note ?? "");
const submitting = ref(false);

const currency = computed(() => reference.getCurrency(props.transaction.currency_code));

async function submit() {
  if (!name.value.trim()) {
    message.warning(t("items.msg.nameRequired"));
    return;
  }
  submitting.value = true;
  try {
    await itemsStore.create({
      name: name.value.trim(),
      purchase_date: props.transaction.date,
      total_cost_cents: props.transaction.amount_cents,
      currency_code: props.transaction.currency_code,
      note: null,
      purchase_transaction_id: props.transaction.id,
    });
    message.success(t("items.msg.added"));
    emit("created");
  } catch (e) {
    message.error(t("items.msg.addFailed", { msg: errorMessage(e) }));
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NSpace vertical :size="12">
      <!-- 自动带出（只读展示，与账本口径一致，不可手改） -->
      <NFormItem :label="t('items.addForm.label.purchaseDate')">
        <NText>{{ transaction.date }}</NText>
      </NFormItem>
      <NFormItem :label="t('items.addForm.label.baseCost')">
        <NText>
          {{ formatAmount(transaction.amount_cents, currency)
          }}{{ t("items.currencySuffix", { code: transaction.currency_code }) }}
        </NText>
      </NFormItem>
      <NFormItem :label="t('items.addForm.label.name')">
        <NInput
          v-model:value="name"
          :placeholder="t('items.addForm.namePlaceholder')"
          style="width: 280px"
          @keyup.enter="submit"
        />
      </NFormItem>
      <NSpace justify="end">
        <NButton :disabled="submitting" @click="emit('cancel')">{{
          t("items.rowActions.cancel")
        }}</NButton>
        <NButton
          type="primary"
          :loading="submitting"
          data-testid="add-item-confirm"
          @click="submit"
        >
          {{ t("items.addForm.confirm") }}
        </NButton>
      </NSpace>
    </NSpace>
  </NForm>
</template>
