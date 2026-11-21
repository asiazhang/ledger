import { pinyin } from "pinyin-pro";
import type { SelectOption } from "naive-ui";

const initialsCache = new Map<string, string>();

export function pinyinInitials(text: string): string {
  const cached = initialsCache.get(text);
  if (cached !== undefined) return cached;

  const raw = pinyin(text, { pattern: "first", toneType: "none", type: "array" }).join("");
  let out = "";
  for (const ch of raw) {
    if (/[a-z0-9]/i.test(ch)) out += ch.toLowerCase();
  }
  initialsCache.set(text, out);
  return out;
}

export function isSubsequence(pattern: string, target: string): boolean {
  const targetChars = [...target.toLowerCase()];
  let cursor = 0;
  for (const p of pattern.toLowerCase()) {
    let found = false;
    while (cursor < targetChars.length) {
      if (targetChars[cursor] === p) {
        found = true;
        cursor++;
        break;
      }
      cursor++;
    }
    if (!found) return false;
  }
  return true;
}

function termMatchesText(term: string, text: string): boolean {
  return (
    text.toLowerCase().includes(term.toLowerCase()) || isSubsequence(term, pinyinInitials(text))
  );
}

export function matchLabel(pattern: string, label: string): boolean {
  return pattern
    .split(/\s+/)
    .filter(Boolean)
    .every((term) => termMatchesText(term, label));
}

export function pinyinFilter(pattern: string, option: SelectOption): boolean {
  const { label } = option;
  if (typeof label === "string") return matchLabel(pattern, label);
  if (typeof label === "number") return matchLabel(pattern, String(label));
  return true;
}
