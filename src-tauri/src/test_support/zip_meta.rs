use std::io::Read;
use std::path::Path;

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
