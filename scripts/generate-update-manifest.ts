#!/usr/bin/env bun
// 更新清单生成器（issue #1428 / ADR-0124 决策 2/10）：发布 publish 阶段把各平台
// 更新工件与 .sig 签名装配成 latest.json 随 GitHub Release 上传——发布即更新源，
// 清单生成零手工步骤（不引入 tauri-action，沿 ADR-0066 决策 3 手写 publish 路线）。
// publish job（build.yml）与本地干跑共用本脚本，清单形态只有这一份权威。
//
// 清单契约（消费方 = tauri-plugin-updater Static 形态，随 #1429 进应用）：
//   {
//     "version":   "x.y.z"            ← 纯三段式语义版本（无 v 前缀，semver 比较）
//     "notes":     "…"                ← 发布说明，承载兼容性警告（取 CHANGELOG 版本条目）
//     "pub_date":  "…Z"               ← RFC3339 UTC，秒精度
//     "platforms": {
//       "<target>": { "url": "…", "signature": "…" }
//     }
//   }
//   - platforms 键（updater target 字符串 = {target}-{arch}）闭集三平台：
//     darwin-aarch64（*.app.tar.gz）、windows-x86_64（NSIS *.exe，更新工件即安装器
//     本体）、linux-x86_64（*.AppImage，同左）；deb 与 Android 无更新工件、不进清单。
//   - signature = 对应更新工件 .sig 文件内容（minisign 签名的 base64 文本，原样内嵌）。
//     签名经 `pnpm tauri signer sign --app-version <版本>` 生成（CLI 2.11.5 起
//     trusted comment 绑定版本，配合插件 requireSignedVersion 可防降级——#1429 配置项）。
//
// 更新源端点定形记录（#1429 插件配置的契约依据）：端点以数组配置、按序尝试，
//   首位为 https://github.com/asiazhang/ledger/releases/latest/download/latest.json
//   （latest 跳转恒指最新已发布 Release 的清单资产，发布零手工）；镜像扩展位 = 数组
//   追加，不改代码（ADR-0124 决策 2）。
//
// 本地干跑（不必触发完整发布，验证生成物）：
//   bun scripts/generate-update-manifest.ts --dir <工件目录> --version 0.9.0 \
//     --tag v0.9.0 --repo asiazhang/ledger --notes-file <发布说明.md>
//   目录内放置三个更新工件（文件名任意、平铺或嵌套子目录均可——按后缀识别，
//   嵌套形态 = publish 下载的 upload-artifact LCA 布局）并各配 `pnpm tauri signer sign
//   --app-version 0.9.0 <工件>` 产出的 .sig；不传 --out 时清单打印到 stdout。

import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

/** 平台键 → 更新工件文件后缀的闭集（updater Static 形态的 platforms 键）。 */
const PLATFORM_ARTIFACT_SUFFIXES: ReadonlyArray<{ target: string; suffix: string }> = [
  { target: "darwin-aarch64", suffix: ".app.tar.gz" },
  { target: "windows-x86_64", suffix: ".exe" },
  { target: "linux-x86_64", suffix: ".AppImage" },
];

/** 单平台更新工件：文件名与随附 .sig 的内容（清单 signature 字段原样内嵌）。 */
export interface PlatformArtifact {
  target: string;
  file: string;
  signature: string;
}

/** 清单装配输入（pubDate 显式注入，测试可固定时间）。 */
export interface ManifestInput {
  version: string;
  notes: string | undefined;
  pubDate: string;
  repo: string;
  tag: string;
  artifacts: PlatformArtifact[];
}

/** 校验三段式语义版本（与发布流程 versionCode 派生同一规则；拒绝 v 前缀）。 */
export function validateVersion(version: string): void {
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`✗ 版本号 ${version} 不是三段式语义版本（应为 x.y.z，无 v 前缀）`);
  }
}

/** 扫描工件目录（递归含子目录），按后缀闭集识别三平台更新工件并读取随附 .sig。
 *  递归的缘由：publish 下载的工件按 upload-artifact 最小公共祖先保留子目录形态
 *  （dmg 与更新工件分属 bundle/dmg、bundle/macos，v0.9.1 首次执行 publish 实测）。
 *  file 取 basename——Release 资产恒平铺，清单 URL 以资产名为准；.sig 与工件同目录。
 *  缺工件 / 缺签名 / 同平台多候选一律报错——发布门禁宁红不哑。 */
export function collectUpdaterArtifacts(dir: string): PlatformArtifact[] {
  const entries = readdirSync(dir, { recursive: true, encoding: "utf8" });
  const basename = (entry: string): string => entry.split(/[\\/]/).pop()!;
  const artifacts: PlatformArtifact[] = [];
  for (const { target, suffix } of PLATFORM_ARTIFACT_SUFFIXES) {
    const matches = entries.filter((entry) => basename(entry).endsWith(suffix));
    if (matches.length === 0) {
      throw new Error(
        `✗ ${dir} 中未找到 ${target} 更新工件（*${suffix}）；deb / dmg / apk 不进清单，缺更新工件即发布失败`,
      );
    }
    if (matches.length > 1) {
      throw new Error(`✗ ${target} 更新工件候选不唯一：${matches.join("、")}`);
    }
    const file = basename(matches[0]!);
    const sigPath = join(dir, `${matches[0]!}.sig`);
    let signature: string;
    try {
      signature = readFileSync(sigPath, "utf8").trim();
    } catch {
      throw new Error(`✗ 更新工件 ${file} 缺随附签名 ${file}.sig（无签名的工件不被更新器接受）`);
    }
    if (signature.length === 0 || !/^[A-Za-z0-9+/=\r\n]+$/.test(signature)) {
      throw new Error(`✗ 签名文件 ${file}.sig 不是非空 base64 文本，疑似损坏`);
    }
    artifacts.push({ target, file, signature });
  }
  return artifacts;
}

/** 装配 latest.json（字段序即输出序：version / notes / pub_date / platforms）。 */
export function buildManifest(input: ManifestInput): string {
  validateVersion(input.version);
  if (!input.repo || input.repo.includes("..") || !/^[^/]+\/[^/]+$/.test(input.repo)) {
    throw new Error(`✗ 仓库标识 ${input.repo} 非法（应为 owner/name）`);
  }
  const platforms: Record<string, { url: string; signature: string }> = {};
  for (const artifact of input.artifacts) {
    platforms[artifact.target] = {
      url: `https://github.com/${input.repo}/releases/download/${input.tag}/${artifact.file}`,
      signature: artifact.signature,
    };
  }
  const manifest: Record<string, unknown> = {
    version: input.version,
    notes: input.notes,
    pub_date: input.pubDate,
    platforms,
  };
  return `${JSON.stringify(manifest, null, 2)}\n`;
}

export interface CliOptions {
  dir: string;
  version: string;
  repo: string | undefined;
  tag: string | undefined;
  notesFile: string | undefined;
  out: string | undefined;
}

/** 解析 CLI 参数；repo / tag 缺省取 CI 环境（GITHUB_REPOSITORY / GITHUB_REF_NAME）。 */
export function parseArgs(argv: readonly string[], env: NodeJS.ProcessEnv): CliOptions {
  const values = new Map<string, string>();
  for (let i = 0; i < argv.length; i += 2) {
    const flag = argv[i]!;
    const value = argv[i + 1];
    if (!flag.startsWith("--") || value === undefined) {
      throw new Error(`✗ 参数形态非法：${flag} ${value ?? ""}（应为 --flag value 成对出现）`);
    }
    if (values.has(flag)) {
      throw new Error(`✗ 参数 ${flag} 重复给出`);
    }
    values.set(flag, value);
  }
  const required = (flag: string): string => {
    const value = values.get(flag);
    if (!value) throw new Error(`✗ 缺少必填参数 ${flag}`);
    return value;
  };
  const optional = (flag: string): string | undefined => values.get(flag);
  return {
    dir: required("--dir"),
    version: required("--version"),
    repo: optional("--repo") ?? env.GITHUB_REPOSITORY,
    tag: optional("--tag") ?? env.GITHUB_REF_NAME,
    notesFile: optional("--notes-file"),
    out: optional("--out"),
  };
}

function main(): void {
  let options: CliOptions;
  try {
    options = parseArgs(process.argv.slice(2), process.env);
  } catch (error) {
    console.error(String(error instanceof Error ? error.message : error));
    console.error(
      "用法：bun scripts/generate-update-manifest.ts --dir <工件目录> --version x.y.z [--tag v…|--repo owner/name|--notes-file <文件>|--out <文件>]",
    );
    process.exit(1);
  }
  try {
    let notes: string | undefined;
    if (options.notesFile !== undefined) {
      notes = readFileSync(options.notesFile, "utf8").replace(/\n$/, "");
    }
    const artifacts = collectUpdaterArtifacts(options.dir);
    const pubDate = new Date().toISOString().replace(/\.\d{3}Z$/, "Z");
    const manifest = buildManifest({
      version: options.version,
      notes,
      pubDate,
      repo: options.repo ?? "",
      tag: options.tag ?? "",
      artifacts,
    });
    if (options.out === undefined) {
      process.stdout.write(manifest);
    } else {
      writeFileSync(options.out, manifest);
      console.error(`✓ 更新清单已生成：${options.out}`);
    }
  } catch (error) {
    console.error(String(error instanceof Error ? error.message : error));
    process.exit(1);
  }
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的装配函数
// （与 scripts/check-commands.ts 同一惯法）。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
