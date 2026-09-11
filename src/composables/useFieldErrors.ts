import { computed, reactive, ref, type ComputedRef, type Ref } from 'vue'
import { fieldErrorKind, type FieldErrorKind } from '@/utils/field-error'

/**
 * useFieldErrors：字段错误态装配工厂（表单级，ADR-0058 决策 4 补完 / issue #1007）。
 *
 * 判定口径全仓单点在 src/utils/field-error.ts（纯函数层，含全部格式口径）；本模块
 * 收编装配半，替换各表单重复的「判定 + 时机 → 错误态」脚手架：
 * - 字段按「原始文本 ref + 判定函数（可选启用条件）」一行声明；
 * - 产出每字段视图（error / 已解析值 value / markBlurred）与表单聚合
 *   （hasError / markSaveAttempted / reset）。
 *
 * 接口纪律（「只收口径不代判时机」）：
 * - 只记账时机、不代判时机：失焦（touched）与保存尝试（saveAttempted）事件仍由
 *   消费方显式上报，判定与错误态时机规则留在 field-error 纯函数层；
 * - 文本 ref 由表单持有（编辑回填/清空需直接赋值），工厂只读；
 * - reset 只清时机标志、不清文本——文本清空归表单，「初始为空不红」由时机清零兑现。
 */

/** 判定对象最小结构面：ok / 错误 kind，加可选取值载荷（金额/价格为 yuan、数量为 value） */
export interface FieldJudgment {
  kind: 'ok' | FieldErrorKind
  yuan?: number
  value?: number
}

/** 单字段声明 */
export interface FieldSpec {
  /** 原始文本 ref（工厂只读）；表单直接赋值承载回填/清空 */
  text: Ref<string>
  /** 判定纯函数（消费 field-error 单点，不在此复制口径） */
  judge: (text: string) => FieldJudgment
  /** 启用条件 getter：返回 false 时该字段错误态恒 null 且不参与聚合（如基金形态单价无输入面） */
  enabled?: () => boolean
}

/** 单字段视图 */
export interface FieldView {
  /** 当前错误类别（null = 无错误态） */
  error: ComputedRef<FieldErrorKind | null>
  /** ok 判定归一后的数值（金额/价格为元、数量为数值；非 ok 为 null） */
  value: ComputedRef<number | null>
  /** 失焦上报：空值红时机输入（touched） */
  markBlurred: () => void
}

export interface FieldErrors<K extends string> {
  /** 每字段视图，按声明表键名取用 */
  fields: Record<K, FieldView>
  /** 任一启用字段处于错误态（提交禁用依据） */
  hasError: ComputedRef<boolean>
  /** 保存尝试上报：空值兜底红时机输入（saveAttempted，表单级共享） */
  markSaveAttempted: () => void
  /** 清零全部时机标志（重置或提交成功后调用） */
  reset: () => void
}

/** ok 判定载荷归一为数值：金额/价格取 yuan、数量取 value，其余（含无载荷 ok）→ null */
function resolveValue(judgment: FieldJudgment): number | null {
  if (judgment.kind !== 'ok') return null
  return judgment.yuan ?? judgment.value ?? null
}

export function useFieldErrors<K extends string>(specs: Record<K, FieldSpec>): FieldErrors<K> {
  const keys = Object.keys(specs) as K[]
  const saveAttempted = ref(false)
  const touched = reactive<Record<string, boolean>>({})
  for (const key of keys) touched[key] = false

  const fields = {} as Record<K, FieldView>
  for (const key of keys) {
    const spec = specs[key]
    const judgment = computed(() => spec.judge(spec.text.value))
    fields[key] = {
      error: computed<FieldErrorKind | null>(() => {
        if (spec.enabled && !spec.enabled()) return null
        return fieldErrorKind(judgment.value, {
          touched: touched[key],
          saveAttempted: saveAttempted.value,
        })
      }),
      value: computed(() => resolveValue(judgment.value)),
      markBlurred: () => {
        touched[key] = true
      },
    }
  }

  return {
    fields,
    hasError: computed(() => keys.some((key) => fields[key].error.value != null)),
    markSaveAttempted: () => {
      saveAttempted.value = true
    },
    reset: () => {
      saveAttempted.value = false
      for (const key of keys) touched[key] = false
    },
  }
}
