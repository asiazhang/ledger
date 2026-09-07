#!/usr/bin/env bun
// 弹窗表单行距节奏守门（issue #804 / ADR-0079 决策 4）：非 inline 的 NForm 内，
// NFormItem 必须包在 <NSpace vertical :size="12"> 节奏容器内（表单项与按钮行同包，
// 不依赖组件库默认零行距裸排）。#699 曾修保单/实物资产建档/商户编辑三处漏网，
// #804 账户编辑弹窗同病复发后清零存量并立此门槛：违例在合入前被拦截，不再靠模仿维持。
// 12 硬编码：行距是 ADR-0079 决策定稿值，改动须回 ADR 并同步本脚本（与档位像素同一纪律）；
// 不设注释豁免通道，逃逸 = 显式回 ADR 讨论后改脚本。
// 检测范围：.vue <template> 静态模板（含 kebab-case 等价标签形）；render 函数形态不在
// 检测范围（仓库无 NForm render 先例）。内联卡片表单（<NForm inline>）不在弹窗节奏
// 约定范围，整树豁免。
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-dialog-forms.ts`。
// 默认扫描本仓库 src；测试可传位置参数指向夹具目录：bun scripts/check-dialog-forms.ts [scan-root]
// 挂载于 scripts/check.sh 质量门槛序列与 CI。

import { readdirSync, readFileSync } from 'node:fs'
import { join, relative } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'
import { parse } from 'vue/compiler-sfc'

/** 节奏容器唯一档位（ADR-0079 决策 4 定稿值）：改动须回 ADR 并同步此值 */
const RHYTHM_SIZE = '12'

// @vue/compiler-core NodeTypes 数值（经 vue/compiler-sfc 的 AST 消费；避免直引传递依赖）：
// ELEMENT = 1、ATTRIBUTE = 6、DIRECTIVE = 7
const NODE_ELEMENT = 1
const PROP_ATTRIBUTE = 6
const PROP_DIRECTIVE = 7

/** 模板 AST 节点的最小结构面（只取本守门需要的字段） */
interface AstNode {
  type: number
  tag?: string
  props?: AstProp[]
  children?: unknown[]
  loc?: { start?: { line?: number } }
}

/** 属性/指令节点的最小结构面：Attribute 有 value，Directive 有 arg 与 exp */
interface AstProp {
  type: number
  name?: string
  value?: { content?: string } | null
  arg?: { content?: string } | null
  exp?: { content?: string } | null
}

/** 守门违例：文件 + 行号 + 违例标签 */
export interface Violation {
  file: string
  line: number
  tag: string
}

/** 单文件模板解析失败（SFC 语法错误）：同样拦截合入 */
export interface ParseFailure {
  file: string
  message: string
}

function normalizeTag(tag: string): string {
  return tag.replace(/-/g, '').toLowerCase()
}

/** 属性的声明名：静态属性取 name；只认 v-bind（:foo），v-model 等不参与判定 */
function propName(p: AstProp): string | null {
  if (p.type === PROP_ATTRIBUTE) return p.name ?? null
  if (p.type === PROP_DIRECTIVE && p.name === 'bind') return p.arg?.content ?? null
  return null
}

/** 布尔属性的取值：静态存在即 true；`:foo="false"` 显式为假 */
function isTrueProp(props: AstProp[], name: string): boolean {
  return props.some((p) => {
    if (propName(p) !== name) return false
    if (p.type === PROP_ATTRIBUTE) return true
    return (p.exp?.content ?? 'true') !== 'false'
  })
}

/** size 是否为节奏档位字面量（静态 size="12" 或 :size="12"；绑定表达式一律视为不合规） */
function isRhythmSize(props: AstProp[]): boolean {
  return props.some((p) => {
    if (propName(p) !== 'size') return false
    if (p.type === PROP_ATTRIBUTE) return p.value?.content === RHYTHM_SIZE
    return p.exp?.content === RHYTHM_SIZE
  })
}

/** 节奏容器判定：NSpace + vertical + size 12 */
function isRhythmWrapper(node: AstNode): boolean {
  return (
    normalizeTag(node.tag ?? '') === 'nspace' &&
    Array.isArray(node.props) &&
    isTrueProp(node.props, 'vertical') &&
    isRhythmSize(node.props)
  )
}

function isFormTag(node: AstNode): boolean {
  return normalizeTag(node.tag ?? '') === 'nform'
}

function isFormItemTag(node: AstNode): boolean {
  return normalizeTag(node.tag ?? '') === 'nformitem'
}

function findLastIndex(arr: AstNode[], pred: (x: AstNode) => boolean): number {
  for (let i = arr.length - 1; i >= 0; i--) if (pred(arr[i])) return i
  return -1
}

/**
 * 检查单个模板 AST：每个 NFormItem 必须被节奏容器（NSpace vertical size 12）覆盖——
 * 有同文件 NForm 祖先时须夹在表单项与所属 NForm 之间；无同文件 NForm（父级装配或
 * 独立作行）时须在本文件祖先链上兜底。ancestors 为根到当前节点父级的链。
 * 纯函数导出供单测（src/__tests__/check-dialog-forms.test.ts）。
 */
export function checkTemplateAst(ast: unknown, file: string, out: Violation[]): void {
  const walk = (raw: unknown, ancestors: AstNode[]): void => {
    const node = raw as AstNode
    if (node.type !== NODE_ELEMENT) return
    if (isFormTag(node)) {
      // inline 表单不在弹窗节奏约定范围：整树豁免（子树不再继续检查）
      if (Array.isArray(node.props) && isTrueProp(node.props, 'inline')) return
      for (const child of node.children ?? []) walk(child, [...ancestors, node])
      return
    }
    if (isFormItemTag(node) && Array.isArray(node.props)) {
      // 最近一个所属 NForm；inline NForm 已整树豁免，链上出现的 NForm 均为非 inline。
      // 两种形态：① 有同文件 NForm 祖先 → 节奏容器必须夹在表单项与所属 NForm 之间；
      // ② 无同文件 NForm（表单项由父级弹窗装配，或独立作行，如 PolicyAgreementFields /
      // RestoreConfirmModal）→ 组件边界外无法看，本文件整条祖先链须有节奏容器兜底。
      const formIdx = findLastIndex(ancestors, isFormTag)
      const wrapped =
        formIdx >= 0
          ? ancestors.slice(formIdx + 1).some((a) => isRhythmWrapper(a))
          : ancestors.some((a) => isRhythmWrapper(a))
      if (!wrapped) {
        out.push({
          file,
          line: node.loc?.start?.line ?? 0,
          tag: node.tag ?? 'NFormItem',
        })
      }
    }
    for (const child of node.children ?? []) walk(child, [...ancestors, node])
  }
  const root = ast as AstNode
  for (const child of root.children ?? []) walk(child, [])
}

/** 递归收集扫描根下全部 .vue 文件（按名排序保证输出稳定） */
export function collectVueFiles(root: string): string[] {
  const out: string[] = []
  const visit = (dir: string): void => {
    for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
      a.name.localeCompare(b.name),
    )) {
      const full = join(dir, entry.name)
      if (entry.isDirectory()) visit(full)
      else if (entry.name.endsWith('.vue')) out.push(full)
    }
  }
  visit(root)
  return out
}

/** 检查单个 .vue 文件：返回节奏违例与模板解析失败（如有） */
export function checkVueFile(file: string): { violations: Violation[]; failure?: string } {
  const source = readFileSync(file, 'utf-8')
  const { descriptor, errors } = parse(source, { filename: file })
  if (errors.length > 0) {
    return { violations: [], failure: errors.map((e) => e.message).join('; ') }
  }
  const template = descriptor.template
  if (!template) return { violations: [] }
  const violations: Violation[] = []
  checkTemplateAst(template.ast, file, violations)
  return { violations }
}

function main(): void {
  const scanRoot = process.argv[2] ?? fileURLToPath(new URL('../src', import.meta.url))
  const files = collectVueFiles(scanRoot)
  const violations: Violation[] = []
  const failures: ParseFailure[] = []
  for (const file of files) {
    const r = checkVueFile(file)
    violations.push(...r.violations)
    if (r.failure) failures.push({ file, message: r.failure })
  }
  if (violations.length === 0 && failures.length === 0) {
    console.log(
      `✅ 弹窗表单节奏：${files.length} 个 .vue 文件检查通过——非 inline NForm 的表单项均在节奏容器内（ADR-0079 决策 4）`,
    )
    return
  }
  for (const f of failures) {
    console.error(`✗ 模板解析失败：${relative(process.cwd(), f.file) || f.file} — ${f.message}`)
  }
  for (const v of violations) {
    console.error(
      `✗ ${relative(process.cwd(), v.file) || v.file}:${v.line} — <${v.tag}> 未包在 <NSpace vertical :size="12"> 节奏容器内（NFormItem 默认零行距，表单项与按钮行同包；ADR-0079 决策 4 / issue #804）`,
    )
  }
  process.exit(1)
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的检查函数
// （与 scripts/check-i18n-keys.ts 同一惯法）。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
