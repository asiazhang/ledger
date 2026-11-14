fn main() {
    // 分平台门（ADR-0098 决策 4）：`desktop` / `mobile` 原由 Tauri 构建体系注入
    //（`tauri_build::build()` 随 #2083 旧壳退役），本 build.rs 是该门全仓唯一
    // 注入点——按目标 OS 重放同一门（桌面 = 非 Android/iOS），既登记条件名、
    // 也保证 `#[cfg(desktop)]` 的平台分流与拆 crate 前逐字节一致。
    use std::env;

    println!("cargo::rustc-check-cfg=cfg(desktop)");
    println!("cargo::rustc-check-cfg=cfg(mobile)");
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("android") | Ok("ios") => println!("cargo::rustc-cfg=mobile"),
        _ => println!("cargo::rustc-cfg=desktop"),
    }
}
