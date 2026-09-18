import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { repoRoot } from './has-command-line.test-helper.ts'
import { maskComments } from './ts-comment-mask.ts'

// 被测对象是 TS 侧注释掩码共享模块 scripts/ts-comment-mask.ts（issue #1481）：
// check-frontend-structure.ts（import 说明符扫描）与 check-commands.ts（TS 调用面
// 识别）曾各自消费同一实现却把实现住在结构守门脚本内，本票上收共享模块。
// （仓库根定位消费 scripts/has-command-line.test-helper.ts，#1487 上收唯一定义点）
const read = (rel: string): string => readFileSync(join(repoRoot(), ...rel.split('/')), 'utf8')

describe('maskComments（TS/Vue 注释掩码单源模块语义）', () => {
  it('行注释与块注释掩为等长空白：保留换行与列位（行号稳定）', () => {
    const src = 'a() // note\nx /* blk */ y\n'
    const out = maskComments(src)
    expect(out).toHaveLength(src.length)
    // 换行位置逐列不变
    expect([...out].map((c) => c === '\n')).toEqual([...src].map((c) => c === '\n'))
    const [line0, line1] = out.split('\n')
    expect(line0?.trimEnd()).toBe('a()') // 行注释内容不再出现
    expect(line1?.startsWith('x ')).toBe(true) // 块注释前代码保留
    expect(line1?.endsWith(' y')).toBe(true) // 块注释后代码保留
    expect(line1).not.toContain('blk')
  })

  it('字符串与模板字面量内的 // /* 不误当注释（否则吞掉同行真实代码）', () => {
    const src = 'const s = "http://x"\nconst t = `a /* b */ c`\n'
    expect(maskComments(src)).toBe(src)
  })

  it('正则体内转义的斜杠序列不触发注释掩码，其后的真实代码保留', () => {
    const src = 'const r = /a\\/\\/b/\nconst after = 1\n'
    expect(maskComments(src)).toBe(src)
  })
})

// 「删除即变红」负向判据（ADR-0087 断言强度 / 接线型守门）：本票的核心价值是
// maskComments 只有一份实现、两个门禁脚本消费同一出口。任一消费者删掉共享模块
// import 并回退为本地副本（或共享模块定义被移除），下面的断言至少一条变红；
// 只在真实仓行为层（exit code / output）保持绿无法单独锚定「同一出口」这条接线。
describe('单源收敛：maskComments 只有一份实现（issue #1481）', () => {
  const consumers = ['check-frontend-structure.ts', 'check-commands.ts'] as const

  for (const file of consumers) {
    it(`${file} 从共享模块消费 maskComments 且不本地重定义`, () => {
      const src = read(`scripts/${file}`)
      expect(src).toMatch(
        /import\s*\{[^}]*\bmaskComments\b[^}]*\}\s*from\s*'\.\/ts-comment-mask\.ts'/,
      )
      // 本地重定义（含函数表达式 / 箭头函数副本）同样即红
      expect(src).not.toMatch(/(?:function|const|let|var)\s+maskComments\b/)
    })
  }

  it('共享模块导出 maskComments（两消费者导入的同一出口）', () => {
    expect(read('scripts/ts-comment-mask.ts')).toMatch(/export\s+function\s+maskComments\b/)
  })
})
