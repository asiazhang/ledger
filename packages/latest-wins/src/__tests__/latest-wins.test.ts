import { describe, it, expect } from "vitest";
import { createLatestWins } from "../latest-wins";

describe("createLatestWins（最新胜出竞态纪元共享 module，issue #1678）", () => {
  it("begin 使此前 token 全部过期：旧请求结果不覆盖新请求结果（最新胜出）", () => {
    const wins = createLatestWins();
    const first = wins.begin();
    expect(first.isStale()).toBe(false);

    const second = wins.begin();
    expect(first.isStale()).toBe(true);
    expect(second.isStale()).toBe(false);

    // 多次推进只认最后一次
    const third = wins.begin();
    expect(second.isStale()).toBe(true);
    expect(third.isStale()).toBe(false);
  });

  it("observe 采样当前纪元不推进：并发采样共享同一裁决点，仅后续推进使其过期", () => {
    const wins = createLatestWins();
    const s1 = wins.observe();
    const s2 = wins.observe();
    expect(s1.isStale()).toBe(false);
    expect(s2.isStale()).toBe(false);

    // 采样不推进：采样之间互不作废（push-first-list refresh 在途合并语义）
    const s3 = wins.observe();
    expect(s1.isStale()).toBe(false);
    expect(s3.isStale()).toBe(false);

    // 后续推进使全部采样 token 一并过期
    wins.invalidate();
    expect(s1.isStale()).toBe(true);
    expect(s2.isStale()).toBe(true);
    expect(s3.isStale()).toBe(true);
  });

  it("invalidate 推进作废全部在途：begin 与 observe 的 token 一并过期（清空/重置出口）", () => {
    const wins = createLatestWins();
    const begun = wins.begin();
    const sampled = wins.observe();
    wins.invalidate();
    expect(begun.isStale()).toBe(true);
    expect(sampled.isStale()).toBe(true);

    // invalidate 后新发起照常取得未过期 token
    const fresh = wins.begin();
    expect(fresh.isStale()).toBe(false);
  });

  it("并发发起的胜出序：交错 begin/invalidate 后只有最后一次推进时的最新 token 未过期", () => {
    const wins = createLatestWins();
    const a = wins.begin();
    const b = wins.begin();
    wins.invalidate();
    const c = wins.begin();
    expect(a.isStale()).toBe(true);
    expect(b.isStale()).toBe(true);
    expect(c.isStale()).toBe(false);
    wins.invalidate();
    expect(c.isStale()).toBe(true);
  });
});
