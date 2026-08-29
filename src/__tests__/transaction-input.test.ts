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
  it('expense 完整 wire 形状（含占位字段）', () => {
    expect(buildExpenseIncomeInput(expenseState)).toEqual({
      kind: 'expense',
      amount_cents: 8850,
      currency_code: 'CNY',
      account_id: 'acc-1',
      to_account_id: null,
      category_id: 'cat-1',
      merchant_id: 'm-1',
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
    quantity: 100,
    price: 12.34,
    fee: 5.5,
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
      refund_of_transaction_id: null,
      note: null,
      date: '2024-06-15',
      instrument_id: 'ins-1',
      quantity: 100,
      price_cents: 1234,
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
      refund_of_transaction_id: null,
      note: '止盈',
      date: '2024-06-15',
      instrument_id: 'ins-1',
      quantity: 100,
      price_cents: 1234,
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
