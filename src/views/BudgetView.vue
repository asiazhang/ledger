<script setup lang="ts">
import { h, computed, onMounted, ref } from 'vue'
import {
  NCard,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NInputNumber,
  NSpace,
  NProgress,
  NTag,
  NSpin,
  NText,
  useMessage,
  type DataTableColumns,
} from 'naive-ui'
import { api } from '@/api'
import { t } from '@/i18n'
import { useLoadable } from '@/composables/useLoadable'
import { useModalIntent } from '@/composables/useModalIntent'
import { useWindowTier } from '@/composables/useWindowTier'
import AppModal from '@/components/AppModal.vue'
import AppPopconfirm from '@/components/AppPopconfirm.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import {
  MOBILE_CELL_STYLE,
  MOBILE_SUB_STYLE,
  MOBILE_TOUCH_TARGET_STYLE,
} from '@/components/mobile-cells'
import { useReferenceStore } from '@/stores/reference'
import { errorMessage } from '@/utils/errors'
import { yuanToCents } from '@/utils/money'
import { todayStr } from '@/utils/date'
import { formatAmount, centsToYuan } from '@/types'
import type { BudgetInput, BudgetProgress } from '@/types'

const reference = useReferenceStore()
const message = useMessage()
const list = ref<BudgetProgress[]>([])

// 清单加载收编 Loadable（issue #1008 / ADR-0040）：loading 置收、竞态裁决与错误
// 提示内化；失败 = error 置位 + 默认裸 toast（治愈原 try/finally 无 catch 的静默
// 失败与未处理 rejection），旧行保留到下次成功替换。
const { loading, run: runList } = useLoadable(() => api.budgetProgress())

// 移动档适配（issue #848 / ADR-0088 决策 11 票⑧）：预算表三分列（周期/状态并入
// 分类副行、进度含已支/预算文案）+ 新增表单纵排。断点口径接窗口分级 composable
// 唯一事实源，不自立断点；桌面档列结构与表单布局一字不动（回归红线）。
// 进度语义（父含子、子只算自身、超支判断）零变化——同一命令输出，仅布局适配。
const windowTier = useWindowTier()
const isMobileTier = computed(() => windowTier.value === 'mobile')

// 移动档单元格共用样式（纵排堆叠/弱化副行/触控目标）全仓单点：@/components/mobile-cells；
// 首行「分类名 + 状态标签同行」为预算列特有布局，留守本地。
const MOBILE_HEAD_STYLE = 'display: flex; align-items: center; gap: 6px; min-width: 0;'

const categoryId = ref<string | null>(null)
const amount = ref<number | null>(null)

// —— 编辑弹窗（issue #184）：仅金额可改，分类/周期不可改（改法为删旧建新）——
// 开启/目标/关闭编排归弹窗意图工厂 ModalIntent（ADR-0072，词汇表 ModalIntent）：
// 意图闭集单成员（携带目标预算进度行），显示由「意图非空」派生（无独立 show 布尔），
// 序号随开启递增驱动表单重建（:key=editSeq），关闭（✕ / ESC / 取消 / 保存成功）
// 统一经工厂清回 null 终态。现状无序号守卫，迁移为缺陷修复（本票唯一声明的行为
// 变化）：守卫从无到有，「弹窗开着时目标行被替换回填旧行」缺陷消亡，同目标重开
// 重回填等边缘语义细化同归此类；此外等价。金额回填（editAmount）是表单字段而非
// 意图状态，留本视图在开启时直灌（票内裁量，非验收条件）。

/** 编辑预算弹窗意图（单成员闭集）：携带目标预算进度行。 */
interface BudgetEditIntent {
  progress: BudgetProgress
}

const {
  intent: editIntent,
  seq: editSeq,
  open: openEditIntent,
  close: closeEdit,
} = useModalIntent<BudgetEditIntent>()

const editAmount = ref<number | null>(null)

// 创建预算分类选项（issue #356）：从仅顶级支出分类放开到全部支出分类
//（顶级 + 子分类，按 kind 过滤即可——子分类与父分类类型一致），按分类树排序
//（子分类紧跟父分类），子分类 label 用「父 > 子」路径名；拼音可搜由 PinyinSelect
// 对 label 整体匹配，父名/子名拼音均可命中。收入分类（无论层级）被 kind 过滤排除。
const categoryOptions = () =>
  reference
    .treeCategoryOptions('expense')
    .flatMap((root) => [
      { label: reference.categoryDisplayName(root.key, root.category.name), value: root.key },
      ...(root.children ?? []).map((child) => ({
        label: reference.categoryDisplayName(child.key, child.category.name),
        value: child.key,
      })),
    ])

// 子分类预算统一以路径名呈现（列表/编辑弹窗同源，issue #356）；
// 解析不到（守卫生效前的历史孤儿预算）回退后端返回的分类名（「未分类」）。
const displayCategoryName = (row: BudgetProgress) =>
  reference.categoryDisplayName(row.budget.category_id, row.category_name)

const editingCategoryName = computed(() =>
  editIntent.value
    ? reference.categoryDisplayName(
        editIntent.value.progress.budget.category_id,
        editIntent.value.progress.category_name,
      )
    : '',
)

async function refresh() {
  const progress = await runList()
  if (progress !== null) list.value = progress
}

async function create() {
  if (!categoryId.value || amount.value == null) {
    message.warning(t('budget.message.required'))
    return
  }
  if (amount.value <= 0) {
    message.warning(t('budget.message.positive'))
    return
  }
  const input: BudgetInput = {
    category_id: categoryId.value,
    amount_cents: yuanToCents(amount.value) ?? 0,
    // start_date 已退化为记录字段（永久滚动预算，进度与日期无关），传创建当日（本地日历日）即可
    start_date: todayStr(),
  }
  try {
    await api.createBudget(input)
    message.success(t('budget.message.created'))
    categoryId.value = null
    amount.value = null
    await refresh()
  } catch (e) {
    // 后端拒绝（金额非正/收入分类/同分类同周期重复）时把错误信息清晰呈现给用户；
    // 查重提示自带「可编辑该预算的金额」引导
    message.error(t('budget.message.createFailed', { message: errorMessage(e) }))
  }
}

function openEdit(row: BudgetProgress) {
  editAmount.value = centsToYuan(row.budget.amount_cents)
  openEditIntent({ progress: row })
}

async function saveEdit() {
  const target = editIntent.value
  if (!target) return
  if (editAmount.value == null || editAmount.value <= 0) {
    message.warning(t('budget.message.positive'))
    return
  }
  try {
    await api.updateBudget(target.progress.budget.id, {
      amount_cents: yuanToCents(editAmount.value) ?? 0,
    })
    message.success(t('budget.message.updated'))
    closeEdit()
    await refresh()
  } catch (e) {
    message.error(t('budget.message.updateFailed', { message: errorMessage(e) }))
  }
}

async function remove(id: string) {
  try {
    await api.deleteBudget(id)
    message.success(t('budget.message.deleted'))
    await refresh()
  } catch (e) {
    message.error(t('budget.message.deleteFailed', { message: errorMessage(e) }))
  }
}

/** 进度百分比：消耗/上限封顶 100（桌面/移动两分支共用同一口径）。 */
function progressPercentage(row: BudgetProgress): number {
  return row.budget.amount_cents > 0
    ? Math.min(100, Math.round((row.spent_cents / row.budget.amount_cents) * 100))
    : 0
}

/** 进度条状态语义色：超支红 / 正常绿（两分支共用）。 */
function progressStatus(row: BudgetProgress): 'error' | 'success' {
  return row.over_budget ? 'error' : 'success'
}

const columns = computed<DataTableColumns<BudgetProgress>>(() => {
  // 移动档三分列（issue #848）：列数收敛至窄屏无横向滚动，信息并入副行不丢失
  if (isMobileTier.value) {
    return [
      {
        title: t('budget.list.colCategory'),
        key: 'category_name',
        render: (row) =>
          h('div', { style: MOBILE_CELL_STYLE }, [
            h('div', { style: MOBILE_HEAD_STYLE }, [
              h('span', displayCategoryName(row)),
              row.over_budget
                ? h(NTag, { type: 'error' }, () => t('budget.status.over'))
                : h(NTag, { type: 'success' }, () => t('budget.status.normal')),
            ]),
            // 周期以本地化标签呈现（桌面列沿用历史原值呈现，移动档不跟随其旧账）
            h('span', { style: MOBILE_SUB_STYLE }, t(`budget.period.${row.budget.period}`)),
          ]),
      },
      {
        title: t('budget.list.colProgress'),
        key: 'progress',
        render: (row) =>
          h('div', { style: MOBILE_CELL_STYLE }, [
            h(NProgress, {
              type: 'line',
              percentage: progressPercentage(row),
              status: progressStatus(row),
            }),
            h(
              'span',
              { style: MOBILE_SUB_STYLE },
              t('budget.list.spentOfBudget', {
                spent: formatAmount(row.spent_cents),
                total: formatAmount(row.budget.amount_cents),
              }),
            ),
          ]),
      },
      {
        title: t('budget.list.colActions'),
        key: 'actions',
        render: (row) =>
          h(NSpace, { size: 4, wrap: false }, () => [
            h(
              NButton,
              {
                size: 'tiny',
                type: 'primary',
                quaternary: true,
                style: MOBILE_TOUCH_TARGET_STYLE,
                onClick: () => openEdit(row),
              },
              () => t('budget.actions.edit'),
            ),
            h(
              AppPopconfirm,
              { onPositiveClick: () => remove(row.budget.id) },
              {
                default: () => t('budget.actions.confirmDelete'),
                trigger: () =>
                  h(
                    NButton,
                    { size: 'tiny', type: 'error', quaternary: true, style: MOBILE_TOUCH_TARGET_STYLE },
                    () => t('budget.actions.delete'),
                  ),
              },
            ),
          ]),
      },
    ]
  }
  return [
  {
    title: t('budget.list.colCategory'),
    key: 'category_name',
    render: (row) => displayCategoryName(row),
  },
  { title: t('budget.list.colPeriod'), key: 'budget.period' },
  {
    title: t('budget.list.colAmount'),
    key: 'budget.amount_cents',
    render: (row) => formatAmount(row.budget.amount_cents),
  },
  {
    title: t('budget.list.colSpent'),
    key: 'spent_cents',
    render: (row) => formatAmount(row.spent_cents),
  },
  {
    title: t('budget.list.colProgress'),
    key: 'progress',
    render: (row) =>
      h(NProgress, {
        type: 'line',
        percentage: progressPercentage(row),
        status: progressStatus(row),
      }),
  },
  {
    title: t('budget.list.colStatus'),
    key: 'over_budget',
    width: 80,
    render: (row) =>
      row.over_budget
        ? h(NTag, { type: 'error' }, () => t('budget.status.over'))
        : h(NTag, { type: 'success' }, () => t('budget.status.normal')),
  },
  {
    title: t('budget.list.colActions'),
    key: 'actions',
    width: 130,
    render: (row) =>
      h(NSpace, { size: 4, wrap: false }, () => [
        h(
          NButton,
          { size: 'tiny', type: 'primary', quaternary: true, onClick: () => openEdit(row) },
          () => t('budget.actions.edit'),
        ),
        h(
          AppPopconfirm,
          { onPositiveClick: () => remove(row.budget.id) },
          {
            default: () => t('budget.actions.confirmDelete'),
            trigger: () => h(NButton, { size: 'tiny', type: 'error', quaternary: true }, () => t('budget.actions.delete')),
          },
        ),
      ]),
  },
]
})

onMounted(() => {
  // 参考数据由 useReferenceStore self-init + ledger:changed 信号兜底，无需手工 loadAll
  void refresh()
})
</script>

<template>
  <NSpin :show="loading">
    <NSpace vertical :size="16">
      <NCard :title="t('budget.create.title')" size="small">
        <!-- 新增预算表单按窗口分级分档（issue #848）：桌面档行内横排（既有布局一字
             不动）；移动档纵排堆叠（标签上置 + 控件满宽，行内横排窄屏必溢出），
             行距取 ADR-0079 决策 4 的 12px 节奏值（同账户新增表单先例）。
             「添加」主操作热区扩至 ≥48px（视觉不变）。 -->
        <NForm
          :inline="!isMobileTier"
          :label-placement="isMobileTier ? 'top' : 'left'"
          :show-feedback="false"
          size="small"
          :style="
            isMobileTier
              ? { display: 'flex', flexDirection: 'column', gap: '12px' }
              : undefined
          "
        >
          <NFormItem :label="t('budget.create.category')">
            <PinyinSelect
              v-model:value="categoryId"
              :options="categoryOptions()"
              :placeholder="t('budget.create.categoryPlaceholder')"
              :style="isMobileTier ? { width: '100%' } : { width: '160px' }"
            />
          </NFormItem>
          <NFormItem :label="t('budget.create.amount')">
            <NInputNumber
              v-model:value="amount"
              :precision="2"
              :style="isMobileTier ? { width: '100%' } : { width: '140px' }"
            />
          </NFormItem>
          <NButton
            type="primary"
            :class="isMobileTier ? 'touch-hit-area' : undefined"
            :style="isMobileTier ? { '--touch-hit-inset': '-10px -14px' } : undefined"
            @click="create"
          >{{ t('budget.create.add') }}</NButton>
        </NForm>
      </NCard>

      <NCard :title="t('budget.list.title')" size="small">
        <NDataTable :columns="columns" :data="list" :bordered="false" size="small" />
      </NCard>
    </NSpace>

    <!-- 编辑弹窗（issue #184）：仅金额可改，分类/周期只读。显示由「意图非空」派生
         （无独立 show 布尔），关闭统一经工厂清回 null 终态；序号作 key 强制重建
         （ADR-0072）。 -->
    <AppModal
      :key="editSeq"
      :show="editIntent !== null"
      :title="t('budget.edit.title')"
      preset="card"
      display-directive="if"
      card-size="sm"
      @update:show="(v: boolean) => (v ? undefined : closeEdit())"
    >
      <NForm label-placement="left" :show-feedback="false" size="small">
        <!-- 行距节奏容器：NFormItem 默认零行距，表单项与按钮行同包（ADR-0079 决策 4 / issue #804） -->
        <NSpace vertical :size="12">
          <NFormItem :label="t('budget.edit.category')">
            <NText>{{ editingCategoryName }}</NText>
          </NFormItem>
          <NFormItem :label="t('budget.edit.period')">
            <NText>{{
              editIntent ? t(`budget.period.${editIntent.progress.budget.period}`) : ''
            }}</NText>
          </NFormItem>
          <NFormItem :label="t('budget.edit.amount')">
            <NInputNumber v-model:value="editAmount" :precision="2" style="width: 100%" />
          </NFormItem>
          <NSpace justify="end" :size="8">
            <NButton @click="closeEdit">{{ t('budget.edit.cancel') }}</NButton>
            <NButton type="primary" @click="saveEdit">{{ t('budget.edit.save') }}</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>
  </NSpin>
</template>
