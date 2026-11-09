fn main() {
    // 分平台门（ADR-0098 决策 4）：`desktop` / `mobile` 由 Tauri 构建体系注入；
    // 本 crate 作为 workspace 成员独立编译时不会继承根包的 `tauri_build::build()`
    // 注入，故在此按目标 OS 重放同一门（桌面 = 非 Android/iOS），既登记条件名、
    // 也保证 `#[cfg(desktop)]` 的平台分流与拆 crate 前逐字节一致。
    use std::env;

    println!("cargo::rustc-check-cfg=cfg(desktop)");
    println!("cargo::rustc-check-cfg=cfg(mobile)");
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("android") | Ok("ios") => println!("cargo::rustc-cfg=mobile"),
        _ => println!("cargo::rustc-cfg=desktop"),
    }
}
