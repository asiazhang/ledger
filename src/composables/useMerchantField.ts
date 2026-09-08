import { computed, ref } from 'vue'
import { useReferenceStore } from '@/stores/reference'
import { resolveMerchantRef } from '@/composables/resolve-merchant'
import { t } from '@/i18n'

/**
 * 商户输入字段（issue #189 原生于 useCategoryForm，issue #875 / ADR-0092 提取为共享接缝）：
 * 商户选择器状态 + 选项 + 保存时解析，支出/收入与借贷（借出/借入）两个表单形态共用
 * 同一份语义，不出现第二份实现。
 *
 * 解析细则不在此重写：委托共享单一权威 [`resolveMerchantRef`]
 * （`resolve-merchant.ts`，issue #190/#206/#360 既有接缝）——本模块只持有选择器值、
 * 组织选项，并在解析时把本地选择与编辑回填 id 传给该接缝。
 *
 * `editingMerchantId`：编辑模式下该行当前的商户 id（创建路径传 null）——用于两条语义：
 * 1. 编辑时原商户已不在字典（软删且超出会话缓存）→ 追加可读兜底选项承载原 id
 *    （裸 uuid 不可读），提交按「未改动」语义原样保留；
 * 2. 解析时命中该 id 即原样返回（后端 existing_merchant_id unchanged 语义跳过校验），
 *    软删商户的历史交易仍可修改其他字段。
 */
export function useMerchantField(editingMerchantId: string | null = null) {
  const reference = useReferenceStore()

  /** 商户选择器值：已选/已填的商户 id 或自由文本名字（保存时经 resolveMerchantId 解析） */
  const merchantRef = ref<string | null>(null)

  /** 商户下拉选项（在用商户；编辑时原商户已不在字典则追加兜底选项承载原 id） */
  const merchantOptions = computed<{ label: string; value: string }[]>(() => {
    const base = reference.merchants.map((m) => ({ label: m.name, value: m.id }))
    if (editingMerchantId && !reference.merchantMap.has(editingMerchantId)) {
      base.unshift({ label: t('transactions.form.merchantDeleted'), value: editingMerchantId })
    }
    return base
  })

  /** 保存时解析选择器值（空/id/名字 → 商户 id）：细则见 [`resolveMerchantRef`] */
  async function resolveMerchantId(): Promise<string | null> {
    return resolveMerchantRef(merchantRef.value, editingMerchantId)
  }

  return { merchantRef, merchantOptions, resolveMerchantId }
}
