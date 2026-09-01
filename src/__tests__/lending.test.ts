import { describe, expect, it } from 'vitest'
import {
  LENDING_DIRECTION_SIDES,
  LENDING_FORM_DIRECTIONS,
  accountMatchesSide,
  deriveLendingDirection,
  lendingAccountSide,
  lendingLabelKey,
  type LendingDirection,
} from '@/domain/lending'
import { ACCOUNT_TYPES, TRANSACTION_KINDS, type AccountType } from '@/types'

/** 测试真值表（与实现独立）：账户类型的借贷侧别归类 */
function sideOf(type: AccountType): 'fund' | 'receivable' | 'debt' {
  if (type === 'receivable' || type === 'debt') return type
  return 'fund'
}

/** 测试真值表：账户类型组合 → 五态（spec #374：借出/收回/借入/还款/普通转账） */
function expectedOf(from: AccountType, to: AccountType): LendingDirection {
  const f = sideOf(from)
  const t = sideOf(to)
  if (f === 'fund' && t === 'receivable') return 'lend'
  if (f === 'receivable' && t === 'fund') return 'collect'
  if (f === 'debt' && t === 'fund') return 'borrow'
  if (f === 'fund' && t === 'debt') return 'repay'
  return 'none'
}

describe('deriveLendingDirection（借贷方向派生，issue #374 S1）', () => {
  it.each([
    ['cash', 'receivable', 'lend'],
    ['bank', 'receivable', 'lend'],
    ['receivable', 'bank', 'collect'],
    ['receivable', 'cash', 'collect'],
    ['debt', 'cash', 'borrow'],
    ['debt', 'bank', 'borrow'],
    ['cash', 'debt', 'repay'],
    ['bank', 'debt', 'repay'],
  ] as const)('transfer %s → %s = %s', (from, to, expected) => {
    expect(deriveLendingDirection('transfer', from, to)).toBe(expected)
  })

  it('账户类型全组合（8×8）与真值表一致：仅四个借贷方向，其余（资金户互转、借贷户互转）一律普通转账', () => {
    for (const from of ACCOUNT_TYPES) {
      for (const to of ACCOUNT_TYPES) {
        expect(deriveLendingDirection('transfer', from, to), `${from} → ${to}`).toBe(
          expectedOf(from, to),
        )
      }
    }
  })

  it('边界：借贷端对端为未知/黑洞账户（账户类型缺失）→ 仍按借贷方向派生（issue #374 修订）', () => {
    // 借贷方向由借贷侧（debt/receivable）唯一决定，对端账户类型缺失
    // （黑洞 is_hidden 占位 / 已删 / 不可查）不改变借贷语义：
    // debt 转出 = 借入、负债方转入 = 还款、receivable 转出 = 收回、receivable 转入 = 借出。
    expect(deriveLendingDirection('transfer', 'debt', null)).toBe('borrow')
    expect(deriveLendingDirection('transfer', 'debt', undefined)).toBe('borrow')
    expect(deriveLendingDirection('transfer', null, 'debt')).toBe('repay')
    expect(deriveLendingDirection('transfer', 'receivable', null)).toBe('collect')
    expect(deriveLendingDirection('transfer', undefined, 'receivable')).toBe('lend')
  })

  it('边界：资金端对端缺失 / 两端均缺失（无任何借贷侧）→ 普通转账', () => {
    // 非借贷侧（资金账户）对端缺失，或两端都缺失：无借贷语义，仍是普通转账。
    expect(deriveLendingDirection('transfer', 'cash', null)).toBe('none')
    expect(deriveLendingDirection('transfer', null, 'bank')).toBe('none')
    expect(deriveLendingDirection('transfer', undefined, undefined)).toBe('none')
  })

  it('边界：非 transfer kind 一律普通转账（借贷只从转账派生，不新增 kind）', () => {
    for (const kind of TRANSACTION_KINDS) {
      if (kind === 'transfer') continue
      expect(deriveLendingDirection(kind, 'cash', 'receivable')).toBe('none')
      expect(deriveLendingDirection(kind, 'debt', 'cash')).toBe('none')
    }
  })
})

describe('账户侧别归类（借贷表单账户过滤的依据）', () => {
  it('receivable/debt 归借贷侧，其余类型归资金侧，缺失归未知', () => {
    expect(lendingAccountSide('receivable')).toBe('receivable')
    expect(lendingAccountSide('debt')).toBe('debt')
    for (const type of ACCOUNT_TYPES) {
      if (type === 'receivable' || type === 'debt') continue
      expect(lendingAccountSide(type)).toBe('fund')
    }
    expect(lendingAccountSide(null)).toBe('unknown')
    expect(lendingAccountSide(undefined)).toBe('unknown')
  })

  it('accountMatchesSide：同侧命中、异侧与未知不命中', () => {
    expect(accountMatchesSide('cash', 'fund')).toBe(true)
    expect(accountMatchesSide('receivable', 'receivable')).toBe(true)
    expect(accountMatchesSide('debt', 'debt')).toBe(true)
    expect(accountMatchesSide('receivable', 'fund')).toBe(false)
    expect(accountMatchesSide('cash', 'receivable')).toBe(false)
    expect(accountMatchesSide(null, 'fund')).toBe(false)
  })
})

describe('LENDING_DIRECTION_SIDES（方向 → 账户过滤侧别）与派生判定同源', () => {
  it('四个方向，每个方向的转出/转入侧恰为「一资金侧 + 一借贷侧」', () => {
    expect(LENDING_FORM_DIRECTIONS).toEqual(['lend', 'collect', 'borrow', 'repay'])
    for (const direction of LENDING_FORM_DIRECTIONS) {
      const { from, to } = LENDING_DIRECTION_SIDES[direction]
      // 侧别组合不变量：每方向恰好一端 fund、另一端 receivable/debt
      const pair = [from, to].sort().join('+')
      expect(
        ['debt+fund', 'fund+receivable'].includes(pair),
        `${direction} 方向应为「资金侧 ↔ 借贷侧」，实际 ${pair}`,
      ).toBe(true)
    }
  })

  it('方向派生与过滤表互为镜像：按过滤表选出的两端账户必派生回同一方向', () => {
    for (const direction of LENDING_FORM_DIRECTIONS) {
      const { from, to } = LENDING_DIRECTION_SIDES[direction]
      const fromType: AccountType = from === 'fund' ? 'cash' : from
      const toType: AccountType = to === 'fund' ? 'bank' : to
      expect(deriveLendingDirection('transfer', fromType, toType)).toBe(direction)
    }
  })
})

describe('lendingLabelKey（方向 → 文案 key）', () => {
  it('四方向映射到 transactions.lending.*，普通转账回退 transactions.kind.transfer', () => {
    expect(lendingLabelKey('lend')).toBe('transactions.lending.lend')
    expect(lendingLabelKey('collect')).toBe('transactions.lending.collect')
    expect(lendingLabelKey('borrow')).toBe('transactions.lending.borrow')
    expect(lendingLabelKey('repay')).toBe('transactions.lending.repay')
    expect(lendingLabelKey('none')).toBe('transactions.kind.transfer')
  })
})
