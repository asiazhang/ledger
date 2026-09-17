import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  DEEP_MODULE_BOUNDARIES,
  SCRIPT_INVOCATION,
} from '../scripts/check-frontend-structure.ts'

// 被测对象是仓库工具脚本 scripts/check-frontend-structure.ts 的规则⑦「深模块边界
// 登记表」（issue #1323 / ADR-0118 决策 7）。与守门脚本测试先例同形制（#1158：测试
// 与所测脚本同目录；#1149 起既有 src/__tests__/check-frontend-structure.test.ts 覆盖
// 规则①—⑥与接线核对，本文件只覆盖新增的规则⑦）。脚本以 Bun 运行时执行（ADR-0083）：
// spawnSync('bun') 与门槛调用同款，测的就是门槛路径。按测试决策只测外部可观察结果
// ——进程退出码与输出（ADR-0087 断言强度），不触及脚本内部函数形状；通过位置参数
// 把校验目标指向临时夹具仓库根（[repo-root] [packages-manifest.json]），夹具登记表
// 经 arg2 JSON 注入空表隔离规则①—⑤，规则⑦登记表不可注入（生产 DEEP_MODULE_BOUNDARIES
// 单一事实源，对夹具目录扫描，与规则⑥同形制）。
// （vitest 转换后 import.meta.url 非 file: scheme，取进程 cwd = 仓库根定位脚本）
const script = join(process.cwd(), 'scripts', 'check-frontend-structure.ts')

interface RunResult {
  status: number
  output: string
}

function run(args: string[]): RunResult {
  const r = spawnSync('bun', [script, ...args], { encoding: 'utf8' })
  return { status: r.status ?? -1, output: (r.stdout ?? '') + (r.stderr ?? '') }
}

const tempDirs: string[] = []
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true })
})

/** 规则⑦ 夹具仓库根：其余规则的最小绿基线（空包登记表注入 + 接线宿主 + 规则⑥登记
 *  目录 src/utils）+ 登记模块本体（生产登记表不可注入，夹具绿基线须自足含全部登记项：
 *  src/transaction/useTransactionFilter.ts 与 src/investment/useInstrumentSearch.ts，
 *  #1308 起后者入表；默认创建；omitModule = 「登记模块不存在」靶形）。返回 spawnSync args。 */
function fixtureRepo(opts: { omitModule?: boolean } = {}): string[] {
  const root = mkdtempSync(join(tmpdir(), 'check-frontend-structure-rule7-'))
  tempDirs.push(root)
  writeFileSync(
    join(root, 'pnpm-workspace.yaml'),
    ['packages:', '  # 夹具注释', '  - packages/*', 'allowBuilds:', '  esbuild: true'].join('\n') +
      '\n',
  )
  mkdirSync(join(root, 'packages'), { recursive: true })
  writeFileSync(join(root, 'fixture-manifest.json'), JSON.stringify([], null, 2))
  mkdirSync(join(root, 'scripts'), { recursive: true })
  writeFileSync(join(root, 'scripts', 'check.sh'), `#!/bin/sh\n${SCRIPT_INVOCATION}\n`)
  mkdirSync(join(root, '.github', 'workflows'), { recursive: true })
  writeFileSync(
    join(root, '.github', 'workflows', 'build.yml'),
    `jobs:\n  frontend:\n    steps:\n      - name: 前端结构守门检查\n        run: ${SCRIPT_INVOCATION}\n`,
  )
  mkdirSync(join(root, 'src', 'utils'), { recursive: true })
  if (!opts.omitModule) {
    mkdirSync(join(root, 'src', 'transaction'), { recursive: true })
    writeFileSync(
      join(root, 'src', 'transaction', 'useTransactionFilter.ts'),
      'export const useTransactionFilter = () => ({})\n',
    )
    mkdirSync(join(root, 'src', 'investment'), { recursive: true })
    writeFileSync(
      join(root, 'src', 'investment', 'useInstrumentSearch.ts'),
      'export const useInstrumentSearch = () => ({})\n',
    )
  }
  return [root, join(root, 'fixture-manifest.json')]
}

/** 向夹具写入源文件（路径相对夹具根，posix 分隔） */
function writeSource(root: string, rel: string, source: string): void {
  const abs = join(root, ...rel.split('/'))
  mkdirSync(abs.slice(0, abs.lastIndexOf('/')), { recursive: true })
  writeFileSync(abs, source)
}

describe('规则⑦：深模块边界登记表（#1323 / ADR-0118 决策 7）', () => {
  it('删除规则登记项即变红：登记表与已固化边界全等（TransactionFilter → src/views；#1308 起 useInstrumentSearch → src/investment）', () => {
    expect(DEEP_MODULE_BOUNDARIES).toEqual([
      {
        module: 'src/transaction/useTransactionFilter.ts',
        allowedConsumers: ['src/views'],
        note: expect.any(String),
      },
      {
        module: 'src/investment/useInstrumentSearch.ts',
        allowedConsumers: ['src/investment'],
        note: expect.any(String),
      },
    ])
  })

  it('白名单内消费绿（src/views 消费面：TransactionsView / ReportsView 形态）', () => {
    const args = fixtureRepo()
    writeSource(
      args[0] as string,
      'src/views/TransactionsView.vue',
      "<script setup lang=\"ts\">\nimport { useTransactionFilter } from '@/transaction/useTransactionFilter'\n</script>\n",
    )
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('白名单外消费即红：报告消费方文件、行号、被消费模块与白名单（失败信息可读）', () => {
    const args = fixtureRepo()
    const root = args[0] as string
    writeSource(
      root,
      'src/components/FilterPanel.vue',
      "<script setup lang=\"ts\">\nimport { useTransactionFilter } from '@/transaction/useTransactionFilter'\n</script>\n",
    )
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('深模块边界')
    expect(r.output).toContain('src/components/FilterPanel.vue:2')
    expect(r.output).toContain('src/transaction/useTransactionFilter.ts')
    expect(r.output).toContain('src/views')
  })

  it('相对路径形态同样命中（解析落点比对，非仅 @/ 别名文本）', () => {
    const args = fixtureRepo()
    writeSource(
      args[0] as string,
      'src/components/Drilldown.vue',
      "import { useTransactionFilter } from '../transaction/useTransactionFilter'\nexport { useTransactionFilter }\n",
    )
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('深模块边界')
    expect(r.output).toContain('src/components/Drilldown.vue:1')
  })

  it('测试文件消费放行（单测引用被测对象是天然形态，白名单表达生产消费面）', () => {
    const args = fixtureRepo()
    writeSource(
      args[0] as string,
      'src/__tests__/useTransactionFilter.test.ts',
      "import { useTransactionFilter } from '@/transaction/useTransactionFilter'\nit('smoke', () => {})\n",
    )
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('注释中的 import 形态不误报（复用注释掩码机制）', () => {
    const args = fixtureRepo()
    writeSource(
      args[0] as string,
      'src/components/Legacy.vue',
      "<script setup lang=\"ts\">\n// import { useTransactionFilter } from '@/transaction/useTransactionFilter'\n</script>\n",
    )
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('登记模块文件不存在即红（模块改名/删除后拒绝规则静默失效，同规则⑥形制）', () => {
    const r = run(fixtureRepo({ omitModule: true }))
    expect(r.status).toBe(1)
    expect(r.output).toContain('深模块边界')
    expect(r.output).toContain('登记模块不存在')
  })
})
