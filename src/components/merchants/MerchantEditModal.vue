<script setup lang="ts">
import { ref, watch } from 'vue'
import { NButton, NForm, NFormItem, NInput, NModal, useMessage } from 'naive-ui'
import { api } from '@/api'
import type { Merchant, MerchantUpdateInput } from '@/types'

const props = defineProps<{
  show: boolean
  merchant: Merchant | null
}>()
const emit = defineEmits<{ 'update:show': [value: boolean] }>()

const message = useMessage()

const editName = ref('')
const editIcon = ref('')
const editColor = ref('')

// 打开弹窗时同步待编辑字段（弹窗内容在关闭后仍保留在 DOM，需在打开时回填；
// immediate 兼容 show 初始即为 true 的挂载）
watch(
  () => [props.show, props.merchant],
  () => {
    if (!props.show || !props.merchant) return
    editName.value = props.merchant.name
    editIcon.value = props.merchant.icon ?? ''
    editColor.value = props.merchant.color ?? ''
  },
  { immediate: true },
)

async function saveEdit() {
  const m = props.merchant
  if (!m) return
  const name = editName.value.trim()
  if (!name) {
    message.warning('请输入商户名称')
    return
  }
  const input: MerchantUpdateInput = {
    name,
    icon: editIcon.value || null,
    color: editColor.value || null,
  }
  try {
    await api.updateMerchant(m.id, input)
    message.success('已更新商户')
    emit('update:show', false)
    // 参考数据由 ledger:changed 信号自动重拉：历史交易即时显示新名（引用指向 id）
  } catch (e) {
    // 重名等后端校验错误原样上抛展示（如「商户已存在: 京东」），弹窗不关、内容不丢
    message.error(`更新失败: ${e}`)
  }
}
</script>

<template>
  <NModal
    :show="show"
    title="编辑商户"
    preset="card"
    style="width: 420px"
    :bordered="false"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <NFormItem label="名称">
        <NInput v-model:value="editName" placeholder="商户名称" />
      </NFormItem>
      <NFormItem label="图标">
        <NInput v-model:value="editIcon" placeholder="图标名" style="width: 120px" />
      </NFormItem>
      <NFormItem label="颜色">
        <NInput v-model:value="editColor" placeholder="颜色" style="width: 120px" />
      </NFormItem>
      <NButton type="primary" block @click="saveEdit">保存</NButton>
    </NForm>
  </NModal>
</template>
