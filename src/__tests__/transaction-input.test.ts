import { describe, expect, it } from 'vitest'
import {
  buildExpenseIncomeInput,
  buildRefundInput,
  buildTradeInput,
  buildTransferInput,
} from '@/domain/transaction-input'
import type {
  ExpenseIncomeFormState,
  RefundFormState,
  TradeFormState,
  TransferFormState,
} from '@/domain/transaction-input'

/** 本地日历日构造时间戳（按本地时区分量构造，读取也走本地分量——任何机器时区下往返一致） */
function localTs(year: number, month1: number, day: number, hour = 12, minute = 0): number {
  return new Date(year, month1 - 1, day, hour, minute).getTime()
}

const expenseState: ExpenseIncomeFormState = {
  kind: 'expense',
  amount: 88.5,
  currencyCode: 'CNY',
  accountId: 'acc-1',
  categoryId: 'cat-1',
  merchantId: 'm-1',
  policyId: null,
  note: '午餐',
  date: localTs(2024, 6, 15),
}

const transferState: TransferFormState = {
  amount: 500,
  currencyCode: 'CNY',
  accountId: 'acc-1',
  toAccountId: 'acc-2',
  note: '房租分摊',
  date: localTs(2024, 6, 15),
}

describe('buildExpenseIncomeInput', () => {
  it('可选保单引用（issue #361）：policyId 原样透传到 policy_id', () => {
    const input = buildExpenseIncomeInput({ ...expenseState, policyId: 'pol-1' })
    expect(input.policy_id).toBe('pol-1')
  })

  it('expense 完整 wire 形状（含占位字段）', () => {
    expect(buildExpenseIncomeInput(expenseState)).toEqual({
      kind: 'expense',
      amount_cents: 8850,
      currency_code: 'CNY',
      account_id: 'acc-1',
      to_account_id: null,
      category_id: 'cat-1',
      merchant_id: 'm-1',
      policy_id: null,
      refund_of_transaction_id: null,
      note: '午餐',
      date: '2024-06-15',
    })
  })

  it('income 同构：商户可空、空备注 → null', () => {
    expect(
      buildExpenseIncomeInput({
        kind: 'income',
        amount: 100,
        currencyCode: 'USD',
        accountId: 'acc-2',
        categoryId: null,
        merchantId: null,
        policyId: null,
        note: '',
        date: localTs(2024, 6, 15),
      }),
    ).toEqual({
      kind: 'income',
      amount_cents: 10000,
      currency_code: 'USD',
      account_id: 'acc-2',
      to_account_id: null,
      category_id: null,
      merchant_id: null,
      policy_id: null,
      refund_of_transaction_id: null,
      note: null,
      date: '2024-06-15',
    })
  })

  it('边界：15.505 → 1551（toFixed(8) 消浮点误差后四舍五入到分）', () => {
    const input = buildExpenseIncomeInput({ ...expenseState, amount: 15.505 })
    expect(input.amount_cents).toBe(1551)
  })
})

describe('buildTransferInput', () => {
  it('完整 wire 形状：关联字段全 null 占位', () => {
    expect(buildTransferInput(transferState)).toEqual({
      kind: 'transfer',
      amount_cents: 50000,
      currency_code: 'CNY',
      account_id: 'acc-1',
      to_account_id: 'acc-2',
      category_id: null,
      merchant_id: null,
      policy_id: null,
      refund_of_transaction_id: null,
      note: '房租分摊',
      date: '2024-06-15',
    })
  })

  it('边界：本地 0–8 点的日期归属当天（不走 UTC 截断）', () => {
    const input = buildTransferInput({ ...transferState, date: localTs(2024, 6, 15, 0, 30) })
    expect(input.date).toBe('2024-06-15')
  })

  it('fail fast：转入账户缺失抛中文错误', () => {
    expect(() => buildTransferInput({ ...transferState, toAccountId: null })).toThrow(
      '转入账户不能为空',
    )
  })

  // 借贷四方向（issue #374）：借贷录入是转账表单的变体，装配产物与同形状转账完全
  // 同构——方向 toggle 只影响双账户填法（转出/转入各属哪侧），kind 恒为 transfer、
  // 装配器无任何借贷分支（方向派生收口 domain 层借贷模块，不在此接缝）。
  it.each([
    ['lend', 'acc-fund', 'acc-recv'],
    ['collect', 'acc-recv', 'acc-fund'],
    ['borrow', 'acc-debt', 'acc-fund'],
    ['repay', 'acc-fund', 'acc-debt'],
  ] as const)('借贷方向 %s：装配产物与同形状转账一致（方向即 account_id/to_account_id 填法）', (direction, fromId, toId) => {
    const input = buildTransferInput({
      ...transferState,
      accountId: fromId,
      toAccountId: toId,
      note: `借贷·${direction}`,
    })
    expect(input).toEqual({
      kind: 'transfer',
      amount_cents: 50000,
      currency_code: 'CNY',
      account_id: fromId,
      to_account_id: toId,
      category_id: null,
      merchant_id: null,
      policy_id: null,
      refund_of_transaction_id: null,
      note: `借贷·${direction}`,
      date: '2024-06-15',
    })
  })
})

describe('buildRefundInput', () => {
  const refundState: RefundFormState = {
    amount: 30.5,
    currencyCode: 'CNY',
    accountId: 'acc-1',
    refundOfTransactionId: 'tx-origin',
    note: '部分退款',
    date: localTs(2024, 6, 16),
  }

  it('完整 wire 形状：to_account_id / category_id / merchant_id null 占位（useRefundForm 首次 wire 覆盖）', () => {
    expect(buildRefundInput(refundState)).toEqual({
      kind: 'refund',
      amount_cents: 3050,
      currency_code: 'CNY',
      account_id: 'acc-1',
      to_account_id: null,
      category_id: null,
      merchant_id: null,
      policy_id: null,
      refund_of_transaction_id: 'tx-origin',
      note: '部分退款',
      date: '2024-06-16',
    })
  })

  it('fail fast：原始支出交易缺失抛中文错误', () => {
    expect(() => buildRefundInput({ ...refundState, refundOfTransactionId: null })).toThrow(
      '原始支出交易不能为空',
    )
  })
})

describe('buildTradeInput', () => {
  const buyState: TradeFormState = {
    kind: 'buy',
    currencyCode: 'CNY',
    accountId: 'inv-1',
    instrumentId: 'ins-1',
    amount: null,
    quantity: 100,
    price: 12.34,
    fee: 5.5,
    note: '',
    date: localTs(2024, 6, 15),
  }

  const fundBuyState: TradeFormState = {
    kind: 'buy',
    currencyCode: 'CNY',
    accountId: 'inv-1',
    instrumentId: 'ins-fund',
    amount: 1000,
    quantity: 987.6543,
    price: null,
    fee: 1.5,
    note: '',
    date: localTs(2024, 6, 15),
  }

  it('buy 完整 wire 形状：amount_cents 占位 0、关联字段全 null、数量/单价/费用落位', () => {
    expect(buildTradeInput(buyState)).toEqual({
      kind: 'buy',
      amount_cents: 0,
      currency_code: 'CNY',
      account_id: 'inv-1',
      to_account_id: null,
      category_id: null,
      merchant_id: null,
      policy_id: null,
      refund_of_transaction_id: null,
      note: null,
      date: '2024-06-15',
      instrument_id: 'ins-1',
      quantity: 100,
      price_cents: 123400, // 单价为万分之一元刻度：12.34 元 → 123400（ADR-0038）
      fee_cents: 550,
    })
  })

  it('sell 同构：完整 wire 形状（逐形态断言，含占位字段）', () => {
    expect(buildTradeInput({ ...buyState, kind: 'sell', note: '止盈' })).toEqual({
      kind: 'sell',
      amount_cents: 0,
      currency_code: 'CNY',
      account_id: 'inv-1',
      to_account_id: null,
      category_id: null,
      merchant_id: null,
      policy_id: null,
      refund_of_transaction_id: null,
      note: '止盈',
      date: '2024-06-15',
      instrument_id: 'ins-1',
      quantity: 100,
      price_cents: 123400,
      fee_cents: 550,
    })
  })

  it('边界：空 fee → fee_cents: null（而非 0）', () => {
    const input = buildTradeInput({ ...buyState, fee: null })
    expect(input.fee_cents).toBeNull()
  })

  it('fail fast：数量/标的缺失抛中文错误', () => {
    expect(() => buildTradeInput({ ...buyState, quantity: null })).toThrow('数量不能为空')
    expect(() => buildTradeInput({ ...buyState, instrumentId: null })).toThrow('标的不能为空')
  })

  it('基金形态（issue #302）：确认单金额权威落 amount_cents、单价不落 wire（price_cents null）', () => {
    expect(buildTradeInput(fundBuyState)).toEqual({
      kind: 'buy',
      amount_cents: 100000, // 确认单整分金额（1000 元），权威不被单价舍入污染
      currency_code: 'CNY',
      account_id: 'inv-1',
      to_account_id: null,
      category_id: null,
      merchant_id: null,
      policy_id: null,
      refund_of_transaction_id: null,
      note: null,
      date: '2024-06-15',
      instrument_id: 'ins-fund',
      quantity: 987.6543,
      price_cents: null, // 单价由后端按（金额 ∓ 费用）÷ 份额反算
      fee_cents: 150,
    })
  })

  it('基金形态 sell 同构：金额权威、price_cents null', () => {
    const input = buildTradeInput({ ...fundBuyState, kind: 'sell', note: '赎回' })
    expect(input.amount_cents).toBe(100000)
    expect(input.price_cents).toBeNull()
    expect(input.note).toBe('赎回')
  })

  it('fail fast：金额与单价同供（形态互斥）抛中文错误', () => {
    expect(() => buildTradeInput({ ...fundBuyState, price: 1.01 })).toThrow('不可同时提供')
  })

  it('fail fast：非基金形态缺单价仍抛「单价不能为空」', () => {
    expect(() => buildTradeInput({ ...fundBuyState, amount: null })).toThrow('单价不能为空')
  })
})

describe('fail fast（非法表单状态不静默兜底）', () => {
  it('金额缺失/非法 → 抛中文错误', () => {
    expect(() => buildExpenseIncomeInput({ ...expenseState, amount: null })).toThrow(
      '金额不能为空',
    )
    expect(() => buildExpenseIncomeInput({ ...expenseState, amount: NaN })).toThrow('金额无效')
    expect(() => buildExpenseIncomeInput({ ...expenseState, amount: Infinity })).toThrow(
      '金额无效',
    )
  })

  it('账户缺失 → 抛中文错误', () => {
    expect(() => buildExpenseIncomeInput({ ...expenseState, accountId: null })).toThrow(
      '账户不能为空',
    )
    expect(() => buildTransferInput({ ...transferState, accountId: null })).toThrow(
      '转出账户不能为空',
    )
  })

  it('日期时间戳非法（NaN）→ 抛中文错误', () => {
    expect(() => buildTransferInput({ ...transferState, date: NaN })).toThrow('日期无效')
  })
})
