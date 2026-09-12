import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { PACKAGES, SCRIPT_INVOCATION } from '../../scripts/check-frontend-structure.ts'

// 被测对象是仓库工具脚本 scripts/check-frontend-structure.ts（前端 workspace 结构
// 守门，issue #1149）。脚本以 Bun 运行时执行（ADR-0083）：spawnSync('bun') 与门槛
// 调用同款，测的就是门槛路径。按测试决策只测外部可观察结果——进程退出码与输出；
// 通过位置参数把校验目标指向临时夹具仓库根（[repo-root] [packages-manifest.json]），
// 夹具登记表经 arg2 JSON 注入（生产路径不传，PACKAGES 单一事实源不变）。
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

/** 夹具登记表条目（与 PACKAGES 同形的最小条目） */
interface FixtureEntry {
  name: string
  dir: string
  deps: string[]
  testSupport?: boolean
  note: string
}

/** 建夹具仓库根：workspace yaml（glob 声明）+ 空成员目录 + 两个接线宿主。
 *  与真实仓库同构的最小绿基线；opts 覆盖缺口场景。 */
function fixtureRepo(opts: {
  /** 是否写 pnpm-workspace.yaml 的 packages/* 声明（缺省写；false = 缺声明场景） */
  yamlGlob?: boolean
  /** packages/ 下预创建的成员目录名（不写 package.json——规则①删除即变红②靶形） */
  memberDirs?: string[]
  /** 接线宿主缺位场景：跳过 check.sh / build.yml 的接线行 */
  omitWiring?: ('check.sh' | 'build.yml')[]
  /** 是否写夹具登记表 JSON（返回参数含 manifest 路径） */
  manifest?: FixtureEntry[]
}): string[] {
  const root = mkdtempSync(join(tmpdir(), 'check-frontend-structure-'))
  tempDirs.push(root)
  const yamlLines = ['# 夹具', 'allowBuilds:', '  esbuild: true']
  if (opts.yamlGlob !== false) {
    yamlLines.unshift('packages:', '  # 夹具注释', '  - packages/*')
  }
  writeFileSync(join(root, 'pnpm-workspace.yaml'), yamlLines.join('\n') + '\n')
  mkdirSync(join(root, 'packages'), { recursive: true })
  for (const dir of opts.memberDirs ?? []) {
    mkdirSync(join(root, 'packages', dir), { recursive: true })
  }
  if (!opts.omitWiring?.includes('check.sh')) {
    mkdirSync(join(root, 'scripts'), { recursive: true })
    writeFileSync(join(root, 'scripts', 'check.sh'), `#!/bin/sh\n${SCRIPT_INVOCATION}\n`)
  }
  if (!opts.omitWiring?.includes('build.yml')) {
    mkdirSync(join(root, '.github', 'workflows'), { recursive: true })
    writeFileSync(
      join(root, '.github', 'workflows', 'build.yml'),
      `jobs:\n  frontend:\n    steps:\n      - name: 前端结构守门检查\n        run: ${SCRIPT_INVOCATION}\n`,
    )
  }
  const args = [root]
  if (opts.manifest) {
    const manifestPath = join(root, 'fixture-manifest.json')
    writeFileSync(manifestPath, JSON.stringify(opts.manifest, null, 2))
    args.push(manifestPath)
  }
  return args
}

/** 写成员包清单（name 字段与登记表全等，deps 按给定值） */
function writePackageManifest(root: string, dir: string, pkg: Record<string, unknown>): void {
  writeFileSync(join(root, dir, 'package.json'), JSON.stringify(pkg, null, 2))
}

describe('check-frontend-structure（前端 workspace 结构守门）', () => {
  it('真实仓库默认通过：登记成员与磁盘全等的骨架绿（#1149 验收；#1150 起有首个成员）', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain('前端结构守门')
  })

  it('夹具最小绿：glob 声明 + 空成员 + 接线齐全', () => {
    // #1150 起 PACKAGES 非空：无 manifest 的夹具运行会拿生产登记表对夹具根校验、
    // 触发「清单漂移」假红，故最小绿夹具显式注入空登记表（注入即完全替代）。
    const r = run(fixtureRepo({ manifest: [] }))
    expect(r.status).toBe(0)
    expect(r.output).toContain('成员登记 0 个')
  })

  describe('规则①：成员登记（磁盘 ↔ PACKAGES 双向全等）', () => {
    it('删除即变红②：packages/ 下新建成员目录不登记即红', () => {
      const r = run(fixtureRepo({ memberDirs: ['unregistered'] }))
      expect(r.status).toBe(1)
      expect(r.output).toContain('成员登记')
      expect(r.output).toContain('packages/unregistered')
    })

    it('登记表与磁盘双向：登记目录不存在即红（清单漂移 fail loud）', () => {
      const entry: FixtureEntry = {
        name: '@ledger/ui',
        dir: 'packages/ui',
        deps: [],
        note: '夹具',
      }
      const r = run(fixtureRepo({ manifest: [entry] }))
      expect(r.status).toBe(1)
      expect(r.output).toContain('清单漂移')
      expect(r.output).toContain('packages/ui')
    })

    it('登记名与包清单 name 不一致即红', () => {
      const entry: FixtureEntry = {
        name: '@ledger/ui',
        dir: 'packages/ui',
        deps: [],
        note: '夹具',
      }
      const args = fixtureRepo({ memberDirs: ['ui'], manifest: [entry] })
      const root = args[0] as string
      writePackageManifest(root, 'packages/ui', { name: '@ledger/other' })
      const r = run(args)
      expect(r.status).toBe(1)
      expect(r.output).toContain('不一致')
    })

    it('pnpm-workspace.yaml 缺 packages/* 声明即红（workspace 骨架）', () => {
      const r = run(fixtureRepo({ yamlGlob: false }))
      expect(r.status).toBe(1)
      expect(r.output).toContain('未以 glob 声明')
    })
  })

  describe('规则②：包依赖方向（方向表逐票补充）', () => {
    /** 建一对已登记成员 a、b（a 依赖 b 的方向由 deps 与包清单共同决定） */
    function twoPackages(aDeps: string[], aManifestDeps: Record<string, string>): string[] {
      const manifest: FixtureEntry[] = [
        { name: '@ledger/a', dir: 'packages/a', deps: aDeps, note: '夹具 a' },
        { name: '@ledger/b', dir: 'packages/b', deps: [], note: '夹具 b' },
      ]
      const args = fixtureRepo({ memberDirs: ['a', 'b'], manifest })
      const root = args[0] as string
      writePackageManifest(root, 'packages/a', {
        name: '@ledger/a',
        dependencies: aManifestDeps,
      })
      writePackageManifest(root, 'packages/b', { name: '@ledger/b' })
      return args
    }

    it('依赖未登记包即红', () => {
      // 登记表只含 a（ghost 缺席），单独核「依赖未登记包」靶形
      const manifest: FixtureEntry[] = [
        { name: '@ledger/a', dir: 'packages/a', deps: [], note: '夹具 a' },
      ]
      const args = fixtureRepo({ memberDirs: ['a'], manifest })
      const root = args[0] as string
      writePackageManifest(root, 'packages/a', {
        name: '@ledger/a',
        dependencies: { '@ledger/ghost': 'workspace:*' },
      })
      const r = run(args)
      expect(r.status).toBe(1)
      expect(r.output).toContain('依赖未登记包')
    })

    it('依赖不在方向表内即红（方向表为空表）', () => {
      const r = run(twoPackages([], { '@ledger/b': 'workspace:*' }))
      expect(r.status).toBe(1)
      expect(r.output).toContain('不在其方向表内')
    })

    it('方向表放行该边即绿', () => {
      const r = run(twoPackages(['@ledger/b'], { '@ledger/b': 'workspace:*' }))
      expect(r.status).toBe(0)
    })

    it('devDependencies 同受方向表约束（包间测试边也是真实边界）', () => {
      const args = twoPackages([], {})
      const root = args[0] as string
      writePackageManifest(root, 'packages/a', {
        name: '@ledger/a',
        devDependencies: { '@ledger/b': 'workspace:*' },
      })
      const r = run(args)
      expect(r.status).toBe(1)
      expect(r.output).toContain('不在其方向表内')
    })
  })

  describe('规则③：跨包引用形态（@ledger/* 包名，禁 @/ 与相对穿越）', () => {
    /** 建已登记成员 a + 其源文件内容 */
    function packageWithSource(source: string): string[] {
      const manifest: FixtureEntry[] = [
        { name: '@ledger/a', dir: 'packages/a', deps: [], note: '夹具 a' },
      ]
      const args = fixtureRepo({ memberDirs: ['a'], manifest })
      const root = args[0] as string
      mkdirSync(join(root, 'packages/a/src'), { recursive: true })
      writeFileSync(join(root, 'packages/a/src/x.ts'), source)
      writePackageManifest(root, 'packages/a', { name: '@ledger/a' })
      return args
    }

    it('@/ 别名即红，定位文件与行号', () => {
      const r = run(packageWithSource("import { helper } from '@/utils/format'\nexport { helper }\n"))
      expect(r.status).toBe(1)
      expect(r.output).toContain('@/ 别名')
      expect(r.output).toContain('packages/a/src/x.ts:1')
    })

    it('相对路径穿越包边界即红', () => {
      const r = run(
        packageWithSource("import { x } from '../../other/src/y'\nexport { x }\n"),
      )
      expect(r.status).toBe(1)
      expect(r.output).toContain('相对路径穿越包边界')
      expect(r.output).toContain('packages/a/src/x.ts:1')
    })

    it('包内相对引用绿', () => {
      const r = run(packageWithSource("import { x } from './y'\nexport { x }\n"))
      expect(r.status).toBe(0)
    })

    it('注释中的 import 形态不误报', () => {
      const r = run(packageWithSource("// import { helper } from '@/utils/format'\nexport {}\n"))
      expect(r.status).toBe(0)
    })
  })

  describe('规则④：深导入禁令（跨包引用必须命中 exports 入口）', () => {
    /** 建 a → b 深导入夹具：b 的 exports 按给定值写，a 源文件给给定 import */
    function deepImportFixture(
      importSpec: string,
      exportsField: Record<string, unknown> | string,
    ): string[] {
      const manifest: FixtureEntry[] = [
        { name: '@ledger/a', dir: 'packages/a', deps: ['@ledger/b'], note: '夹具 a' },
        { name: '@ledger/b', dir: 'packages/b', deps: [], note: '夹具 b' },
      ]
      const args = fixtureRepo({ memberDirs: ['a', 'b'], manifest })
      const root = args[0] as string
      mkdirSync(join(root, 'packages/a/src'), { recursive: true })
      writeFileSync(join(root, 'packages/a/src/x.ts'), `import { y } from '${importSpec}'\nexport { y }\n`)
      writePackageManifest(root, 'packages/a', {
        name: '@ledger/a',
        dependencies: { '@ledger/b': 'workspace:*' },
      })
      writePackageManifest(root, 'packages/b', { name: '@ledger/b', exports: exportsField })
      return args
    }

    it('深导入命中 exports 精确键绿', () => {
      const r = run(
        deepImportFixture('@ledger/b/sub', { '.': './src/index.ts', './sub': './src/sub.ts' }),
      )
      expect(r.status).toBe(0)
    })

    it('深导入未命中 exports 入口即红', () => {
      const r = run(
        deepImportFixture('@ledger/b/other', { '.': './src/index.ts', './sub': './src/sub.ts' }),
      )
      expect(r.status).toBe(1)
      expect(r.output).toContain('深导入禁令')
      expect(r.output).toContain('@ledger/b/other')
    })

    it('exports 字符串形态（仅暴露 .）不接受深导入', () => {
      const r = run(deepImportFixture('@ledger/b/sub', './src/index.ts'))
      expect(r.status).toBe(1)
      expect(r.output).toContain('深导入禁令')
    })

    it('包名整引（无子路径）不受 exports 约束绿', () => {
      const r = run(deepImportFixture('@ledger/b', { './sub': './src/sub.ts' }))
      expect(r.status).toBe(0)
    })

    it('exports ./* 通配放行目录级深导入绿', () => {
      const r = run(deepImportFixture('@ledger/b/sub', { './*': './src/*.ts' }))
      expect(r.status).toBe(0)
    })
  })

  describe('接线核对（删除即变红①）', () => {
    it('check.sh 删除调用行即红', () => {
      const r = run(fixtureRepo({ omitWiring: ['check.sh'] }))
      expect(r.status).toBe(1)
      expect(r.output).toContain('scripts/check.sh')
      expect(r.output).toContain('接线核对')
    })

    it('build.yml frontend job 删除调用行即红', () => {
      const r = run(fixtureRepo({ omitWiring: ['build.yml'] }))
      expect(r.status).toBe(1)
      expect(r.output).toContain('.github/workflows/build.yml')
      expect(r.output).toContain('接线核对')
    })

    it('接线行注释掉即红（非注释行才算接线）', () => {
      const args = fixtureRepo({ omitWiring: ['check.sh'] })
      const root = args[0] as string
      mkdirSync(join(root, 'scripts'), { recursive: true })
      writeFileSync(
        join(root, 'scripts', 'check.sh'),
        `#!/bin/sh\n# ${SCRIPT_INVOCATION}\n`,
      )
      const r = run(args)
      expect(r.status).toBe(1)
      expect(r.output).toContain('接线核对')
    })
  })

  describe('夹具登记表注入接缝（arg2）', () => {
    it('畸形 JSON fail loud 不栈爆', () => {
      const args = fixtureRepo({})
      const manifestPath = join(args[0] as string, 'broken.json')
      writeFileSync(manifestPath, '{not json')
      const r = run([args[0] as string, manifestPath])
      expect(r.status).toBe(1)
      expect(r.output).toContain('夹具登记表不可解析')
    })

    it('非数组 JSON fail loud', () => {
      const args = fixtureRepo({})
      const manifestPath = join(args[0] as string, 'not-array.json')
      writeFileSync(manifestPath, '{"name": "x"}')
      const r = run([args[0] as string, manifestPath])
      expect(r.status).toBe(1)
      expect(r.output).toContain('须为 JSON 数组')
    })

    it('PACKAGES 生产登记表与已落位包全等（#1150/#1151/#1152/#1153/#1155 抽包落位）', () => {
      expect(PACKAGES).toEqual([
        {
          name: '@ledger/types',
          dir: 'packages/types',
          deps: [],
          note: expect.any(String),
        },
        {
          name: '@ledger/api',
          dir: 'packages/api',
          deps: ['@ledger/types'],
          note: expect.any(String),
        },
        {
          name: '@ledger/storage',
          dir: 'packages/storage',
          deps: [],
          note: expect.any(String),
        },
        {
          name: '@ledger/i18n',
          dir: 'packages/i18n',
          deps: ['@ledger/storage'],
          note: expect.any(String),
        },
        {
          name: '@ledger/money',
          dir: 'packages/money',
          deps: ['@ledger/types', '@ledger/i18n'],
          note: expect.any(String),
        },
        {
          name: '@ledger/test-support',
          dir: 'packages/test-support',
          deps: ['@ledger/types'],
          testSupport: true,
          note: expect.any(String),
        },
      ])
    })
  })

  describe('规则⑤：测试支持纯净性（testSupport 包仅 devDependency 消费，#1152）', () => {
    /** 建含测试支持包的夹具：登记表注入（testSupport 标志），成员与根包清单自足。
     *  返回完整 args（含 manifest 路径）；memberManifest 覆写成员清单（规则⑤自身
     *  dependencies 用例）。 */
    function testSupportArgs(
      consumerManifest: Record<string, unknown>,
      memberManifest: Record<string, unknown> = { name: '@ledger/ts', devDependencies: {} },
    ): string[] {
      const args = fixtureRepo({
        manifest: [
          { name: '@ledger/ts', dir: 'packages/ts', deps: [], testSupport: true, note: '夹具' },
        ],
        memberDirs: ['ts'],
      })
      const root = args[0] as string
      writePackageManifest(root, 'packages/ts', memberManifest)
      writePackageManifest(root, '.', consumerManifest)
      return args
    }

    it('删除即变红：根包 dependencies 出现测试支持包即红（生产依赖图零测试支持）', () => {
      const r = run(testSupportArgs({ dependencies: { '@ledger/ts': 'workspace:*' } }))
      expect(r.status).toBe(1)
      expect(r.output).toContain('测试支持纯净性')
      expect(r.output).toContain('根包（应用壳）')
      expect(r.output).toContain('devDependencies')
    })

    it('peerDependencies 出现测试支持包同样红（dev 是唯一合法通道）', () => {
      const r = run(testSupportArgs({ peerDependencies: { '@ledger/ts': 'workspace:*' } }))
      expect(r.status).toBe(1)
      expect(r.output).toContain('测试支持纯净性')
    })

    it('测试支持包自身 dependencies 非空即红（零生产依赖）', () => {
      const r = run(
        testSupportArgs(
          {},
          {
            name: '@ledger/ts',
            dependencies: { 'some-runtime': '^1.0.0' },
            devDependencies: {},
          },
        ),
      )
      expect(r.status).toBe(1)
      expect(r.output).toContain('自身 dependencies 非空')
    })

    it('经 devDependencies 消费且自身零生产依赖即绿', () => {
      const r = run(testSupportArgs({ devDependencies: { '@ledger/ts': 'workspace:*' } }))
      expect(r.status).toBe(0)
    })
  })
})
