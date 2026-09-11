import { describe, it, expect } from 'vitest'
import { ref } from 'vue'
import { useFieldErrors } from '@/composables/useFieldErrors'
import { judgeAmountText, judgeQuantityText, judgePriceText } from '@/utils/field-error'

/**
 * 表单级字段错误态装配工厂测试（ADR-0058 决策 4 补完 / issue #1007）：
 * 以 Vue 响应式直接驱动、不挂组件，钉死「文本 + 时机事件 → error / value」全表、
 * hasError 聚合、reset 清时机、enabled 抑制。判定口径闭集归
 * src/__tests__/field-error.test.ts（纯函数层），此处只验证装配。
 */
describe('useFieldErrors（字段错误态装配工厂，issue #1007）', () => {
  it('初始空文本：无错误态、无已解析值、聚合无错误', () => {
    const amountText = ref('')
    const errors = useFieldErrors({ amount: { text: amountText, judge: judgeAmountText } })

    expect(errors.fields.amount.error.value).toBeNull()
    expect(errors.fields.amount.value.value).toBeNull()
    expect(errors.hasError.value).toBe(false)
  })

  it('非法文本即时红（不待失焦/保存尝试），并令 hasError 置位', () => {
    const amountText = ref('')
    const errors = useFieldErrors({ amount: { text: amountText, judge: judgeAmountText } })

    amountText.value = '4.30发'

    expect(errors.fields.amount.error.value).toBe('parse-error')
    expect(errors.hasError.value).toBe(true)
  })

  it('超精度即时红（金额至多两位小数）', () => {
    const amountText = ref('')
    const errors = useFieldErrors({ amount: { text: amountText, judge: judgeAmountText } })

    amountText.value = '4.305'

    expect(errors.fields.amount.error.value).toBe('over-precision')
  })

  it('空值初始不红；markBlurred 后红；修正为合法即解除', () => {
    const amountText = ref('')
    const errors = useFieldErrors({ amount: { text: amountText, judge: judgeAmountText } })

    expect(errors.fields.amount.error.value).toBeNull()
    errors.fields.amount.markBlurred()
    expect(errors.fields.amount.error.value).toBe('empty')
    amountText.value = '12'
    expect(errors.fields.amount.error.value).toBeNull()
    expect(errors.hasError.value).toBe(false)
  })

  it('空值初始不红；markSaveAttempted 后红（提交意图兜底，表单级共享）', () => {
    const amountText = ref('')
    const errors = useFieldErrors({ amount: { text: amountText, judge: judgeAmountText } })

    errors.markSaveAttempted()

    expect(errors.fields.amount.error.value).toBe('empty')
    expect(errors.hasError.value).toBe(true)
  })

  it('ok 判定归一已解析值：金额取 yuan、数量取 value（非 ok 为 null）', () => {
    const amountText = ref('12.5')
    const quantityText = ref('100')
    const errors = useFieldErrors({
      amount: { text: amountText, judge: judgeAmountText },
      quantity: { text: quantityText, judge: judgeQuantityText },
    })

    expect(errors.fields.amount.value.value).toBe(12.5)
    expect(errors.fields.quantity.value.value).toBe(100)
    amountText.value = '4.30发'
    expect(errors.fields.amount.value.value).toBeNull()
  })

  it('hasError 聚合任一字段：单字段红即置位，全清即复位', () => {
    const quantityText = ref('')
    const priceText = ref('')
    const errors = useFieldErrors({
      quantity: { text: quantityText, judge: judgeQuantityText },
      price: { text: priceText, judge: judgePriceText },
    })

    expect(errors.hasError.value).toBe(false)
    priceText.value = '1.23456'
    expect(errors.fields.quantity.error.value).toBeNull()
    expect(errors.fields.price.error.value).toBe('over-precision')
    expect(errors.hasError.value).toBe(true)
    priceText.value = '1.2345'
    expect(errors.hasError.value).toBe(false)
  })

  it('reset 清零全部时机标志（touched + saveAttempted）', () => {
    const quantityText = ref('')
    const priceText = ref('')
    const errors = useFieldErrors({
      quantity: { text: quantityText, judge: judgeQuantityText },
      price: { text: priceText, judge: judgePriceText },
    })

    errors.markSaveAttempted()
    errors.fields.quantity.markBlurred()
    errors.fields.price.markBlurred()
    expect(errors.hasError.value).toBe(true)

    errors.reset()

    expect(errors.fields.quantity.error.value).toBeNull()
    expect(errors.fields.price.error.value).toBeNull()
    expect(errors.hasError.value).toBe(false)
  })

  it('reset 不清文本：即时类错误依文本判定，重置后仍在', () => {
    const amountText = ref('4.30发')
    const errors = useFieldErrors({ amount: { text: amountText, judge: judgeAmountText } })

    errors.markSaveAttempted()
    expect(errors.fields.amount.error.value).toBe('parse-error')

    errors.reset()

    expect(errors.fields.amount.error.value).toBe('parse-error')
  })

  it('enabled=false 抑制单字段错误态与聚合（基金形态单价无输入面）', () => {
    const priceText = ref('')
    const enabled = ref(false)
    const errors = useFieldErrors({
      price: { text: priceText, judge: judgePriceText, enabled: () => enabled.value },
    })

    errors.markSaveAttempted()
    priceText.value = '1.23456'
    expect(errors.fields.price.error.value).toBeNull()
    expect(errors.hasError.value).toBe(false)

    enabled.value = true
    expect(errors.fields.price.error.value).toBe('over-precision')
    expect(errors.hasError.value).toBe(true)
  })
})
