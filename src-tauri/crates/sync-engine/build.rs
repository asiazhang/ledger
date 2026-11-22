fn main() {
    use std::env;

    println!("cargo::rustc-check-cfg=cfg(desktop)");
    println!("cargo::rustc-check-cfg=cfg(mobile)");
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("android") | Ok("ios") => println!("cargo::rustc-cfg=mobile"),
        _ => println!("cargo::rustc-cfg=desktop"),
    }
}
