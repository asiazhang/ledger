export function gitShaFull(): string {
  if (typeof __GIT_SHA__ === "string") return __GIT_SHA__;
  return "";
}

function isGitDirty(): boolean {
  return typeof __GIT_DIRTY__ === "boolean" && __GIT_DIRTY__;
}

export function gitVersionLabel(): string {
  const sha = gitShaFull();
  if (!sha) return "";
  return sha.slice(0, 7) + (isGitDirty() ? "-dirty" : "");
}
