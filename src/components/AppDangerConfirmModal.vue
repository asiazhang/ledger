<script setup lang="ts">
import { NAlert, NButton, NSpace, NText } from 'naive-ui'
import AppModal from '@/components/AppModal.vue'

/**
 * 危险确认弹窗共享封装（issue #650 / ADR-0078）：模态危险操作确认的两级形态收口——
 *
 * - **error 级**：不可逆，或回退需依赖用户无感知的隐藏通道，或核心数据面临不可读
 *   风险（开启加密、修改主口令、忘记口令重置、恢复备份）。形态：警示块内加粗后果句
 *   （红色 NAlert + 红色加粗文案，后果说明必选）＋红色确认按钮。
 * - **warning 级**：破坏性但有兜底或影响可再操作（关闭加密、手动清理备份、接管已有
 *   库）。形态：琥珀色（warning）警示块与确认按钮，警示句承载后果与兜底说明。
 *
 * 纯确认、不带输入（带口令输入与失败就地重试的恢复确认保持独立组件，ADR-0078 决策
 * 2 不迁入）；标题、说明段与确认/取消文案全部由调用方经 props 传入（调用方持有
 * i18n 键），本封装不持业务语义、零 i18n 依赖。
 *
 * 分级行为不单独立实现测试——由消费点组件测试覆盖（ADR-0078 测试裁决）；弹层纪律
 * 继承 AppModal 薄封装：遮罩点击不关、✕/ESC 可关、开/关上报弹层注册表（快捷键抑制
 * 零新机制，ADR-0035）；宽度走 AppModal cardSize 分档（md，spec #630）。
 */
withDefaults(
  defineProps<{
    /** 分级（ADR-0078 决策 2）：判据是「错误执行的代价与可回退性」。 */
    level: 'error' | 'warning'
    /** 是否显示（受控）；false 卸载态不渲染内容。 */
    show: boolean
    /** 弹窗标题。 */
    title: string
    /** 说明段（顶部灰字，如状态变化的直接后果）。 */
    lead?: string
    /** 警示块标题（error 级常用）。 */
    alertTitle?: string
    /** 加粗警示句：error 级为后果说明（后果说明必选，类型系统不强制，调用方保证）；
     *  warning 级为后果与兜底说明。 */
    strongWarning?: string
    /** 补充说明段（警示块下灰字，如转换流程与自动重启说明）。 */
    detail?: string
    /** 确认按钮文案。 */
    confirmText: string
    /** 取消按钮文案。 */
    cancelText: string
    /** 是否提交中（危险动作执行期间），期间禁用按钮。 */
    submitting?: boolean
    /** 确认：父组件执行实际动作。 */
    onConfirm: () => void
    /** 取消：父组件关闭弹窗。 */
    onCancel: () => void
  }>(),
  { submitting: false },
)

const emit = defineEmits<{ 'update:show': [value: boolean] }>()

/** 点 ✕ / ESC（NModal 关闭意图）时上报 update:show(false)，由父组件判断是否可关。 */
function onUpdateShow(value: boolean) {
  if (!value) emit('update:show', false)
}
</script>

<template>
  <AppModal
    :show="show"
    preset="card"
    :title="title"
    card-size="md"
    @update:show="onUpdateShow"
  >
    <NSpace vertical :size="12">
      <NText v-if="lead" depth="3">{{ lead }}</NText>

      <!-- 加粗警示句：error 级红色警示块（后果说明必选）；warning 级琥珀警示块 -->
      <NAlert v-if="strongWarning" :type="level" :show-icon="true" :title="alertTitle">
        <NText strong :type="level === 'error' ? 'error' : undefined">{{ strongWarning }}</NText>
      </NAlert>

      <NText v-if="detail" depth="3">{{ detail }}</NText>

      <NSpace justify="end">
        <NButton :disabled="submitting" data-testid="danger-cancel" @click="onCancel">
          {{ cancelText }}
        </NButton>
        <NButton
          :type="level"
          :loading="submitting"
          :disabled="submitting"
          data-testid="danger-confirm"
          @click="onConfirm"
        >
          {{ confirmText }}
        </NButton>
      </NSpace>
    </NSpace>
  </AppModal>
</template>
