#!/usr/bin/env bun

import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const PLATFORM_ARTIFACT_SUFFIXES: ReadonlyArray<{ target: string; suffix: string }> = [
  { target: "darwin-aarch64", suffix: ".app.tar.gz" },
  { target: "windows-x86_64", suffix: ".exe" },
  { target: "linux-x86_64", suffix: ".AppImage" },
];

export interface PlatformArtifact {
  target: string;
  file: string;
  signature: string;
}

export interface ManifestInput {
  version: string;
  notes: string | undefined;
  pubDate: string;
  repo: string;
  tag: string;
  artifacts: PlatformArtifact[];
}

export function validateVersion(version: string): void {
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`✗ 版本号 ${version} 不是三段式语义版本（应为 x.y.z，无 v 前缀）`);
  }
}

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

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
