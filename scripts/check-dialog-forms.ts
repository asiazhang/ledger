#!/usr/bin/env bun

import { readFileSync } from "node:fs";
import { relative } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { walkTextFiles } from "./gate-primitives.ts";
import { parse } from "vue/compiler-sfc";

const VUE_EXTENSIONS: ReadonlySet<string> = new Set([".vue"]);

const RHYTHM_SIZE = "12";

const NODE_ELEMENT = 1;
const PROP_ATTRIBUTE = 6;
const PROP_DIRECTIVE = 7;

interface AstNode {
  type: number;
  tag?: string;
  props?: AstProp[];
  children?: unknown[];
  loc?: { start?: { line?: number } };
}

interface AstProp {
  type: number;
  name?: string;
  value?: { content?: string } | null;
  arg?: { content?: string } | null;
  exp?: { content?: string } | null;
}

export interface Violation {
  file: string;
  line: number;
  tag: string;
}

export interface ParseFailure {
  file: string;
  message: string;
}

function normalizeTag(tag: string): string {
  return tag.replace(/-/g, "").toLowerCase();
}

function propName(p: AstProp): string | null {
  if (p.type === PROP_ATTRIBUTE) return p.name ?? null;
  if (p.type === PROP_DIRECTIVE && p.name === "bind") return p.arg?.content ?? null;
  return null;
}

function isTrueProp(props: AstProp[], name: string): boolean {
  return props.some((p) => {
    if (propName(p) !== name) return false;
    if (p.type === PROP_ATTRIBUTE) return true;
    return (p.exp?.content ?? "true") !== "false";
  });
}

function isRhythmSize(props: AstProp[]): boolean {
  return props.some((p) => {
    if (propName(p) !== "size") return false;
    if (p.type === PROP_ATTRIBUTE) return p.value?.content === RHYTHM_SIZE;
    return p.exp?.content === RHYTHM_SIZE;
  });
}

function isRhythmWrapper(node: AstNode): boolean {
  return (
    normalizeTag(node.tag ?? "") === "nspace" &&
    Array.isArray(node.props) &&
    isTrueProp(node.props, "vertical") &&
    isRhythmSize(node.props)
  );
}

function isFormTag(node: AstNode): boolean {
  return normalizeTag(node.tag ?? "") === "nform";
}

function isFormItemTag(node: AstNode): boolean {
  return normalizeTag(node.tag ?? "") === "nformitem";
}

function findLastIndex(arr: AstNode[], pred: (x: AstNode) => boolean): number {
  for (let i = arr.length - 1; i >= 0; i--) if (pred(arr[i])) return i;
  return -1;
}

export function checkTemplateAst(ast: unknown, file: string, out: Violation[]): void {
  const walk = (raw: unknown, ancestors: AstNode[]): void => {
    const node = raw as AstNode;
    if (node.type !== NODE_ELEMENT) return;
    if (isFormTag(node)) {
      if (Array.isArray(node.props) && isTrueProp(node.props, "inline")) return;
      for (const child of node.children ?? []) walk(child, [...ancestors, node]);
      return;
    }
    if (isFormItemTag(node) && Array.isArray(node.props)) {
      const formIdx = findLastIndex(ancestors, isFormTag);
      const wrapped =
        formIdx >= 0
          ? ancestors.slice(formIdx + 1).some((a) => isRhythmWrapper(a))
          : ancestors.some((a) => isRhythmWrapper(a));
      if (!wrapped) {
        out.push({
          file,
          line: node.loc?.start?.line ?? 0,
          tag: node.tag ?? "NFormItem",
        });
      }
    }
    for (const child of node.children ?? []) walk(child, [...ancestors, node]);
  };
  const root = ast as AstNode;
  for (const child of root.children ?? []) walk(child, []);
}

export function collectVueFiles(root: string): string[] {
  return walkTextFiles(root, "", { extensions: VUE_EXTENSIONS }).map((f) => f.abs);
}

export function checkVueFile(file: string): { violations: Violation[]; failure?: string } {
  const source = readFileSync(file, "utf-8");
  const { descriptor, errors } = parse(source, { filename: file });
  if (errors.length > 0) {
    return { violations: [], failure: errors.map((e) => e.message).join("; ") };
  }
  const template = descriptor.template;
  if (!template) return { violations: [] };
  const violations: Violation[] = [];
  checkTemplateAst(template.ast, file, violations);
  return { violations };
}

function main(): void {
  const scanRoot = process.argv[2] ?? fileURLToPath(new URL("../src", import.meta.url));
  const files = collectVueFiles(scanRoot);
  const violations: Violation[] = [];
  const failures: ParseFailure[] = [];
  for (const file of files) {
    const r = checkVueFile(file);
    violations.push(...r.violations);
    if (r.failure) failures.push({ file, message: r.failure });
  }
  if (violations.length === 0 && failures.length === 0) {
    console.log(
      `✅ 弹窗表单节奏：${files.length} 个 .vue 文件检查通过——非 inline NForm 的表单项均在节奏容器内（ADR-0079 决策 4）`,
    );
    return;
  }
  for (const f of failures) {
    console.error(`✗ 模板解析失败：${relative(process.cwd(), f.file) || f.file} — ${f.message}`);
  }
  for (const v of violations) {
    console.error(
      `✗ ${relative(process.cwd(), v.file) || v.file}:${v.line} — <${v.tag}> 未包在 <NSpace vertical :size="12"> 节奏容器内（NFormItem 默认零行距，表单项与按钮行同包；ADR-0079 决策 4 / issue #804）`,
    );
  }
  process.exit(1);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
