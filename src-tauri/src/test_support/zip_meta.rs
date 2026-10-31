//! 共享「备份包内层元数据注入」器具（spec #1982 review，ADR-0084 准入：备份
//! 域单测夹具与 BDD e2e 步骤层 ≥2 处同体消费）：把外来字段并入既有备份产物
//! 的内层元数据（`backup.json`）——逐条目原样复制、元数据条目合并重写（整包
//! 重建，测试侧一次性动作）。
//!
//! **为什么共享**：模拟「另一台设备 / 另一本账」产物水印时，两处曾各持一份
//! ~30 行的 zip 整包重写逻辑（读全条目 → 重建 zip → 并入 `backup.json` →
//! 回写），属目录级测试薄壳硬违规（CONTEXT-testing）。共享的是**备份包重写
//! 动作**，不是产物语义——水印字段取值仍由各调用方给定。

use std::io::Read;
use std::path::Path;

/// 把 `extra`（JSON 对象）的字段并入 `path` 处备份产物的 `backup.json`：
/// 逐条目原样复制、元数据条目合并重写（整包重建，测试侧一次性动作）。
/// 供备份域单测（crates/backup/src/tests.rs，经 dev-dependency 消费）与
/// e2e 步骤层（tests/e2e/backup_steps.rs）共用。
pub fn merge_extra_backup_meta(path: &Path, extra: &serde_json::Value) {
    let file = std::fs::File::open(path).expect("打开备份包失败");
    let mut archive = zip::ZipArchive::new(file).expect("不是有效 zip 备份包");
    let mut buf = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let options = zip::write::SimpleFileOptions::default();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            let name = entry.name().unwrap().to_string();
            writer.start_file(name.as_str(), options).unwrap();
            if name == "backup.json" {
                let mut text = String::new();
                entry.read_to_string(&mut text).unwrap();
                let mut meta: serde_json::Value = serde_json::from_str(&text).unwrap();
                let obj = meta.as_object_mut().unwrap();
                for (k, v) in extra.as_object().expect("extra 须为 JSON 对象") {
                    obj.insert(k.clone(), v.clone());
                }
                std::io::Write::write_all(
                    &mut writer,
                    serde_json::to_string(&meta).unwrap().as_bytes(),
                )
                .unwrap();
            } else {
                std::io::copy(&mut entry, &mut writer).unwrap();
            }
        }
        writer.finish().unwrap();
    }
    std::fs::write(path, buf).expect("回写备份包失败");
}
