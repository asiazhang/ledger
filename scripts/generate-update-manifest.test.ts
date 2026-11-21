import { describe, expect, it } from "bun:test";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  buildManifest,
  collectUpdaterArtifacts,
  parseArgs,
  validateVersion,
} from "./generate-update-manifest.ts";
import { gateScript, runGateScript } from "./run-gate-script.test-helper.ts";

const script = gateScript("generate-update-manifest.ts");

const RELEASE_FILES: Record<string, string> = {
  "OpenLedger_0.9.0_aarch64.app.tar.gz": "macos-updater-bytes",
  "OpenLedger_0.9.0_aarch64.app.tar.gz.sig": "bWFjaW50cnVtc2lnbnNpZ25hdHVyZQ==",
  "OpenLedger_0.9.0_x64-setup.exe": "nsis-installer-bytes",
  "OpenLedger_0.9.0_x64-setup.exe.sig": "d2luZG93c21pbnNpZ25zaWc=",
  "OpenLedger_0.9.0_amd64.AppImage": "appimage-bytes",
  "OpenLedger_0.9.0_amd64.AppImage.sig": "bGludXhtaW5zaWduc2lnbg==",
  "OpenLedger_0.9.0_aarch64.dmg": "dmg-bytes",
  "OpenLedger_0.9.0_amd64.deb": "deb-bytes",
  "OpenLedger_0.9.0_arm64.apk": "apk-bytes",
};

function fixtureDir(files: Record<string, string> = RELEASE_FILES): string {
  const dir = mkdtempSync(join(tmpdir(), "update-manifest-"));
  for (const [name, content] of Object.entries(files)) {
    writeFileSync(join(dir, name), content);
  }
  return dir;
}

describe("collectUpdaterArtifacts", () => {
  it("按后缀闭集识别三平台工件并读取随附 .sig，安装包干扰项不进清单", () => {
    const dir = fixtureDir();
    try {
      expect(collectUpdaterArtifacts(dir)).toEqual([
        {
          target: "darwin-aarch64",
          file: "OpenLedger_0.9.0_aarch64.app.tar.gz",
          signature: "bWFjaW50cnVtc2lnbnNpZ25hdHVyZQ==",
        },
        {
          target: "windows-x86_64",
          file: "OpenLedger_0.9.0_x64-setup.exe",
          signature: "d2luZG93c21pbnNpZ25zaWc=",
        },
        {
          target: "linux-x86_64",
          file: "OpenLedger_0.9.0_amd64.AppImage",
          signature: "bGludXhtaW5zaWduc2lnbg==",
        },
      ]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("嵌套子目录形态（download-artifact 按 upload-artifact LCA 保留的结构）同样识别，file 取 basename", () => {
    const layout: Record<string, Record<string, string>> = {
      ".": {
        "OpenLedger_0.9.0_x64-setup.exe": "nsis-installer-bytes",
        "OpenLedger_0.9.0_x64-setup.exe.sig": "d2luZG93c21pbnNpZ25zaWc=",
      },
      dmg: { "OpenLedger_0.9.0_aarch64.dmg": "dmg-bytes" },
      macos: {
        "OpenLedger_0.9.0_aarch64.app.tar.gz": "macos-updater-bytes",
        "OpenLedger_0.9.0_aarch64.app.tar.gz.sig": "bWFjaW50cnVtc2lnbnNpZ25hdHVyZQ==",
      },
      appimage: {
        "OpenLedger_0.9.0_amd64.AppImage": "appimage-bytes",
        "OpenLedger_0.9.0_amd64.AppImage.sig": "bGludXhtaW5zaWduc2lnbg==",
      },
    };
    const dir = mkdtempSync(join(tmpdir(), "update-manifest-nested-"));
    try {
      for (const [subdir, files] of Object.entries(layout)) {
        mkdirSync(join(dir, subdir), { recursive: true });
        for (const [name, content] of Object.entries(files)) {
          writeFileSync(join(dir, subdir, name), content);
        }
      }
      expect(collectUpdaterArtifacts(dir)).toEqual([
        {
          target: "darwin-aarch64",
          file: "OpenLedger_0.9.0_aarch64.app.tar.gz",
          signature: "bWFjaW50cnVtc2lnbnNpZ25hdHVyZQ==",
        },
        {
          target: "windows-x86_64",
          file: "OpenLedger_0.9.0_x64-setup.exe",
          signature: "d2luZG93c21pbnNpZ25zaWc=",
        },
        {
          target: "linux-x86_64",
          file: "OpenLedger_0.9.0_amd64.AppImage",
          signature: "bGludXhtaW5zaWduc2lnbg==",
        },
      ]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("缺更新工件报错并点名平台（deb / dmg / apk 缺席不是缺工件）", () => {
    const files = { ...RELEASE_FILES };
    delete files["OpenLedger_0.9.0_x64-setup.exe"];
    delete files["OpenLedger_0.9.0_x64-setup.exe.sig"];
    const dir = fixtureDir(files);
    try {
      expect(() => collectUpdaterArtifacts(dir)).toThrow(/windows-x86_64/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("缺随附 .sig 报错（无签名的工件不被更新器接受）", () => {
    const files = { ...RELEASE_FILES };
    delete files["OpenLedger_0.9.0_amd64.AppImage.sig"];
    const dir = fixtureDir(files);
    try {
      expect(() => collectUpdaterArtifacts(dir)).toThrow(/\.sig/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("同平台工件候选不唯一报错，不给静默选错文件留口子", () => {
    const dir = fixtureDir({
      ...RELEASE_FILES,
      "Other_1.0.0_x64-setup.exe": "other-bytes",
      "Other_1.0.0_x64-setup.exe.sig": "b3RoZXJzaWc=",
    });
    try {
      expect(() => collectUpdaterArtifacts(dir)).toThrow(/候选不唯一/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("签名内容不是 base64 文本报错，疑似损坏即拒绝", () => {
    const dir = fixtureDir({
      ...RELEASE_FILES,
      "OpenLedger_0.9.0_x64-setup.exe.sig": "不是 base64！",
    });
    try {
      expect(() => collectUpdaterArtifacts(dir)).toThrow(/base64/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("buildManifest", () => {
  const artifacts = [
    {
      target: "darwin-aarch64",
      file: "OpenLedger_0.9.0_aarch64.app.tar.gz",
      signature: "bWFjaW50cnVtc2lnbnNpZ25hdHVyZQ==",
    },
    {
      target: "windows-x86_64",
      file: "OpenLedger_0.9.0_x64-setup.exe",
      signature: "d2luZG93c21pbnNpZ25zaWc=",
    },
    {
      target: "linux-x86_64",
      file: "OpenLedger_0.9.0_amd64.AppImage",
      signature: "bGludXhtaW5zaWduc2lnbg==",
    },
  ];

  it("装配 Static 形态清单：版本、发布说明、RFC3339 时间与各平台地址、签名齐全", () => {
    const manifest = JSON.parse(
      buildManifest({
        version: "0.9.0",
        notes: "升级前请阅读兼容性警告。",
        pubDate: "2026-09-27T09:00:00Z",
        repo: "asiazhang/ledger",
        tag: "v0.9.0",
        artifacts,
      }),
    );
    expect(manifest).toEqual({
      version: "0.9.0",
      notes: "升级前请阅读兼容性警告。",
      pub_date: "2026-09-27T09:00:00Z",
      platforms: {
        "darwin-aarch64": {
          url: "https://github.com/asiazhang/ledger/releases/download/v0.9.0/OpenLedger_0.9.0_aarch64.app.tar.gz",
          signature: "bWFjaW50cnVtc2lnbnNpZ25hdHVyZQ==",
        },
        "windows-x86_64": {
          url: "https://github.com/asiazhang/ledger/releases/download/v0.9.0/OpenLedger_0.9.0_x64-setup.exe",
          signature: "d2luZG93c21pbnNpZ25zaWc=",
        },
        "linux-x86_64": {
          url: "https://github.com/asiazhang/ledger/releases/download/v0.9.0/OpenLedger_0.9.0_amd64.AppImage",
          signature: "bGludXhtaW5zaWduc2lnbg==",
        },
      },
    });
  });

  it("尾部以换行收尾（文件产物形态），notes 可缺省", () => {
    const manifest = buildManifest({
      version: "0.9.0",
      notes: undefined,
      pubDate: "2026-09-27T09:00:00Z",
      repo: "asiazhang/ledger",
      tag: "v0.9.0",
      artifacts,
    });
    expect(manifest.endsWith("\n")).toBe(true);
    expect(JSON.parse(manifest).notes).toBeUndefined();
  });

  it("版本号非三段式（v 前缀 / 两段）拒绝", () => {
    expect(() => validateVersion("v0.9.0")).toThrow(/三段式/);
    expect(() => validateVersion("0.9")).toThrow(/三段式/);
    expect(() => validateVersion("0.9.0")).not.toThrow();
  });

  it("仓库标识非法拒绝", () => {
    expect(() =>
      buildManifest({
        version: "0.9.0",
        notes: undefined,
        pubDate: "2026-09-27T09:00:00Z",
        repo: "",
        tag: "v0.9.0",
        artifacts,
      }),
    ).toThrow(/owner\/name/);
  });
});

describe("parseArgs", () => {
  it("repo / tag 缺省回落 CI 环境（GITHUB_REPOSITORY / GITHUB_REF_NAME）", () => {
    const options = parseArgs(["--dir", "release", "--version", "0.9.0"], {
      GITHUB_REPOSITORY: "asiazhang/ledger",
      GITHUB_REF_NAME: "v0.9.0",
    });
    expect(options).toMatchObject({
      dir: "release",
      version: "0.9.0",
      repo: "asiazhang/ledger",
      tag: "v0.9.0",
    });
  });

  it("缺必填参数报错", () => {
    expect(() => parseArgs(["--dir", "release"], {})).toThrow(/--version/);
  });

  it("参数不成对 / 重复给出报错", () => {
    expect(() => parseArgs(["--dir"], {})).toThrow(/成对/);
    expect(() => parseArgs(["--dir", "a", "--dir", "b", "--version", "0.9.0"], {})).toThrow(/重复/);
  });
});

describe("CLI（publish job 同款拉起）", () => {
  it("--out 写文件、内容可解析且地址指向 tag 资产", () => {
    const dir = fixtureDir();
    const out = join(dir, "latest.json");
    try {
      const run = runGateScript(script, [
        "--dir",
        dir,
        "--version",
        "0.9.0",
        "--tag",
        "v0.9.0",
        "--repo",
        "asiazhang/ledger",
        "--notes-file",
        writeNotes(dir, "0.9.0 发布说明。\n"),
        "--out",
        out,
      ]);
      expect(run.status).toBe(0);
      const manifest = JSON.parse(readFileSync(out, "utf8"));
      expect(manifest.version).toBe("0.9.0");
      expect(manifest.notes).toBe("0.9.0 发布说明。");
      expect(manifest.platforms["darwin-aarch64"].url).toBe(
        "https://github.com/asiazhang/ledger/releases/download/v0.9.0/OpenLedger_0.9.0_aarch64.app.tar.gz",
      );
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("缺工件时退出码 1 并输出平台点名", () => {
    const files = { ...RELEASE_FILES };
    delete files["OpenLedger_0.9.0_aarch64.app.tar.gz"];
    delete files["OpenLedger_0.9.0_aarch64.app.tar.gz.sig"];
    const dir = fixtureDir(files);
    try {
      const run = runGateScript(script, [
        "--dir",
        dir,
        "--version",
        "0.9.0",
        "--tag",
        "v0.9.0",
        "--repo",
        "asiazhang/ledger",
      ]);
      expect(run.status).toBe(1);
      expect(run.output).toContain("darwin-aarch64");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("缺必填参数退出码 1 并带用法", () => {
    const run = runGateScript(script, ["--version", "0.9.0"]);
    expect(run.status).toBe(1);
    expect(run.output).toContain("用法");
  });

  function writeNotes(dir: string, content: string): string {
    const path = join(dir, "notes.md");
    writeFileSync(path, content);
    return path;
  }
});
