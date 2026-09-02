<script setup lang="ts">
import {
  NForm,
  NFormItem,
  NInput,
  NButton,
  NButtonGroup,
  NSpace,
} from 'naive-ui'
import { t } from '@/i18n'
import AppSelect from '@/components/AppSelect.vue'
import AppDatePicker from '@/components/AppDatePicker.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { useLendingForm } from '@/composables/useLendingForm'
import { LENDING_FORM_DIRECTIONS, type LendingFormDirection } from '@/domain/lending'
import type { Transaction } from '@/types'

// 借贷录入 = 转账表单的借贷变体（issue #374 / ADR-0053）：不新增交易 kind，提交产物与
// 转账同构（useLendingForm 复用 useTransferForm 的装配与提交路由）。创建模式由入口预置
// 方向（「借出」「借入」两项各预设其一，表单内方向切换覆盖四方向）；编辑模式 kind 锁死
// 为既有交易的 transfer，方向由两端账户类型派生回填（分派方 TransactionForm 已识别）。
const props = defineProps<{
  /** 创建模式预置方向；编辑模式忽略（按既有交易派生，派生失败时兜底） */
  initialDirection?: LendingFormDirection
  /** 编辑模式：借贷形态的既有转账交易 */
  editing?: Transaction | null
}>()

const emit = defineEmits<{ created: []; saved: [] }>()

const ctx = useLendingForm({
  initialDirection: props.initialDirection,
  onCreated: () => emit('created'),
  onUpdated: () => emit('saved'),
  editing: () => props.editing ?? null,
})
</script>

<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NSpace vertical :size="12">
      <NFormItem :label="t('transactions.lending.direction')">
        <NButtonGroup size="small">
          <NButton
            v-for="d in LENDING_FORM_DIRECTIONS"
            :key="d"
            :type="ctx.direction.value === d ? 'primary' : 'default'"
            @click="ctx.setDirection(d)"
          >
            {{ t(`transactions.lending.${d}`) }}
          </NButton>
        </NButtonGroup>
      </NFormItem>

      <NFormItem :label="t('transactions.form.amount')">
        <!-- 字段错误态（ADR-0058 / #415）：借贷复用 useTransferForm 金额状态，
             红态行为与转账/支出/收入形态完全一致（自由文本、即时红、不拦截） -->
        <NInput
          v-model:value="ctx.amountText.value"
          :status="ctx.amountError.value ? 'error' : undefined"
          :placeholder="t('transactions.form.amount')"
          style="width: 160px"
          @blur="ctx.markAmountBlurred"
        />
        <AppSelect
          v-model:value="ctx.currencyCode.value"
          :options="ctx.currencyOptions.value"
          style="width: 130px; margin-left: 8px"
        />
      </NFormItem>

      <NFormItem :label="t('transactions.form.fromAccount')">
        <PinyinSelect
          v-model:value="ctx.accountId.value"
          :options="ctx.fromAccountOptions.value"
          :placeholder="t('transactions.form.fromAccountPlaceholder')"
          style="width: 200px"
        />
      </NFormItem>

      <NFormItem :label="t('transactions.form.toAccount')">
        <PinyinSelect
          v-model:value="ctx.toAccountId.value"
          :options="ctx.toAccountOptions.value"
          :placeholder="t('transactions.form.toAccountPlaceholder')"
          style="width: 200px"
        />
      </NFormItem>

      <NFormItem :label="t('transactions.form.date')">
        <AppDatePicker v-model:value="ctx.date.value" type="date" style="width: 200px" />
      </NFormItem>

      <NFormItem :label="t('transactions.form.note')">
        <NInput
          v-model:value="ctx.note.value"
          :placeholder="t('transactions.form.notePlaceholder')"
          style="width: 280px"
        />
      </NFormItem>

      <!-- 任一字段错误态下禁用（红框＋提交禁用两件同发，ADR-0058 决策 1） -->
      <NButton type="primary" :disabled="ctx.hasFieldError.value" @click="ctx.submit">
        {{
          editing
            ? t('transactions.form.saveChanges')
            : t('transactions.lending.submit', {
                dir: t(`transactions.lending.${ctx.direction.value}`),
              })
        }}
      </NButton>
    </NSpace>
  </NForm>
</template>
