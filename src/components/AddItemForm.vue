<script setup lang="ts">
import { errorMessage } from '@/utils/errors'
import { computed, ref } from 'vue'
import { NButton, NFormItem, NInput, NForm, NSpace, NText, useMessage } from 'naive-ui'
import { formatAmount } from '@/types'
import { useReferenceStore } from '@/stores/reference'
import { useItemsStore } from '@/stores/items'
import type { Transaction } from '@/types'

/**
 * 「加入物品」确认弹窗（issue #119 / ADR-0025 创建唯一入口）：
 * 购买日期 / 基础成本 / 币种从交易自动带出并**只读展示**（提交时原样传给后端，
 * 后端仍以交易值覆盖——前端预填只是口径预览，不是第二事实源）；
 * 名称默认取交易备注（可微调）。确认后调用创建命令（溯源必填）。
 *
 * 后端校验失败（重复创建 / 非 expense 等）时错误信息经 message 可见，
 * 弹窗保持打开，用户可取消或改名重试。
 */
const props = defineProps<{ transaction: Transaction }>()

const emit = defineEmits<{ created: []; cancel: [] }>()

const itemsStore = useItemsStore()
const reference = useReferenceStore()
const message = useMessage()

/** 名称默认取交易备注，可微调；备注为空时留空，提交前由校验提示。 */
const name = ref(props.transaction.note ?? '')
const submitting = ref(false)

const currency = computed(() => reference.getCurrency(props.transaction.currency_code))

async function submit() {
  if (!name.value.trim()) {
    message.warning('请输入物品名称')
    return
  }
  submitting.value = true
  try {
    await itemsStore.create({
      name: name.value.trim(),
      purchase_date: props.transaction.date,
      total_cost_cents: props.transaction.amount_cents,
      currency_code: props.transaction.currency_code,
      note: null,
      purchase_transaction_id: props.transaction.id,
    })
    message.success('已加入物品')
    emit('created')
  } catch (e) {
    message.error(`加入物品失败: ${errorMessage(e)}`)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NSpace vertical :size="12">
      <!-- 自动带出（只读展示，与账本口径一致，不可手改） -->
      <NFormItem label="购买日期">
        <NText>{{ transaction.date }}</NText>
      </NFormItem>
      <NFormItem label="基础成本">
        <NText>
          {{ formatAmount(transaction.amount_cents, currency) }}（{{ transaction.currency_code }}）
        </NText>
      </NFormItem>
      <NFormItem label="物品名称">
        <NInput
          v-model:value="name"
          placeholder="默认取交易备注，可微调"
          style="width: 280px"
          @keyup.enter="submit"
        />
      </NFormItem>
      <NSpace justify="end">
        <NButton :disabled="submitting" @click="emit('cancel')">取消</NButton>
        <NButton type="primary" :loading="submitting" data-testid="add-item-confirm" @click="submit">
          确认创建
        </NButton>
      </NSpace>
    </NSpace>
  </NForm>
</template>
