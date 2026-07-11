use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::import_parser;
use crate::models::{ImportRequest, ImportedRow};

#[tauri::command]
pub fn preview_import(db: State<'_, DbState>, req: ImportRequest) -> Result<Vec<ImportedRow>> {
    let _ = &db;
    let path = req.path.as_str();
    if path.to_lowercase().ends_with(".csv") {
        import_parser::parse_csv(path)
    } else if let Some(ext) = path.rsplit('.').next()
        && matches!(ext.to_lowercase().as_str(), "xlsx" | "xls")
    {
        import_parser::parse_excel(path)
    } else {
        Err(AppError::Invalid("仅支持 .csv / .xlsx / .xls 文件".into()))
    }
}
