<script setup lang="ts">
import { ref, watch } from 'vue'
import { NButton, NForm, NFormItem, NInput, NSpace, useMessage } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'
import { api } from '@/api'
import { t } from '@/i18n'
import type { Merchant, MerchantUpdateInput } from '@/types'

const props = defineProps<{
  show: boolean
  merchant: Merchant | null
}>()
const emit = defineEmits<{ 'update:show': [value: boolean] }>()

const message = useMessage()

const editName = ref('')

// 打开弹窗时同步待编辑字段（弹窗内容在关闭后仍保留在 DOM，需在打开时回填；
// immediate 兼容 show 初始即为 true 的挂载）
watch(
  () => [props.show, props.merchant],
  () => {
    if (!props.show || !props.merchant) return
    editName.value = props.merchant.name
  },
  { immediate: true },
)

async function saveEdit() {
  const m = props.merchant
  if (!m) return
  const name = editName.value.trim()
  if (!name) {
    message.warning(t('settings.merchants.msg.nameRequired'))
    return
  }
  const input: MerchantUpdateInput = { name }
  try {
    await api.updateMerchant(m.id, input)
    message.success(t('settings.merchants.msg.updated'))
    emit('update:show', false)
    // 参考数据由 ledger:changed 信号自动重拉：历史交易即时显示新名（引用指向 id）
  } catch (e) {
    // 重名等后端校验错误原样上抛展示（如「商户已存在: 京东」），弹窗不关、内容不丢
    message.error(t('settings.merchants.msg.updateFailed', { msg: e }))
  }
}
</script>

<template>
  <!-- 卡片外观走 AppModal 对话框排版规范（spec #630）：sm 档 + 默认无边框；
       按钮行右对齐单主键（轻量单字段编辑保留无取消键语义，issue #637）。 -->
  <AppModal
    :show="show"
    :title="t('settings.merchants.editModal.title')"
    preset="card"
    card-size="sm"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <NFormItem :label="t('settings.merchants.form.name')">
        <NInput v-model:value="editName" :placeholder="t('settings.merchants.form.namePlaceholder')" />
      </NFormItem>
      <NSpace justify="end">
        <NButton type="primary" @click="saveEdit">{{ t('settings.merchants.form.save') }}</NButton>
      </NSpace>
    </NForm>
  </AppModal>
</template>
