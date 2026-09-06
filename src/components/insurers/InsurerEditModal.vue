<script setup lang="ts">
import { ref, watch } from 'vue'
import { NButton, NForm, NFormItem, NInput, NSpace, useMessage } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'
import { api } from '@/api'
import { t } from '@/i18n'
import type { Insurer, InsurerUpdateInput } from '@/types'

// 保司编辑弹窗（issue #714 / ADR-0082 决策 3）：交互照商户编辑弹窗先例
// （issue #189）——轻量单字段编辑，改名即时生效（引用指向 id，不回刷），
// 重名等后端校验错误原样上抛展示，弹窗不关、内容不丢。
const props = defineProps<{
  show: boolean
  insurer: Insurer | null
}>()
const emit = defineEmits<{ 'update:show': [value: boolean] }>()

const message = useMessage()

const editName = ref('')

// 打开弹窗时同步待编辑字段（弹窗内容在关闭后仍保留在 DOM，需在打开时回填；
// immediate 兼容 show 初始即为 true 的挂载）
watch(
  () => [props.show, props.insurer],
  () => {
    if (!props.show || !props.insurer) return
    editName.value = props.insurer.name
  },
  { immediate: true },
)

async function saveEdit() {
  const insurer = props.insurer
  if (!insurer) return
  const name = editName.value.trim()
  if (!name) {
    message.warning(t('policies.insurers.msg.nameRequired'))
    return
  }
  const input: InsurerUpdateInput = { name }
  try {
    await api.updateInsurer(insurer.id, input)
    message.success(t('policies.insurers.msg.updated'))
    emit('update:show', false)
    // 保司字典由 ledger:changed 信号自动重拉：存量保单即时显示新名（引用指向 id）
  } catch (e) {
    // 重名等后端校验错误原样上抛展示（如「保司已存在: 平安人寿」），弹窗不关、内容不丢
    message.error(t('policies.insurers.msg.updateFailed', { msg: e }))
  }
}
</script>

<template>
  <!-- 卡片外观走 AppModal 对话框排版规范（spec #630）：sm 档 + 默认无边框；
       按钮行右对齐单主键（轻量单字段编辑保留无取消键语义，issue #637）。 -->
  <AppModal
    :show="show"
    :title="t('policies.insurers.editModal.title')"
    preset="card"
    card-size="sm"
    @update:show="(v: boolean) => emit('update:show', v)"
  >
    <NForm label-placement="left" :show-feedback="false" size="small">
      <!-- 行距由 NSpace 12 统一提供（对话框排版规范，issue #699）：NFormItem 默认零行距 -->
      <NSpace vertical :size="12">
        <NFormItem :label="t('policies.insurers.form.name')">
          <NInput v-model:value="editName" :placeholder="t('policies.insurers.form.namePlaceholder')" />
        </NFormItem>
        <NSpace justify="end">
          <NButton type="primary" @click="saveEdit">{{ t('policies.insurers.form.save') }}</NButton>
        </NSpace>
      </NSpace>
    </NForm>
  </AppModal>
</template>
