import { describe, expect, it } from 'vitest'
import { effectScope } from 'vue'
import { setFakeMedia } from './helpers/media-mock'
import { useInputMode, type InputMode } from '@/composables/useInputMode'

/**
 * 输入轴（Input Mode）模块测试（issue #841，ADR-0088 决策 6 / 词汇表「输入轴」）：
 * composable 只锁「hover / pointer 信号 → 触控轴 / 指针轴」纯映射——hover 与
 * pointer 的组合矩阵与实时换档；交互形态断言（快捷键退役渲染等）不上浮到本层。
 * 换档一律经媒体查询测试接缝（helpers/media-mock）。
 */

/** 在指定 hover / pointer 信号下取输入轴（effectScope 内实例化）。 */
function modeAt(hover: 'hover' | 'none', pointer: 'fine' | 'coarse'): InputMode {
  setFakeMedia({ hover, pointer })
  return effectScope().run(() => useInputMode())!.value
}

describe('useInputMode 信号 → 输入轴纯映射（hover / pointer 组合矩阵）', () => {
  it.each([
    ['可悬停 + 精细主指针（桌面鼠标 / 触控板）→ 指针轴', 'hover', 'fine', 'pointer'],
    ['不可悬停 + 粗糙主指针（Android 手机 / 平板）→ 触控轴', 'none', 'coarse', 'touch'],
    ['可悬停 + 粗糙主指针 → 触控轴（主指针粗糙即触控信号）', 'hover', 'coarse', 'touch'],
    ['不可悬停 + 精细主指针 → 触控轴（不可悬停即触控信号）', 'none', 'fine', 'touch'],
  ])('%s', (_label, hover, pointer, expected) => {
    expect(modeAt(hover as 'hover' | 'none', pointer as 'fine' | 'coarse')).toBe(expected)
  })
})

describe('useInputMode 实时换档（媒体查询 change 驱动）', () => {
  it('同一实例随 hover 信号重编程在两轴间往返翻转', () => {
    setFakeMedia({ hover: 'hover', pointer: 'fine' })
    const mode = effectScope().run(() => useInputMode())!
    expect(mode.value).toBe('pointer')

    setFakeMedia({ hover: 'none' })
    expect(mode.value).toBe('touch')

    setFakeMedia({ hover: 'hover' })
    expect(mode.value).toBe('pointer')
  })

  it('pointer 信号翻转同样驱动换档（两信号任一变化即重判）', () => {
    setFakeMedia({ hover: 'hover', pointer: 'fine' })
    const mode = effectScope().run(() => useInputMode())!
    expect(mode.value).toBe('pointer')

    setFakeMedia({ pointer: 'coarse' })
    expect(mode.value).toBe('touch')
  })
})
