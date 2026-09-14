import { describe, expect, it } from 'vitest'
import { effectScope } from 'vue'
import { setFakeMedia } from '@ledger/test-support/media-mock'
import {
  useWindowTier,
  WINDOW_TIER_BREAKPOINT_PX,
  type WindowTier,
} from '../useWindowTier'

/**
 * 窗口分级（Window Tier）模块测试（issue #841，ADR-0088 决策 2 / 词汇表「窗口分级」）：
 * composable 只锁「宽度信号 → 档位」纯映射——断点两侧边界与实时换档；
 * 形态断言（按档位渲染什么）不上浮到本层。换档一律经媒体查询测试接缝
 * （@ledger/test-support/media-mock），宽度不散落魔法数字——断言值由唯一断点常量派生。
 *
 * 包落位（issue #1315 / spec #1148）：测试随被测 composable 成包；断点构建期契约
 * （vite.config 提取与占位符替换）的测试留壳侧 src/__tests__/window-tier-build-contract.test.ts
 * ——包内禁止相对路径穿越（check-frontend-structure.ts 规则③），断言原文未动。
 */

/** 在指定视口宽下取档位（effectScope 内实例化，避免无作用域告警）。 */
function tierAt(width: number): WindowTier {
  setFakeMedia({ width })
  return effectScope().run(() => useWindowTier())!.value
}

describe('useWindowTier 信号 → 档位纯映射（断点两侧边界）', () => {
  it('恰好落在断点上为桌面档（≥ 断点即桌面档）', () => {
    expect(tierAt(WINDOW_TIER_BREAKPOINT_PX)).toBe('desktop')
  })

  it('断点下一像素为移动档（< 断点即移动档）', () => {
    expect(tierAt(WINDOW_TIER_BREAKPOINT_PX - 1)).toBe('mobile')
  })

  it.each([
    ['宽视口（默认桌面环境）', 1280],
    ['超宽视口', 2560],
  ])('%s → 桌面档', (_label, width) => {
    expect(tierAt(width)).toBe('desktop')
  })

  it.each([
    ['手机竖屏下限', 360],
    ['手机竖屏上限附近', 412],
  ])('%s → 移动档', (_label, width) => {
    expect(tierAt(width)).toBe('mobile')
  })
})

describe('useWindowTier 实时换档（媒体查询 change 驱动）', () => {
  it('同一实例随重编程宽度在两档间往返翻转', () => {
    setFakeMedia({ width: 1280 })
    const tier = effectScope().run(() => useWindowTier())!
    expect(tier.value).toBe('desktop')

    setFakeMedia({ width: 360 })
    expect(tier.value).toBe('mobile')

    setFakeMedia({ width: WINDOW_TIER_BREAKPOINT_PX })
    expect(tier.value).toBe('desktop')

    setFakeMedia({ width: WINDOW_TIER_BREAKPOINT_PX - 1 })
    expect(tier.value).toBe('mobile')
  })
})
