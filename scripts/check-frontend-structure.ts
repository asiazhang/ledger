#!/usr/bin/env bun

import { existsSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { maskComments, readDirEntries, walkTextFiles, type WalkedFile } from "./gate-primitives.ts";

export interface PackageEntry {
  name: string;
  dir: string;
  deps: readonly string[];
  testSupport?: boolean;
  note: string;
}

export const PACKAGES: readonly PackageEntry[] = [
  {
    name: "@ledger/types",
    dir: "packages/types",
    deps: [],
    note: "纯类型包（issue #1150）：零依赖叶子，方向表恒空——类型层不依赖任何包；金额展示接缝归 @ledger/money（#1153）",
  },
  {
    name: "@ledger/api",
    dir: "packages/api",
    deps: ["@ledger/types"],
    note: "IPC 传输层包（issue #1155）：统一 invoke 封装与 globalBusy 唯一生产消费方归位同包，@ledger 范围内只依赖 @ledger/types；不依赖 composables / stores / components / views",
  },
  {
    name: "@ledger/storage",
    dir: "packages/storage",
    deps: [],
    note: "存储接缝包（issue #1151）：依赖图唯一真叶子，方向表恒空——localStorage 读写的单一收口，不依赖任何包",
  },
  {
    name: "@ledger/i18n",
    dir: "packages/i18n",
    deps: ["@ledger/storage"],
    note: "界面语言包（issue #1151 / ADR-0049）：依赖存储底座 @ledger/storage 单向成边，locales 文案资源随包走；money / errors 等上层消费方依赖本包，utils ↔ i18n 双向环消失",
  },
  {
    name: "@ledger/money",
    dir: "packages/money",
    deps: ["@ledger/types", "@ledger/i18n"],
    note: "金额展示与换算包（issue #1153 / spec #1148）：金额/价格/数量格式化与元/分换算的单一出口，依赖 @ledger/types 与 @ledger/i18n；金额隐私开关的水合与持久化仍归应用设置 store",
  },
  {
    name: "@ledger/test-support",
    dir: "packages/test-support",
    deps: ["@ledger/types"],
    testSupport: true,
    note: "共享测试支持包（issue #1152）：全局测试接缝（invoke/message/matchMedia/listen/返回桥替身 + 每测清理）唯一宿主，消费只经 devDependency（testSupport 标志 → 规则⑤）；参考数据夹具类型边 @ledger/types 显式放行；toast sink 假件 makeFakeSink/resetToastSink 唯一定义点随包（#1354），复位通过调用方传入 @ledger/loadable 的注册接口完成，避免测试支持包反向依赖被测包造成 workspace 环",
  },
  {
    name: "@ledger/modal-intent",
    dir: "packages/modal-intent",
    deps: [],
    note: "弹窗意图编排包（issue #1316 / ADR-0072 / ADR-0118）：useModalIntent 通用工厂——模态弹窗「开启 / 目标 / 关闭」编排的唯一形态，工厂零业务语义、零外部依赖（无 store、无 api、无组件，不接弹层注册表 ADR-0035）；方向表恒空，外部依赖仅 vue",
  },
  {
    name: "@ledger/row-context-menu",
    dir: "packages/row-context-menu",
    deps: [],
    note: "行右键菜单编排包（issue #1317 / ADR-0077 / ADR-0118）：useRowContextMenu 工厂——行菜单打开/重定位/关闭/选中全部时序的单一实现；方向表恒空仅依赖 vue；不接弹层注册表（ADR-0035）与不调 preventDefault 的既有禁令随包保持，菜单选项构建与业务动作分派留视图",
  },
  {
    name: "@ledger/theme",
    dir: "packages/theme",
    deps: ["@ledger/types", "@ledger/money"],
    note: "主题包（issue #1154 / ADR-0093 / issue #888）：语义色、中性设计令牌、组件库主题覆盖、vanilla-extract 主题合同与图表统一样式的单一来源，Theme 类型随包下移；@ledger 范围内只依赖 @ledger/types 与 @ledger/money，不依赖 stores / components / views / composables",
  },
  {
    name: "@ledger/utils",
    dir: "packages/utils",
    deps: ["@ledger/types", "@ledger/storage", "@ledger/i18n", "@ledger/money"],
    note: "通用工具包（issue #1314 / ADR-0118 决策 2）：src/utils 叶子层全量平铺搬迁——日期/期间、分类树与图表数据形态、Chart.js 统一注册、码化错误本地化 errorMessage（ADR-0050）、字段错误、视图状态、拼音过滤等纯函数单一来源；方向表与实际 import 全等（types / storage / i18n / money），exports 逐模块子路径暴露不开运行期 barrel；不依赖 stores / components / views / composables",
  },
  {
    name: "@ledger/field-errors",
    dir: "packages/field-errors",
    deps: ["@ledger/utils"],
    note: "字段错误态装配包（issue #1319 / ADR-0058 / ADR-0118 决策 4）：useFieldErrors 表单级装配工厂——字段按「原始文本 ref + 判定函数（可选启用条件）」声明，产出每字段错误态与已解析值、聚合提交禁用与重置；只收口径不代判时机，判定口径单点在 @ledger/utils/field-error（#1314 归 utils 包，本包 import 不复制）；方向表与实际 import 全等（仅 @ledger/utils）；不依赖 stores / components / views / composables",
  },
  {
    name: "@ledger/window-tier",
    dir: "packages/window-tier",
    deps: ["@ledger/test-support"],
    note: "窗口分级包（issue #1315 / ADR-0088 / ADR-0118 决策 5）：宽度轴唯一事实源——单一断点两档「宽度信号 → 档位」纯映射 composable；断点常量 WINDOW_TIER_BREAKPOINT_PX 全仓唯一收口包内 src/useWindowTier.ts，vite.config.ts 构建期按源码路径提取（收口漂移 fail-loud，构建期契约保留只换坐标），CSS 媒体查询经占位符替换消费同值；生产依赖仅 vue，@ledger 方向表仅测试边 → @ledger/test-support（媒体查询换档接缝，devDependencies 消费，规则⑤）；不依赖 stores / components / views",
  },
  {
    name: "@ledger/latest-wins",
    dir: "packages/latest-wins",
    deps: [],
    note: "最新胜出竞态纪元包（issue #1678 / ADR-0118 决策 3）：createLatestWins——前端异步竞态裁决机制本体（竞态纪元）的唯一实现与唯一合法住址（check-async-guards 规则 1 豁免坐标随包）；token 制接口 begin/observe/invalidate + token isStale（#1678 定稿：begin 推进取 token、observe 采样不推进供在途合并、invalidate 推进作废全部在途，不做 run-wrapper）；词汇表「最新胜出（Latest-Wins）」词条宿主；五处消费点收敛（Loadable 每次 run/invalidate、标的搜索与服务端分页列表、push-first-list refresh 采样与 invalidate、系统返回换档/销毁、交易弹窗代数）；工厂零业务语义，方向表恒空（无 @ledger 依赖、无 vue），过期后做什么属消费点组合层不入本包；源码直出 exports 仅 . 单入口（单文件包），不建构建链",
  },
  {
    name: "@ledger/loadable",
    dir: "packages/loadable",
    deps: ["@ledger/latest-wins", "@ledger/utils", "@ledger/test-support"],
    note: "异步任务生命周期包（issue #1318 / ADR-0040 / ADR-0118 决策 4/6）：useLoadable 统一异步任务生命周期深模块——loading 置收、错误文案归一、竞态裁决（后发覆盖先发）与 invalidate 作废在途的单一实现，竞态裁决自 #1678 起经共享 module @ledger/latest-wins 消费（竞态纪元唯一合法住址随该包）；模块级 toast sink 单例随包 ESM 持有、对外暴露 registerToastSink 注册接口与 toastError 基础设施错误 toast 出口（#1962 替身后端复用应用统一 sink，注册前 no-op），应用入口 MessageSinkBridge 经导入接线，不引入注入机制（ADR-0118 决策 6）；依赖 @ledger/latest-wins（竞态纪元，#1678）与 @ledger/utils（errorMessage）单向成边；测试边 → test-support（假 sink 假件唯一定义点，#1354，devDependencies 消费，规则⑤）；不依赖 stores / components / views",
  },
  {
    name: "@ledger/scheduled-plan-list",
    dir: "packages/scheduled-plan-list",
    deps: [
      "@ledger/api",
      "@ledger/i18n",
      "@ledger/loadable",
      "@ledger/test-support",
      "@ledger/types",
      "@ledger/utils",
    ],
    note: "计划清单包（issue #1322 / ADR-0041 / ADR-0118 决策 4）：ScheduledPlanList 计划清单深模块——定时计划三业务形态共享的清单编排（加载/刷新、状态过滤、Plan Lifecycle 操作、行操作描述符、周期选项/标签）单一实现，三页签是薄适配器；「不得 import 组件与弹层注册表」禁令随包保持（弹层纯度 ADR-0035）；方向表与实际 import 全等（api / i18n / loadable / types / utils，测试边 → test-support 仅 devDependencies，规则⑤）；不依赖 stores / components / views / composables",
  },
  {
    name: "@ledger/transaction-modal-state",
    dir: "packages/transaction-modal-state",
    deps: [
      "@ledger/modal-intent",
      "@ledger/api",
      "@ledger/i18n",
      "@ledger/latest-wins",
      "@ledger/types",
      "@ledger/utils",
      "@ledger/test-support",
    ],
    note: "交易弹窗编排包（issue #1321 / ADR-0045 / ADR-0118 决策 4）：TransactionModalState——交易列表五个弹窗共享的「开启/目标/关闭」编排，意图闭集五单一判别联合唯一事实源，显示开关由「意图非空」派生；ModalIntent 工厂（ADR-0072）之上首个适配器，「先取明细再开窗、失败不开窗、last-open-wins」异步时序守卫留适配器层（代数自 #1678 起消费 @ledger/latest-wins，工厂序号是落位计数不收敛）；直接 import api 与 useMessage 的既有形态随包保持不做注入；方向表与实际 import 全等（票面四包 + types 类型边，测试边 → test-support 仅 devDependencies 消费，规则⑤）；不依赖 stores / components / views",
  },
  {
    name: "@ledger/ui-kit",
    dir: "packages/ui-kit",
    deps: ["@ledger/utils", "@ledger/i18n", "@ledger/window-tier", "@ledger/test-support"],
    note: "界面通用件包（issue #1320 / ADR-0118 决策 3/5/6）：App* 弹层薄封装 8 件 + overlay 单例族（overlayRegistry + useOverlayReporting，ADR-0035 一体族，注册接口语义一字不改）+ 通用件 4 项（AppDangerConfirmModal / PinyinSelect / NoteCopyButton / app-modal.css.ts），成员闭集逐项定界不得自行增删（CreateFab 等域/应用专属留壳，#1159 按域归位）；方向表与实际 import 全等（utils / i18n / window-tier；测试边 → test-support，devDependencies 消费，规则⑤），exports 逐组件/逐模块子路径暴露（.vue 直接作 exports 落点，.css.ts 同 theme 先例）；不依赖 stores / views / 壳内 composables",
  },
  {
    name: "@ledger/mock-backend",
    dir: "packages/mock-backend",
    deps: ["@ledger/loadable", "@ledger/types"],
    note: "浏览器开发模式替身后端（issue #1962 / ADR-0143）：与 @ledger/test-support 分家（后者绑定 vitest，本包消费面是 vite 应用构建，非 testSupport——自身 dependencies 非空合法）；内存状态替身 + 确定性夹具，未实现命令运行时报错（含命令名，console + toast 双出口，toast 经 @ledger/loadable 的基础设施出口 toastError 复用应用统一 sink）；tauri/ 子目录只经 vite.config mockModeAliases 按文件路径替换（非包名深导入面），唯一生产消费点是 main.ts 的 mode 分支动态 import（生产构建静态消除，接线由 src/__tests__/mock-mode-wiring.test.ts 守门）；方向表与实际 import 全等（loadable / types）",
  },
];

const MEMBER_DIR_GLOB = "packages/*";

const PACKAGE_NAME_PREFIX = "@ledger/";

const FROM_SPECIFIER_PATTERN = /\bfrom\s*['"]([^'"]+)['"]/;
const DYNAMIC_IMPORT_PATTERN = /\bimport\s*\(\s*['"]([^'"]+)['"]/;
const SIDE_EFFECT_IMPORT_PATTERN = /\bimport\s+['"]([^'"]+)['"]/;

const SOURCE_EXTENSIONS: ReadonlySet<string> = new Set([".ts", ".tsx", ".vue"]);

export interface ImportHit {
  line: number;
  text: string;
  specifier: string;
}

export function scanImportSpecifiers(text: string): ImportHit[] {
  const masked = maskComments(text);
  const maskedLines = masked.split("\n");
  const rawLines = text.split("\n");
  const hits: ImportHit[] = [];
  for (let i = 0; i < maskedLines.length; i++) {
    const line = maskedLines[i] ?? "";
    for (const pattern of [
      FROM_SPECIFIER_PATTERN,
      DYNAMIC_IMPORT_PATTERN,
      SIDE_EFFECT_IMPORT_PATTERN,
    ]) {
      const m = line.match(pattern);
      if (!m) continue;
      hits.push({ line: i + 1, text: (rawLines[i] ?? "").trim(), specifier: m[1] ?? "" });
    }
  }
  return hits;
}

function parseLedgerSpecifier(specifier: string): { name: string; subpath?: string } | null {
  const m = /^@ledger\/([^/]+)(?:\/(.*))?$/.exec(specifier);
  if (!m) return null;
  return { name: `${PACKAGE_NAME_PREFIX}${m[1]}`, subpath: m[2] };
}

function collectSourceFiles(dir: string, relBase: string): WalkedFile[] {
  if (!existsSync(dir)) return [];
  return walkTextFiles(dir, relBase, { extensions: SOURCE_EXTENSIONS });
}

function exportsExposeSubpath(exportsField: unknown, subpath: string): boolean {
  if (typeof exportsField === "string") return false;
  if (exportsField === null || typeof exportsField !== "object") return false;
  const keys = Object.keys(exportsField);
  if (keys.includes(`./${subpath}`)) return true;
  return keys.includes("./*");
}

function loadFixtureManifest(path: string): PackageEntry[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(readFileSync(path, "utf8"));
  } catch (err) {
    console.error(`✗ 夹具登记表不可解析（${path}）：${String(err)}`);
    process.exit(1);
  }
  if (!Array.isArray(parsed)) {
    console.error(`✗ 夹具登记表须为 JSON 数组：${path}`);
    process.exit(1);
  }
  return parsed as PackageEntry[];
}

function workspaceDeclaresPackagesGlob(yamlText: string): boolean {
  return /(?:^|\n)packages:[^\n]*\n(?:\s*#[^\n]*\n)*\s*-\s*['"]?packages\/\*['"]?\s*(?:\n|$)/.test(
    yamlText,
  );
}

const DEP_KINDS = [
  "dependencies",
  "devDependencies",
  "optionalDependencies",
  "peerDependencies",
] as const;

function checkMemberRegistration(
  repoRoot: string,
  registry: readonly PackageEntry[],
  problems: string[],
): void {
  const packagesDir = join(repoRoot, "packages");
  if (!existsSync(packagesDir)) {
    problems.push(
      `✗ 成员登记：packages/ 目录不存在——workspace 骨架（${MEMBER_DIR_GLOB}）须有成员目录（可为空，issue #1149）`,
    );
    return;
  }
  const onDisk = readDirEntries(packagesDir)
    .filter((e) => e.isDirectory)
    .map((e) => `packages/${e.name}`);
  for (const dir of onDisk) {
    if (!registry.some((p) => p.dir === dir)) {
      problems.push(
        `✗ 成员登记：${dir} 未登记 PACKAGES\n` +
          `    新增前端子包后须在 scripts/check-frontend-structure.ts 的 PACKAGES 追加一行` +
          `（包名 + 方向表 deps + 注释），否则边界知识分裂、依赖方向失守（issue #1149 规则①）`,
      );
    }
  }
  for (const pkg of registry) {
    if (!onDisk.includes(pkg.dir)) {
      problems.push(`✗ 成员登记：PACKAGES 登记的成员目录不存在：${pkg.dir}（清单漂移 fail loud）`);
    }
  }
}

function checkDependencyDirection(
  repoRoot: string,
  registry: readonly PackageEntry[],
  problems: string[],
): void {
  for (const pkg of registry) {
    const manifestPath = join(repoRoot, pkg.dir, "package.json");
    if (!existsSync(manifestPath)) {
      problems.push(`✗ 包依赖方向：${pkg.dir}/package.json 不存在（${pkg.name}，${pkg.note}）`);
      continue;
    }
    let manifest: {
      name?: unknown;
      dependencies?: unknown;
      devDependencies?: unknown;
      optionalDependencies?: unknown;
      peerDependencies?: unknown;
      exports?: unknown;
    };
    try {
      manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    } catch (err) {
      problems.push(`✗ 包依赖方向：${pkg.dir}/package.json 不可解析：${String(err)}`);
      continue;
    }
    if (manifest.name !== pkg.name) {
      problems.push(
        `✗ 成员登记：PACKAGES 登记名 ${pkg.name} 与包清单 name ${String(manifest.name)} 不一致（${pkg.dir}）`,
      );
    }
    for (const kind of DEP_KINDS) {
      const deps = manifest[kind];
      if (deps === null || typeof deps !== "object") continue;
      for (const depName of Object.keys(deps as Record<string, unknown>)) {
        if (!depName.startsWith(PACKAGE_NAME_PREFIX)) continue;
        const target = registry.find((p) => p.name === depName);
        if (!target) {
          problems.push(
            `✗ 包依赖方向：${pkg.name}（${pkg.dir}）${kind} 依赖未登记包 ${depName}\n` +
              `    新包须先在 scripts/check-frontend-structure.ts 的 PACKAGES 登记（issue #1149 规则②）`,
          );
          continue;
        }
        if (!pkg.deps.includes(depName)) {
          problems.push(
            `✗ 包依赖方向：${pkg.name}（${pkg.dir}）${kind} 依赖 ${depName}，不在其方向表内\n` +
              `    方向表（PACKAGES[${pkg.name}].deps）随包抽取逐票补充；确属合法依赖时在方向表追加该边（issue #1149 规则②）`,
          );
        }
      }
    }
  }
}

function checkDependencyConsumption(
  repoRoot: string,
  registry: readonly PackageEntry[],
  problems: string[],
): void {
  const registeredNames = new Set(registry.map((p) => p.name));
  for (const pkg of registry) {
    const pkgRoot = join(repoRoot, pkg.dir);
    if (!existsSync(pkgRoot)) continue;
    const manifest = tryReadManifest(join(pkgRoot, "package.json"));
    if (!manifest) continue;
    const declared = new Set<string>();
    for (const kind of DEP_KINDS) {
      const deps = manifest[kind];
      if (deps === null || typeof deps !== "object") continue;
      for (const depName of Object.keys(deps as Record<string, unknown>)) {
        if (depName.startsWith(PACKAGE_NAME_PREFIX)) declared.add(depName);
      }
    }
    const consumed = new Map<string, { rel: string; line: number; text: string }>();
    for (const f of collectSourceFiles(pkgRoot, pkg.dir)) {
      for (const hit of scanImportSpecifiers(readFileSync(f.abs, "utf8"))) {
        const name = parseLedgerSpecifier(hit.specifier)?.name;
        if (!name || name === pkg.name || !registeredNames.has(name)) continue;
        if (!consumed.has(name)) {
          consumed.set(name, { rel: f.rel, line: hit.line, text: hit.text });
        }
      }
    }
    for (const [name, hit] of consumed) {
      if (declared.has(name)) continue;
      problems.push(
        `✗ 包依赖方向（消费方声明边）：${pkg.name}（${pkg.dir}）包内源码 import ${name}，` +
          `但 package.json 四类依赖均未声明\n` +
          `    ${hit.rel}:${hit.line} ${hit.text}\n` +
          `    幽灵 import 可借 workspace 根 node_modules 符号链接解析，方向表被静默绕过；` +
          `生产代码消费补 dependencies，仅测试代码消费补 devDependencies（issue #1470 规则②）`,
      );
    }
    for (const depName of pkg.deps) {
      if (consumed.has(depName)) continue;
      problems.push(
        `✗ 包依赖方向（反向核对）：${pkg.name}（${pkg.dir}）方向表登记 ${depName}，` +
          `包内源码未实际消费该边\n` +
          `    方向表（PACKAGES[${pkg.name}].deps）与实际 import 全等，两向都可证伪（issue #1470 规则②）：` +
          (registeredNames.has(depName)
            ? `该边确已废弃时从方向表删除，消费被误删时补回源码 import`
            : `${depName} 还不是已登记包，核对登记名拼写或先在 PACKAGES 登记该包`),
      );
    }
  }
}

function checkImportShapes(
  repoRoot: string,
  registry: readonly PackageEntry[],
  problems: string[],
): void {
  for (const pkg of registry) {
    const pkgRoot = join(repoRoot, pkg.dir);
    const files = collectSourceFiles(pkgRoot, pkg.dir);
    for (const f of files) {
      const source = readFileSync(f.abs, "utf8");
      for (const hit of scanImportSpecifiers(source)) {
        const spec = hit.specifier;
        if (spec.startsWith("@/")) {
          problems.push(
            `✗ 跨包引用形态：${f.rel}:${hit.line} 使用 @/ 别名（${spec}）\n` +
              `    ${hit.text}\n` +
              `    @/ 指向根 src，从包内使用必然穿越包边界；跨包引用只能走包名 @ledger/*，` +
              `包内引用走相对路径（issue #1149 规则③）`,
          );
          continue;
        }
        if (spec.startsWith(".")) {
          const resolved = resolve(dirname(f.abs), spec);
          const relToPkg = relative(pkgRoot, resolved);
          if (relToPkg.startsWith("..") || relToPkg === "") {
            problems.push(
              `✗ 跨包引用形态：${f.rel}:${hit.line} 相对路径穿越包边界（${spec}）\n` +
                `    ${hit.text}\n` +
                `    解析落点 ${relToPkg} 越出 ${pkg.dir}；跨包引用只能走包名 @ledger/*（issue #1149 规则③）`,
            );
          }
          continue;
        }
        if (spec.startsWith(PACKAGE_NAME_PREFIX)) {
          const parsed = parseLedgerSpecifier(spec);
          if (!parsed) {
            problems.push(
              `✗ 跨包引用形态：${f.rel}:${hit.line} 非法包名形态（${spec}）（issue #1149 规则③）`,
            );
            continue;
          }
          const { name, subpath } = parsed;
          const target = registry.find((p) => p.name === name);
          if (!target) {
            problems.push(
              `✗ 跨包引用形态：${f.rel}:${hit.line} 引用未登记包 ${spec}\n` +
                `    新包须先在 PACKAGES 登记（issue #1149 规则②/③）`,
            );
            continue;
          }
          if (subpath === undefined) continue;
          const targetManifestPath = join(repoRoot, target.dir, "package.json");
          if (!existsSync(targetManifestPath)) {
            problems.push(
              `✗ 深导入禁令：${f.rel}:${hit.line} 深导入 ${spec}——目标包 package.json 不存在（${target.dir}）`,
            );
            continue;
          }
          let targetManifest: { exports?: unknown };
          try {
            targetManifest = JSON.parse(readFileSync(targetManifestPath, "utf8"));
          } catch (err) {
            problems.push(
              `✗ 深导入禁令：${f.rel}:${hit.line} 目标包清单不可解析：${target.dir}/package.json（${String(err)}）`,
            );
            continue;
          }
          if (!exportsExposeSubpath(targetManifest.exports, subpath)) {
            problems.push(
              `✗ 深导入禁令：${f.rel}:${hit.line} 深导入 ${spec} 未命中目标包 exports 入口\n` +
                `    ${hit.text}\n` +
                `    跨包引用必须命中目标包 package.json exports 的对应入口（精确键 ./${subpath} ` +
                `或 ./* 通配）；补导出入口或经包名整引（issue #1149 规则④）`,
            );
          }
        }
      }
    }
  }
}

function tryReadManifest(path: string): Record<string, unknown> | null {
  if (!existsSync(path)) return null;
  try {
    return JSON.parse(readFileSync(path, "utf8")) as Record<string, unknown>;
  } catch {
    return null;
  }
}

function checkTestSupportPurity(
  repoRoot: string,
  registry: readonly PackageEntry[],
  problems: string[],
): void {
  const testSupportNames = registry.filter((p) => p.testSupport).map((p) => p.name);
  if (testSupportNames.length === 0) return;
  const forbiddenKinds = ["dependencies", "optionalDependencies", "peerDependencies"] as const;
  const manifests: readonly { label: string; path: string }[] = [
    { label: "根包（应用壳）", path: join(repoRoot, "package.json") },
    ...registry.map((p) => ({ label: p.name, path: join(repoRoot, p.dir, "package.json") })),
  ];
  for (const manifest of manifests) {
    const parsed = tryReadManifest(manifest.path);
    if (!parsed) continue;
    for (const kind of forbiddenKinds) {
      const deps = parsed[kind];
      if (deps === null || typeof deps !== "object") continue;
      for (const testSupport of testSupportNames) {
        if (!(testSupport in (deps as Record<string, unknown>))) continue;
        problems.push(
          `✗ 测试支持纯净性：${manifest.label} 的 ${kind} 出现 ${testSupport}\n` +
            `    测试支持只许经 devDependencies 消费（issue #1152 规则⑤）：生产依赖图零测试支持内容；` +
            `把该依赖移入 devDependencies（消费方为测试代码，不影响产物构建）`,
        );
      }
    }
  }
  for (const pkg of registry) {
    if (!pkg.testSupport) continue;
    const parsed = tryReadManifest(join(repoRoot, pkg.dir, "package.json"));
    if (!parsed) continue;
    const prodDeps = parsed.dependencies;
    if (prodDeps !== null && typeof prodDeps === "object" && Object.keys(prodDeps).length > 0) {
      problems.push(
        `✗ 测试支持纯净性：${pkg.name} 自身 dependencies 非空（${Object.keys(prodDeps as Record<string, unknown>).join(" ")}）\n` +
          `    测试支持包零生产依赖（issue #1152 规则⑤）：替身与接缝所需运行面全部走 devDependencies`,
      );
    }
  }
}

export interface UpwardImportRule {
  dir: string;
  forbidden: readonly string[];
  note: string;
}

export const FORBIDDEN_UPWARD_IMPORTS: readonly UpwardImportRule[] = [];

function hitsForbiddenSpecifier(specifier: string, forbidden: readonly string[]): string | null {
  for (const target of forbidden) {
    if (specifier === target || specifier.startsWith(`${target}/`)) return target;
  }
  return null;
}

function checkUpwardImports(repoRoot: string, problems: string[]): void {
  for (const rule of FORBIDDEN_UPWARD_IMPORTS) {
    const dir = join(repoRoot, rule.dir);
    if (!existsSync(dir)) {
      problems.push(
        `✗ 上行引用禁令：登记目录不存在：${rule.dir}（${rule.note}）\n` +
          `    目录改名/漂移后规则静默失效，须同步 FORBIDDEN_UPWARD_IMPORTS（issue #1156）`,
      );
      continue;
    }
    for (const f of collectSourceFiles(dir, rule.dir)) {
      const source = readFileSync(f.abs, "utf8");
      for (const hit of scanImportSpecifiers(source)) {
        const target = hitsForbiddenSpecifier(hit.specifier, rule.forbidden);
        if (!target) continue;
        problems.push(
          `✗ 上行引用禁令：${f.rel}:${hit.line} 引用 ${hit.specifier}\n` +
            `    ${hit.text}\n` +
            `    ${rule.dir} 为叶子层，不得引用 ${target}；把接缝迁到消费侧或改入参注入` +
            `（issue #1156 规则⑥）`,
        );
      }
    }
  }
}

export interface DeepModuleBoundary {
  module: string;
  allowedConsumers: readonly string[];
  note: string;
}

export const DEEP_MODULE_BOUNDARIES: readonly DeepModuleBoundary[] = [
  {
    module: "src/transaction/useTransactionFilter.ts",
    allowedConsumers: ["src/views"],
    note: "交易列表过滤深模块（ADR-0030/0094）：依赖壳内 pinia store（交易页会话级 store）故不成包（ADR-0118 决策 4），消费面 = 交易页与报表页；#1159 起随交易域归位 src/transaction/",
  },
  {
    module: "src/investment/useInstrumentSearch.ts",
    allowedConsumers: ["src/investment"],
    note: "标的远程搜索深模块（issue #1308）：防抖/在途纪元/吞错内化，消费面 = useInvestmentForm 与 useRealizedPnl 两处下拉；成包虽过 ADR-0118 决策 1 判据但消费面全在投资域、无边界收益，同 ADR-0123 createPushFirstList 先例留壳；域内件边界由域目录 + 本登记承担",
  },
];

function consumerAllowed(consumerRel: string, allowed: readonly string[]): boolean {
  return allowed.some((dir) => consumerRel === dir || consumerRel.startsWith(`${dir}/`));
}

function resolveShellSpecifier(repoRoot: string, consumerAbs: string, specifier: string): string {
  const abs = specifier.startsWith("@/")
    ? join(repoRoot, "src", specifier.slice(2))
    : resolve(dirname(consumerAbs), specifier);
  return relative(repoRoot, abs);
}

function hitsDeepModule(candRel: string, moduleRel: string): boolean {
  if (candRel === moduleRel) return true;
  const stem = moduleRel.replace(/\.ts$/, "");
  return candRel === stem || candRel === `${stem}.js`;
}

function isTestFile(rel: string): boolean {
  return rel.split("/").includes("__tests__") || /\.(test|spec)\.[tj]sx?$/.test(rel);
}

function checkDeepModuleBoundaries(repoRoot: string, problems: string[]): void {
  for (const entry of DEEP_MODULE_BOUNDARIES) {
    const moduleAbs = join(repoRoot, entry.module);
    if (!existsSync(moduleAbs)) {
      problems.push(
        `✗ 深模块边界：登记模块不存在：${entry.module}（${entry.note}）\n` +
          `    模块改名/删除后规则静默失效，须同步 DEEP_MODULE_BOUNDARIES（issue #1323 规则⑦）`,
      );
      continue;
    }
    for (const tree of ["src", "packages"]) {
      for (const f of collectSourceFiles(join(repoRoot, tree), tree)) {
        if (isTestFile(f.rel)) continue;
        const source = readFileSync(f.abs, "utf8");
        for (const hit of scanImportSpecifiers(source)) {
          if (!hit.specifier.startsWith("@/") && !hit.specifier.startsWith(".")) continue;
          const candRel = resolveShellSpecifier(repoRoot, f.abs, hit.specifier);
          if (!hitsDeepModule(candRel, entry.module)) continue;
          if (consumerAllowed(f.rel, entry.allowedConsumers)) continue;
          problems.push(
            `✗ 深模块边界：${f.rel}:${hit.line} 消费 ${entry.module}\n` +
              `    ${hit.text}\n` +
              `    ${entry.module} 为深模块，消费方限于白名单（${entry.allowedConsumers.join(" ")}）；` +
              `新消费方先评估扩白名单或改接缝（issue #1323 规则⑦ / ADR-0118 决策 7）`,
          );
        }
      }
    }
  }
}

function main(): void {
  const scriptDir = dirname(fileURLToPath(import.meta.url));
  const repoRoot = process.argv[2] ?? join(scriptDir, "..");
  const fixtureManifestPath = process.argv[3];
  const registry: readonly PackageEntry[] = fixtureManifestPath
    ? loadFixtureManifest(fixtureManifestPath)
    : PACKAGES;

  const problems: string[] = [];

  const workspaceYamlPath = join(repoRoot, "pnpm-workspace.yaml");
  if (!existsSync(workspaceYamlPath)) {
    problems.push(`✗ workspace 骨架：pnpm-workspace.yaml 不存在（${repoRoot}）`);
  } else if (!workspaceDeclaresPackagesGlob(readFileSync(workspaceYamlPath, "utf8"))) {
    problems.push(
      `✗ workspace 骨架：pnpm-workspace.yaml 未以 glob 声明 ${MEMBER_DIR_GLOB}\n` +
        `    packages: 列表须含 - packages/*（成员可为空，新增子包自动入 workspace，issue #1149）`,
    );
  }

  checkMemberRegistration(repoRoot, registry, problems);
  checkDependencyDirection(repoRoot, registry, problems);
  checkDependencyConsumption(repoRoot, registry, problems);
  checkImportShapes(repoRoot, registry, problems);
  checkTestSupportPurity(repoRoot, registry, problems);
  checkUpwardImports(repoRoot, problems);
  checkDeepModuleBoundaries(repoRoot, problems);

  if (problems.length > 0) {
    for (const p of problems) console.error(p);
    console.error(`❌ 前端结构守门失败：${problems.length} 处问题（issue #1149）`);
    process.exit(1);
  }
  console.log(
    `✓ 前端结构守门：pnpm-workspace.yaml 声明 ${MEMBER_DIR_GLOB}` +
      `· 成员登记 ${registry.length} 个（磁盘 ↔ PACKAGES 双向全等）` +
      `· 包依赖方向 ${registry.length} 包（方向表逐票补充 + 消费面声明边/反向全等核对）` +
      `· 跨包引用形态与深导入禁令扫描 ${collectSourceFiles(join(repoRoot, "packages"), "packages").length} 个文件` +
      `· 测试支持纯净性（${
        registry
          .filter((p) => p.testSupport)
          .map((p) => p.name)
          .join(" ") || "无"
      } 仅 devDependency 消费）` +
      `· 上行引用禁令 ${FORBIDDEN_UPWARD_IMPORTS.length} 条（${FORBIDDEN_UPWARD_IMPORTS.map((r) => r.dir).join(" ") || "无"}）` +
      `· 深模块边界 ${DEEP_MODULE_BOUNDARIES.length} 项（${DEEP_MODULE_BOUNDARIES.map((e) => e.module).join(" ") || "无"}）`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
