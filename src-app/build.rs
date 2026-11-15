//! 构建期元信息注入（#2136 设置③「关于」页签）：把 Git 版本信息编译进二进制
//! ——`NSH_GIT_SHA`（完整 40 位，`git rev-parse HEAD`）与 `NSH_GIT_DIRTY`
//! （构建时工作树是否含未提交改动）。与 Web 参照前端 `vite define` 注入的
//! `__GIT_SHA__` / `__GIT_DIRTY__` 同口径（packages/utils git-info 同一事实源
//! 的两侧载体）；非 Git 目录 / git 命令失败降级为空值/不脏，「关于」卡按
//! 「宁缺不填假值」不渲染 Git 行。
//!
//! rerun 策略：`.git`（仓库或 worktree 的 gitdir 指针 / 目录）变化才重跑——
//! 普通源码改动不触发（构建脚本零依赖源码面）。

fn main() {
    println!("cargo:rerun-if-changed=.git");
    let sha = run(&["rev-parse", "HEAD"]);
    println!(
        "cargo:rustc-env=NSH_GIT_SHA={}",
        sha.as_deref().unwrap_or("")
    );
    let dirty = run(&["status", "--porcelain"])
        .map(|out| !out.trim().is_empty())
        .unwrap_or(false)
        // rev-parse 失败（非 Git 目录）时整体降级不脏：空 sha 下脏位无意义。
        && sha.is_some();
    println!("cargo:rustc-env=NSH_GIT_DIRTY={}", dirty);
}

/// 在本 crate 根目录跑一条 git 命令（失败返回 None，调用方降级）。
fn run(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
