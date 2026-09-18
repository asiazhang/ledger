import { afterAll, describe, expect, it } from 'vitest'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { gateScript, runGateScript } from './run-gate-script.test-helper.ts'

// 被测对象是仓库工具脚本 scripts/check-dialog-forms.ts（弹窗表单行距节奏守门）。
// 脚本以 Bun 运行时执行（ADR-0083）：runGateScript 以 spawnSync('bun') 与门槛
// 调用同款拉起，测的就是门槛路径。
// 按测试决策只测外部可观察结果——进程退出码与输出，不测内部函数；
// 通过位置参数把扫描目标指向临时夹具目录（仿 check-i18n-keys.test.ts 先例）。
const script = gateScript('check-dialog-forms.ts')

const tmpDirs: string[] = []

function makeFixture(files: Record<string, string>): string {
  const root = mkdtempSync(join(tmpdir(), 'dialog-forms-'))
  tmpDirs.push(root)
  mkdirSync(root, { recursive: true })
  for (const [name, content] of Object.entries(files)) {
    writeFileSync(join(root, name), content)
  }
  return root
}

function run(scanRoot: string) {
  return runGateScript(script, [scanRoot])
}

afterAll(() => {
  for (const d of tmpDirs) rmSync(d, { recursive: true, force: true })
})

const COMPLIANT = `<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NSpace vertical :size="12">
      <NFormItem label="名称">
        <NInput />
      </NFormItem>
      <NSpace justify="end">
        <NButton>保存</NButton>
      </NSpace>
    </NSpace>
  </NForm>
</template>
`

const BARE_ITEM = `<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NFormItem label="名称">
      <NInput />
    </NFormItem>
  </NForm>
</template>
`

describe('弹窗表单行距节奏守门（check.sh 质量门槛）', () => {
  it('真实仓库默认通过：本仓库全部 .vue 无节奏违例（现行仓库绿）', () => {
    // 无位置参数 = 默认扫描本仓库 src。随 vitest scripts 测试分片进 CI（issue #1473）：
    // 真实仓违例即红，不再只靠本地自觉跑 check.sh。文件计数行证明扫描面真实
    // 覆盖（非零 .vue），守门空转（扫描根失效等）即红。
    const r = runGateScript(script)
    expect(r.status).toBe(0)
    expect(r.output).toMatch(/弹窗表单节奏：[1-9]\d* 个 \.vue 文件检查通过/)
  })

  it('表单项与按钮行同包节奏容器 → 通过', () => {
    const dir = makeFixture({ 'Good.vue': COMPLIANT })
    const { status, output } = run(dir)
    expect(status).toBe(0)
    expect(output).toContain('节奏')
  })

  it('NFormItem 裸排在非 inline NForm 内 → 失败并报文件行号', () => {
    const dir = makeFixture({ 'Bad.vue': BARE_ITEM })
    const { status, output } = run(dir)
    expect(status).toBe(1)
    expect(output).toContain('Bad.vue')
    expect(output).toContain('节奏容器')
  })

  it('inline 表单整树豁免 → 通过', () => {
    const dir = makeFixture({
      'Inline.vue': `<template>
  <NForm inline>
    <NFormItem label="名称">
      <NInput />
    </NFormItem>
  </NForm>
</template>
`,
    })
    const { status } = run(dir)
    expect(status).toBe(0)
  })

  it('包了 NSpace 但 size 非 12 → 失败（档位是 ADR 定稿值）', () => {
    const dir = makeFixture({
      'WrongSize.vue': `<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NSpace vertical :size="8">
      <NFormItem label="名称">
        <NInput />
      </NFormItem>
    </NSpace>
  </NForm>
</template>
`,
    })
    const { status, output } = run(dir)
    expect(status).toBe(1)
    expect(output).toContain('WrongSize.vue')
  })

  it('包了 NSpace 但缺 vertical → 失败', () => {
    const dir = makeFixture({
      'NotVertical.vue': `<template>
  <NForm label-placement="left" :show-feedback="false" size="small">
    <NSpace :size="12">
      <NFormItem label="名称">
        <NInput />
      </NFormItem>
    </NSpace>
  </NForm>
</template>
`,
    })
    const { status, output } = run(dir)
    expect(status).toBe(1)
    expect(output).toContain('NotVertical.vue')
  })

  it('kebab-case 标签等价检出（n-form / n-form-item）', () => {
    const dir = makeFixture({
      'Kebab.vue': BARE_ITEM.replace(/NForm\b/g, 'n-form').replace(/NFormItem/g, 'n-form-item'),
    })
    const { status, output } = run(dir)
    expect(status).toBe(1)
    expect(output).toContain('Kebab.vue')
  })

  it('无同文件 NForm 的表单项（父级装配）由根级节奏容器兜底 → 通过', () => {
    const dir = makeFixture({
      'Standalone.vue': `<template>
  <NSpace vertical :size="12">
    <NFormItem label="名称">
      <NInput />
    </NFormItem>
  </NSpace>
</template>
`,
    })
    const { status } = run(dir)
    expect(status).toBe(0)
  })

  it('无同文件 NForm 且全链无节奏容器 → 失败（父级 NSpace 隔着组件边界管不到内部行距）', () => {
    const dir = makeFixture({
      'StandaloneBare.vue': `<template>
  <div>
    <NFormItem label="名称">
      <NInput />
    </NFormItem>
  </div>
</template>
`,
    })
    const { status, output } = run(dir)
    expect(status).toBe(1)
    expect(output).toContain('StandaloneBare.vue')
  })

  it('模板解析失败 → 失败（坏模板在门槛即拦截）', () => {
    const dir = makeFixture({
      'Broken.vue': `<template>
  <div>
    <unclosed>
  </div>
</template>
`,
    })
    const { status, output } = run(dir)
    expect(status).toBe(1)
    expect(output).toContain('解析失败')
  })
})
