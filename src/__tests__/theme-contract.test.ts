import { beforeAll, describe, expect, it } from 'vitest'
import { setAdapter, removeAdapter } from '@vanilla-extract/css/adapter'
import { setFileScope, endFileScope } from '@vanilla-extract/css/fileScope'
import { NEUTRAL_TOKENS, type NeutralTokens } from '@/theme/design-tokens'
import { darkOverrides, lightOverrides } from '@/theme/overrides'
import type * as AppTheme from '@/theme/app-theme.css.ts'
import type * as ThemeContract from '@/theme/theme-contract'

/**
 * 新方案主题合同（issue #888 / ADR-0093）：给定亮/暗模式，产出正确的根主题类
 * 与组件库主题覆盖，且两套产物与 token 取值同源一致。只测外部行为，不断言内部
 * 实现——主题类名字符串本身无意义，断言的是「哪个模式给哪个类」的映射、绑定
 * 行为，以及经 vanilla-extract 官方 adapter 接缝捕获的「类 → CSS 变量」产出物
 * 取值（合同产出物的同源一致性证据）。
 *
 * app-theme.css.ts / theme-contract.ts 必须在装好捕获 adapter 之后动态导入：
 * createTheme 在模块求值时产出 CSS，静态导入会先于 adapter 装配完成求值。
 */

let appTheme: typeof AppTheme
let contract: typeof ThemeContract
type CapturedBlock = { type: string; selector?: string; rule?: { vars?: Record<string, string> } }
let capturedBlocks: CapturedBlock[] = []

/** 按主题类名找捕获到的变量定义块（createTheme 产出的块 type 为 'global'，
 *  选择器为类名本身，不带点）。 */
function varsBlockOf(themeClass: string) {
  return capturedBlocks.find(
    (b) => b.type === 'global' && b.selector?.replace(/^\./, '') === themeClass,
  )
}

beforeAll(async () => {
  const captured: CapturedBlock[] = []
  const adapter: Parameters<typeof setAdapter>[0] = {
    appendCss: (block) => captured.push(block as CapturedBlock),
    registerClassName: () => {},
    registerComposition: () => {},
    markCompositionUsed: () => {},
    onEndFileScope: () => {},
    getIdentOption: () => 'short',
  }
  setAdapter(adapter)
  // vitest 不挂 ve 插件（见 vitest.config.ts）：运行时求值需显式提供 file scope
  // （无 bundler 运行时的受支持用法），指向真实模块路径使标识符与构建同构。
  setFileScope('src/theme/app-theme.css.ts')
  try {
    appTheme = await import('@/theme/app-theme.css.ts')
    contract = await import('@/theme/theme-contract')
  } finally {
    endFileScope()
    removeAdapter()
  }
  capturedBlocks = captured
})

describe('resolveAppTheme（模式 → 根主题类 + 组件库覆盖）', () => {
  it('暗色模式产出暗色根主题类与暗色组件库覆盖', () => {
    const resolved = contract.resolveAppTheme('dark')
    expect(resolved.rootClass).toBe(appTheme.darkThemeClass)
    expect(resolved.overrides).toBe(darkOverrides)
  })

  it('亮色模式产出亮色根主题类与亮色组件库覆盖', () => {
    const resolved = contract.resolveAppTheme('light')
    expect(resolved.rootClass).toBe(appTheme.lightThemeClass)
    expect(resolved.overrides).toBe(lightOverrides)
  })

  it('亮暗根主题类互异；解析为纯函数（重复解析结果稳定，无第二主题状态源）', () => {
    expect(appTheme.darkThemeClass).not.toBe(appTheme.lightThemeClass)
    expect(contract.resolveAppTheme('dark')).toEqual(contract.resolveAppTheme('dark'))
    expect(contract.resolveAppTheme('light')).toEqual(contract.resolveAppTheme('light'))
  })
})

describe('根主题类与 token 取值同源一致（createTheme 产出物行为断言）', () => {
  /** 逐叶校验：主题类携带的 CSS 变量取值 = 对应模式的 token 取值。变量引用经
   *  appVars 对齐（叶序不进断言），失败信息报告到具体常量组.叶子。 */
  function expectThemeVarsMatch(theme: 'dark' | 'light'): void {
    const themeClass = theme === 'dark' ? appTheme.darkThemeClass : appTheme.lightThemeClass
    const vars = varsBlockOf(themeClass)?.rule?.vars
    expect(vars, `主题类 ${themeClass} 携带合同变量定义`).toBeDefined()
    const tokens = NEUTRAL_TOKENS[theme]
    const contractVars = appTheme.appVars as unknown as Record<string, Record<string, string>>
    for (const group of Object.keys(tokens) as (keyof NeutralTokens)[]) {
      for (const [leaf, value] of Object.entries(tokens[group])) {
        const varRef = contractVars[group]?.[leaf]
        expect(varRef, `${group}.${leaf} 在合同中存在且为 var() 引用`).toMatch(/^var\(--/)
        expect(vars?.[varRef], `${group}.${leaf} 取值同源`).toBe(value)
      }
    }
  }

  it('暗色主题类携带的合同变量取值 = NEUTRAL_TOKENS.dark', () => {
    expectThemeVarsMatch('dark')
  })

  it('亮色主题类携带的合同变量取值 = NEUTRAL_TOKENS.light', () => {
    expectThemeVarsMatch('light')
  })
})

describe('bindRootThemeClass（主题类绑定应用根元素）', () => {
  it('当前模式类挂 body、另一模式类被移除——换装是单一动作', () => {
    contract.bindRootThemeClass(appTheme.darkThemeClass)
    expect(document.body.classList.contains(appTheme.darkThemeClass)).toBe(true)
    expect(document.body.classList.contains(appTheme.lightThemeClass)).toBe(false)

    contract.bindRootThemeClass(appTheme.lightThemeClass)
    expect(document.body.classList.contains(appTheme.lightThemeClass)).toBe(true)
    expect(document.body.classList.contains(appTheme.darkThemeClass)).toBe(false)
  })

  it('重复绑定同一类不产生重复类名', () => {
    contract.bindRootThemeClass(appTheme.darkThemeClass)
    contract.bindRootThemeClass(appTheme.darkThemeClass)
    expect(
      [...document.body.classList].filter((c) => c === appTheme.darkThemeClass),
    ).toHaveLength(1)
  })
})

describe('appVars（主题变量合同）', () => {
  it('合同覆盖中性 token 的全部四组常量（形状与 NeutralTokens 同构）', () => {
    for (const group of ['radius', 'background', 'border', 'text'] as const) {
      expect(appTheme.appVars).toHaveProperty(group)
      for (const leaf of Object.keys(NEUTRAL_TOKENS.dark[group])) {
        expect(appTheme.appVars[group]).toHaveProperty(leaf)
      }
    }
  })

  it('全部叶子是 var() 引用而非烘焙取值——消费方经变量随主题类整体换装', () => {
    const leaves: unknown[] = []
    for (const group of Object.values(appTheme.appVars)) {
      for (const leaf of Object.values(group)) leaves.push(leaf)
    }
    expect(leaves.length).toBeGreaterThan(0)
    for (const leaf of leaves) expect(leaf).toMatch(/^var\(--/)
  })
})
