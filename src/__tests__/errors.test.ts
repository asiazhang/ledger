import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { errorMessage } from '@/utils/errors'
import { applyLocale, i18n } from '@/i18n'

describe('errorMessage', () => {
  it('从 Tauri 后端 AppError 序列化形态中提取 message', () => {
    expect(errorMessage({ kind: 'Invalid', message: '该分类已存在按月预算' })).toBe(
      '该分类已存在按月预算',
    )
  })

  it('message 非字符串或缺失时回退 String()', () => {
    expect(errorMessage({ kind: 'Db' })).toBe('[object Object]')
    expect(errorMessage({ message: 42 })).toBe('[object Object]')
  })

  it('字符串原样返回', () => {
    expect(errorMessage('预算金额必须为正数')).toBe('预算金额必须为正数')
  })

  it('Error 实例返回其 message', () => {
    expect(errorMessage(new Error('网络错误'))).toBe('网络错误')
  })

  it('null/undefined 走 String() 兜底', () => {
    expect(errorMessage(null)).toBe('null')
    expect(errorMessage(undefined)).toBe('undefined')
  })
})

describe('errorMessage 错误码本地化（issue #342 二期 / ADR-0049）', () => {
  beforeEach(() => {
    // 注入码表夹具（真实码表由后端清点归并进 errors.json，单测自足）
    i18n.global.mergeLocaleMessage('zh-CN', {
      errors: {
        transfer: { 'to-account-required': '转账目标账户不能为空' },
        fx: { 'rate-missing': '缺少 {0}→{1} 汇率，无法折算' },
      },
    })
    i18n.global.mergeLocaleMessage('en-US', {
      errors: {
        transfer: { 'to-account-required': 'Transfer target account is required' },
        fx: { 'rate-missing': 'Missing {0}→{1} exchange rate' },
      },
    })
  })

  afterEach(async () => {
    await applyLocale('zh-CN')
  })

  it('中文界面：已知码命中 zh 模板，输出与后端原文一致', () => {
    expect(
      errorMessage({
        kind: 'Invalid',
        message: '转账目标账户不能为空',
        code: 'transfer.to-account-required',
      }),
    ).toBe('转账目标账户不能为空')
  })

  it('英文界面：已知码显示英文文案', async () => {
    await applyLocale('en-US')
    expect(
      errorMessage({
        kind: 'Invalid',
        message: '转账目标账户不能为空',
        code: 'transfer.to-account-required',
      }),
    ).toBe('Transfer target account is required')
  })

  it('带 params 的错误按码插值成完整自然语句（USD→CNY）', async () => {
    await applyLocale('en-US')
    expect(
      errorMessage({
        kind: 'Invalid',
        message: '缺少 USD→CNY 汇率，无法折算',
        code: 'fx.rate-missing',
        params: ['USD', 'CNY'],
      }),
    ).toBe('Missing USD→CNY exchange rate')
    // 中文界面插值后与后端原文逐字一致
    await applyLocale('zh-CN')
    expect(
      errorMessage({
        kind: 'Invalid',
        message: '缺少 USD→CNY 汇率，无法折算',
        code: 'fx.rate-missing',
        params: ['USD', 'CNY'],
      }),
    ).toBe('缺少 USD→CNY 汇率，无法折算')
  })

  it('未知码降级透传后端原文，绝不显示 key 代号', async () => {
    await applyLocale('en-US')
    expect(
      errorMessage({ kind: 'Invalid', message: '某种新错误', code: 'future.not-yet-translated' }),
    ).toBe('某种新错误')
  })

  it('无 code 的错误不受码化逻辑影响', () => {
    expect(errorMessage({ kind: 'Invalid', message: '老错误' })).toBe('老错误')
  })

  it('真实码表（errors.json）代表码可解析：转账缺目标账户 + 缺汇率插值', async () => {
    await applyLocale('en-US')
    expect(
      errorMessage({
        kind: 'Invalid',
        message: '转账目标账户不能为空',
        code: 'transfer.to-account-required',
      }),
    ).toMatch(/target account/i)
    expect(
      errorMessage({
        kind: 'Invalid',
        message: '缺少 USD→CNY 汇率，无法折算',
        code: 'fx.rate-missing',
        params: ['USD', 'CNY'],
      }),
    ).toContain('USD')
    await applyLocale('zh-CN')
  })

  it('params 非字符串项被过滤，不致运行时崩溃', () => {
    expect(
      errorMessage({
        kind: 'Invalid',
        message: '缺少 USD→CNY 汇率，无法折算',
        code: 'fx.rate-missing',
        params: ['USD', 42, 'CNY'],
      }),
    ).toBe('缺少 USD→CNY 汇率，无法折算')
  })
})
