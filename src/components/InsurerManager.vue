<script setup lang="ts">
import { h, ref, computed } from 'vue'
import {
  NButton,
  NCard,
  NCheckbox,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NSpace,
  NTag,
  useMessage,
  type DataTableColumn,
} from 'naive-ui'
import AppPopconfirm from '@/components/AppPopconfirm.vue'
import InsurerEditModal from '@/components/insurers/InsurerEditModal.vue'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { useModalIntent } from '@/composables/useModalIntent'
import { matchLabel } from '@/utils/pinyin-filter'
import { t } from '@/i18n'
import type { Insurer, InsurerInput } from '@/types'

// 保司管理（issue #714 / ADR-0082 决策 3）：保险域自有字典的管理视图，进侧栏
// 资产组「更多」（组内收纳出厂成员，ADR-0063）。交互照商户管理页形态
// （issue #189 先例）：新增表单卡片 + 列表卡片 + 编辑弹窗；写入成功后保司字典
// 由 ledger:changed 信号经参考数据 store 自动重拉，保单表单选择器等消费方即时更新。
// 拼音搜索与显示已删（照商户管理先例 issue #447）：搜索按统一模糊搜索语义本地过滤
// （ADR-0027，唯一定义点为核心交易域 TransactionSearch），只隐藏未命中项、剩余项
// 顺序不变（保护位置记忆），清空恢复完整列表；已删行只读展示（无编辑/删除），
// 照常计入列表条数。保司为名字字典（无 icon/color 视觉字段，同商户 issue #223）；
// 无关联交易条数列（保司不挂交易，条数下钻是商户管理特有，issue #445）。
// 种子量级约 30 家 + 少量即建行，无前端分页（商户管理分页 issue #457 系其
// 字典无界增长所需，保司字典闭集性远高，不做过度设计）。

const reference = useReferenceStore()
const message = useMessage()

/** 列表行：参考数据单一来源的在用保司。 */
const rows = computed<Insurer[]>(() => reference.insurers)

// —— 显示已删（照商户管理先例 issue #447）：默认只显示在用保司；切换后已软删
// 保司以只读行追加在尾部展示（无编辑/删除操作），照常计入条数。已删保司消费
// 参考 store 既有软删缓存（与在用列表同一份含已删全量拉取拆分），无新增拉取。
const showDeleted = ref(false)

/** 已删行：默认（名称序）展示在在用行之后。 */
const deletedRows = computed<Insurer[]>(() => [...reference.deletedInsurers.values()])

// —— 搜索（照商户管理先例 issue #447）：统一模糊搜索语义（全库唯一定义点为
// 核心交易域 TransactionSearch，ADR-0027），复用拼音过滤工具的前端同规格纯函数；
// 保司字典前端全量驻留，属本地过滤形态（拼音可搜下拉同款）。searchTerm
// 过滤只隐藏未命中项、剩余项顺序不变，清空恢复完整列表。
const searchTerm = ref('')

/** 展示行：（显示已删？在用 + 已删：仅在用）→ 搜索词过滤
 * （matchLabel 空输入恒命中，清空即完整列表；filter 保序不重排）。 */
const displayRows = computed<Insurer[]>(() => {
  const base = showDeleted.value ? [...rows.value, ...deletedRows.value] : rows.value
  return base.filter((i) => matchLabel(searchTerm.value, i.name))
})

// —— 新增 ——
const name = ref('')

async function addInsurer() {
  const trimmed = name.value.trim()
  if (!trimmed) {
    message.warning(t('policies.insurers.msg.nameRequired'))
    return
  }
  const input: InsurerInput = { name: trimmed }
  try {
    await api.createInsurer(input)
    message.success(t('policies.insurers.msg.added'))
    name.value = ''
  } catch (e) {
    // 重名错误（「保司已存在: X」）原样上抛展示，表单不清空、用户可直接修正
    message.error(t('policies.insurers.msg.addFailed', { msg: e }))
  }
}

// —— 编辑 ——
// 开启/目标/关闭编排归弹窗意图工厂 ModalIntent（ADR-0072，词汇表 ModalIntent），
// 与商户管理同一形态：意图闭集单成员（携带目标保司行），显示由「意图非空」派生，
// 序号随开启递增驱动表单重建（:key=editSeq），关闭统一经工厂清回 null 终态。

/** 保司编辑弹窗意图（单成员闭集）：携带目标保司行。 */
interface InsurerEditIntent {
  insurer: Insurer
}

const {
  intent: editIntent,
  seq: editSeq,
  open: openEditIntent,
  close: closeEdit,
} = useModalIntent<InsurerEditIntent>()

function openEdit(insurer: Insurer) {
  openEditIntent({ insurer })
}

// —— 删除（软删：存量保单引用照常显示，不再进新建选择列表） ——
async function removeInsurer(id: string) {
  try {
    await api.deleteInsurer(id)
    message.success(t('policies.insurers.msg.deleted'))
  } catch (e) {
    message.error(t('policies.insurers.msg.deleteFailed', { msg: e }))
  }
}

// —— 列表 ——
const columns: DataTableColumn<Insurer>[] = [
  {
    // 已删行带「已删除」标记（照商户管理先例 issue #447）：与在用行可区分。
    title: () => t('policies.insurers.columns.name'),
    key: 'name',
    ellipsis: { tooltip: true },
    render: (i) =>
      i.is_deleted
        ? h(NSpace, { size: 'small', align: 'center', wrap: false }, () => [
            h('span', i.name),
            h(NTag, { size: 'small', bordered: false }, () => t('policies.insurers.deletedTag')),
          ])
        : i.name,
  },
  {
    title: () => t('policies.insurers.columns.actions'),
    key: 'actions',
    width: 140,
    // 已删行只读：无编辑/删除操作。
    render: (i) =>
      i.is_deleted
        ? null
        : h(NSpace, { size: 'small' }, () => [
            h(
              NButton,
              { size: 'tiny', quaternary: true, type: 'primary', onClick: () => openEdit(i) },
              () => t('policies.insurers.rowActions.edit'),
            ),
            h(
              AppPopconfirm,
              { onPositiveClick: () => removeInsurer(i.id) },
              {
                default: () => t('policies.insurers.deleteConfirm'),
                trigger: () =>
                  h(
                    NButton,
                    { size: 'tiny', type: 'error', quaternary: true },
                    () => t('policies.insurers.rowActions.delete'),
                  ),
              },
            ),
          ]),
  },
]
</script>

<template>
  <NSpace vertical :size="16">
    <NCard :title="t('policies.insurers.addTitle')" size="small">
      <NForm label-placement="left" :show-feedback="false" inline size="small">
        <NFormItem :label="t('policies.insurers.form.name')">
          <NInput v-model:value="name" :placeholder="t('policies.insurers.form.namePlaceholder')" style="width: 160px" />
        </NFormItem>
        <NButton type="primary" @click="addInsurer">{{ t('policies.insurers.form.add') }}</NButton>
      </NForm>
    </NCard>

    <NCard :title="t('policies.insurers.listTitle')" size="small">
      <NSpace vertical :size="12">
        <NSpace align="center">
          <NInput
            v-model:value="searchTerm"
            clearable
            :placeholder="t('policies.insurers.searchPlaceholder')"
            style="width: 240px"
          />
          <NCheckbox v-model:checked="showDeleted">
            {{ t('policies.insurers.showDeleted') }}
          </NCheckbox>
        </NSpace>
        <NDataTable
          :columns="columns"
          :data="displayRows"
          :bordered="false"
          size="small"
          :row-key="(i: Insurer) => i.id"
        />
      </NSpace>
    </NCard>

    <!-- 保司编辑弹窗。显示由「意图非空」派生（无独立 show 布尔），关闭（✕ / ESC /
         取消 / 保存成功）统一经工厂清回 null 终态；序号作 key 强制重建（ADR-0072）。 -->
    <InsurerEditModal
      :key="editSeq"
      :show="editIntent !== null"
      :insurer="editIntent?.insurer ?? null"
      @update:show="(v: boolean) => (v ? undefined : closeEdit())"
    />
  </NSpace>
</template>
