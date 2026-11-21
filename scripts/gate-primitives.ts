#!/usr/bin/env bun

import { readdirSync } from "node:fs";
import { join } from "node:path";

export interface WalkedFile {
  abs: string;
  rel: string;
}

export interface WalkTextOptions {
  extensions: ReadonlySet<string>;
  skipDirs?: ReadonlySet<string>;
  skipFiles?: ReadonlySet<string>;
}

export function walkTextFiles(
  dir: string,
  relBase: string,
  options: WalkTextOptions,
): WalkedFile[] {
  const out: WalkedFile[] = [];
  const walk = (current: string, rel: string): void => {
    for (const entry of readdirSync(current, { withFileTypes: true }).sort((a, b) =>
      a.name.localeCompare(b.name),
    )) {
      const entryAbs = join(current, entry.name);
      const entryRel = rel ? `${rel}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        if (!options.skipDirs?.has(entry.name)) walk(entryAbs, entryRel);
        continue;
      }
      if (!hasExtension(entry.name, options.extensions)) continue;
      if (options.skipFiles?.has(entryRel)) continue;
      out.push({ abs: entryAbs, rel: entryRel });
    }
  };
  walk(dir, relBase);
  return out;
}

export interface DirEntry {
  name: string;
  isDirectory: boolean;
  isFile: boolean;
}

export function readDirEntries(dir: string): DirEntry[] {
  return readdirSync(dir, { withFileTypes: true })
    .sort((a, b) => a.name.localeCompare(b.name))
    .map((entry) => ({
      name: entry.name,
      isDirectory: entry.isDirectory(),
      isFile: entry.isFile(),
    }));
}

export function lineAt(text: string, index: number | undefined): number {
  return (text.slice(0, index ?? 0).match(/\n/g)?.length ?? 0) + 1;
}

function rawStringOpenQuoteAt(text: string, i: number): number | null {
  const prev = i > 0 ? text[i - 1] : "";
  if (/[A-Za-z0-9_]/.test(prev)) return null;
  let j = i;
  if (text[j] === "b" && text[j + 1] === "r") j += 2;
  else if (text[j] === "r") j += 1;
  else return null;
  while (text[j] === "#") j++;
  return text[j] === '"' ? j : null;
}

export function maskNonCode(text: string, keepLiterals = false): string {
  const out = text.split("");
  const n = text.length;
  const blank = (from: number, to: number): void => {
    for (let k = from; k < to && k < n; k++) if (out[k] !== "\n") out[k] = " ";
  };
  let i = 0;
  while (i < n) {
    const c = text[i];
    if (c === "/" && text[i + 1] === "/") {
      const end = text.indexOf("\n", i);
      const stop = end === -1 ? n : end;
      blank(i, stop);
      i = stop;
    } else if (c === "/" && text[i + 1] === "*") {
      let depth = 1;
      let j = i + 2;
      while (j < n && depth > 0) {
        if (text[j] === "/" && text[j + 1] === "*") {
          depth++;
          j += 2;
        } else if (text[j] === "*" && text[j + 1] === "/") {
          depth--;
          j += 2;
        } else {
          j++;
        }
      }
      blank(i, j);
      i = j;
    } else if (c === '"') {
      let j = i + 1;
      while (j < n) {
        if (text[j] === "\\") j += 2;
        else if (text[j] === '"') {
          j++;
          break;
        } else j++;
      }
      if (!keepLiterals) blank(i, j);
      i = j;
    } else if (c === "r" || c === "b") {
      const open = rawStringOpenQuoteAt(text, i);
      if (open === null) {
        i++;
        continue;
      }
      const prefixEnd = c === "b" ? i + 2 : i + 1;
      const hashes = open - prefixEnd;
      const close = '"' + "#".repeat(hashes);
      const end = text.indexOf(close, open + 1);
      const stop = end === -1 ? n : end + close.length;
      if (!keepLiterals) blank(i, stop);
      i = stop;
    } else if (c === "'") {
      let j = i + 1;
      if (text[j] === "\\") {
        j++;
        if (text[j] === "{") {
          const e = text.indexOf("}", j);
          j = e === -1 ? n : e + 1;
        } else {
          j++;
        }
      } else {
        j++;
      }
      if (text[j] === "'") {
        const stop = j + 1;
        if (!keepLiterals) blank(i, stop);
        i = stop;
      } else {
        i++;
      }
    } else {
      i++;
    }
  }
  return out.join("");
}

export function hasExtension(name: string, extensions: ReadonlySet<string>): boolean {
  const dot = name.lastIndexOf(".");
  return dot !== -1 && extensions.has(name.slice(dot));
}

export const RUST_EXTENSIONS: ReadonlySet<string> = new Set([".rs"]);

function stringLiteralEnd(text: string, start: number): number {
  const quote = text[start];
  if (quote !== "'" && quote !== '"' && quote !== "`") return -1;
  let i = start + 1;
  while (i < text.length) {
    if (text[i] === "\\") {
      i += 2;
      continue;
    }
    if (text[i] === quote) return i + 1;
    i++;
  }
  return text.length;
}

export function maskComments(text: string): string {
  const out = text.split("");
  const n = text.length;
  const blank = (from: number, to: number): void => {
    for (let k = from; k < to && k < n; k++) if (out[k] !== "\n") out[k] = " ";
  };
  let i = 0;
  while (i < n) {
    if (text[i] === "/" && text[i - 1] !== "\\" && text[i + 1] === "/") {
      const stop = text.indexOf("\n", i) === -1 ? n : text.indexOf("\n", i);
      blank(i, stop);
      i = stop;
    } else if (text[i] === "/" && text[i - 1] !== "\\" && text[i + 1] === "*") {
      const end = text.indexOf("*/", i + 2);
      const stop = end === -1 ? n : end + 2;
      blank(i, stop);
      i = stop;
    } else {
      const literalEnd = stringLiteralEnd(text, i);
      i = literalEnd === -1 ? i + 1 : literalEnd;
    }
  }
  return out.join("");
}
