// check-frontend-structure.ts 守门脚本的测试夹具唯一定义点（issue #1435）：
// scripts/check-frontend-structure.test.ts（规则⑦）与
// src/__tests__/check-frontend-structure.test.ts（规则①—⑥ + 接线核对）共用的
// 夹具仓库根 builder——此前是同构逻辑的两份手写副本（建 workspace yaml + 接线
// 宿主 + 规则⑦登记模块文件），每次登记项变更都要两处同改，漏一处即 CI 假红/假绿
// （#1308 新增 useInstrumentSearch 登记项时两处各改一处的实测放大）。
//
// 登记项自适配（本文件存在的理由）：规则⑦登记模块文件与规则⑥登记目录不逐项
// 手写，按生产登记表（DEEP_MODULE_BOUNDARIES / FORBIDDEN_UPWARD_IMPORTS，单一
// 事实源，本脚本不可注入面）迭代创建——夹具绿基线自动自足含全部登记项，否则
// 「登记模块不存在」「登记目录不存在」假红随登记表增长必然复发；新增/删除登记
// 项只改登记表与登记表全等断言两点，夹具零编辑。
//
// 夹具只构成守门脚本的**最小绿基线**（接线宿主 + 登记项本体，规则①—⑤经 arg2
// 注入夹具登记表隔离），靶形场景由各测试文件在夹具上续写（writeSource /
// writePackageManifest）。测试与所测脚本同目录住（#1158）；跨目录消费
// （src/__tests__）沿既有先例直引 ../../scripts/（被测脚本同款）。

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import {
  DEEP_MODULE_BOUNDARIES,
  FORBIDDEN_UPWARD_IMPORTS,
  SCRIPT_INVOCATION,
} from "./check-frontend-structure.ts";

/** 夹具登记表条目（与 PACKAGES 同形的最小条目，arg2 JSON 注入面） */
export interface FixtureEntry {
  name: string;
  dir: string;
  deps: string[];
  testSupport?: boolean;
  note: string;
}

export interface FixtureRepoOptions {
  /** 是否写 pnpm-workspace.yaml 的 packages/* 声明（缺省写；false = 缺声明场景） */
  yamlGlob?: boolean;
  /** packages/ 下预创建的成员目录名（不写 package.json——规则①删除即变红②靶形） */
  memberDirs?: string[];
  /** 接线宿主缺位场景：跳过 check.sh / build.yml 的接线行 */
  omitWiring?: ("check.sh" | "build.yml")[];
  /** 夹具登记表（与 PACKAGES 同形）；缺省不注入——脚本退回生产 PACKAGES 登记表
   *  （规则①②的磁盘 ↔ 清单错位靶形依赖该形态）；规则⑦夹具显式注入空表隔离
   *  规则①—⑤（PACKAGES 非空后不注入会对夹具根触发「清单漂移」假红，#1150） */
  manifest?: FixtureEntry[];
  /** 跳过规则⑦登记模块文件（「登记模块不存在」靶形） */
  omitModule?: boolean;
}

const tempDirs: string[] = [];

/** 建夹具仓库根：workspace yaml（glob 声明）+ packages/ 目录 + 两个接线宿主 +
 *  规则⑦登记模块文件与规则⑥登记目录（按生产登记表迭代创建，登记项自适配）；
 *  与真实仓库同构的最小绿基线，opts 覆盖缺口场景。返回 spawnSync args
 *  （[repo-root] 或 [repo-root, fixture-manifest.json]）。 */
export function fixtureRepo(opts: FixtureRepoOptions = {}): string[] {
  const root = mkdtempSync(join(tmpdir(), "check-frontend-structure-fixture-"));
  tempDirs.push(root);
  const yamlLines = ["# 夹具", "allowBuilds:", "  esbuild: true"];
  if (opts.yamlGlob !== false) {
    yamlLines.unshift("packages:", "  # 夹具注释", "  - packages/*");
  }
  writeFileSync(join(root, "pnpm-workspace.yaml"), yamlLines.join("\n") + "\n");
  mkdirSync(join(root, "packages"), { recursive: true });
  // 规则⑦ 登记模块本体（issue #1323 起，#1308 增 useInstrumentSearch）：模块文件
  // 本体只是存在性靶（守门只查存在与「他文件 import 它」），统一桩内容即可；
  // 逐项手写即第二份登记表（#1435 收敛的回潮面），一律走登记表迭代。
  if (!opts.omitModule) {
    for (const entry of DEEP_MODULE_BOUNDARIES) {
      const moduleAbs = join(root, ...entry.module.split("/"));
      mkdirSync(dirname(moduleAbs), { recursive: true });
      writeFileSync(moduleAbs, "export const fixtureRegisteredModule = true\n");
    }
  }
  // 规则⑥ 登记目录本体（issue #1156）：登记目录缺失即红，绿基线同样按登记表
  // 迭代创建（现行空集为 no-op，登记项回潮时夹具自动跟随）。
  for (const rule of FORBIDDEN_UPWARD_IMPORTS) {
    mkdirSync(join(root, ...rule.dir.split("/")), { recursive: true });
  }
  for (const dir of opts.memberDirs ?? []) {
    mkdirSync(join(root, "packages", dir), { recursive: true });
  }
  if (!opts.omitWiring?.includes("check.sh")) {
    mkdirSync(join(root, "scripts"), { recursive: true });
    writeFileSync(join(root, "scripts", "check.sh"), `#!/bin/sh\n${SCRIPT_INVOCATION}\n`);
  }
  if (!opts.omitWiring?.includes("build.yml")) {
    mkdirSync(join(root, ".github", "workflows"), { recursive: true });
    writeFileSync(
      join(root, ".github", "workflows", "build.yml"),
      `jobs:\n  frontend:\n    steps:\n      - name: 前端结构守门检查\n        run: ${SCRIPT_INVOCATION}\n`,
    );
  }
  const args = [root];
  if (opts.manifest) {
    const manifestPath = join(root, "fixture-manifest.json");
    writeFileSync(manifestPath, JSON.stringify(opts.manifest, null, 2));
    args.push(manifestPath);
  }
  return args;
}

/** 清理全部夹具临时目录（各测试文件 afterAll 各调一次，模块态按 vitest 进程隔离） */
export function cleanupFixtureRepos(): void {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
  tempDirs.length = 0;
}
