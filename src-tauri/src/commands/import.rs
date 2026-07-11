use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::{ImportRequest, ImportedRow};

#[tauri::command]
pub fn preview_import(db: State<'_, DbState>, req: ImportRequest) -> Result<Vec<ImportedRow>> {
    let _ = &db;
    let path = req.path.as_str();
    if path.to_lowercase().ends_with(".csv") {
        parse_csv(path)
    } else if let Some(ext) = path.rsplit('.').next()
        && matches!(ext.to_lowercase().as_str(), "xlsx" | "xls")
    {
        parse_excel(path)
    } else {
        Err(AppError::Invalid("仅支持 .csv / .xlsx / .xls 文件".into()))
    }
}

fn parse_csv(path: &str) -> Result<Vec<ImportedRow>> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)?;
    let headers: std::collections::HashMap<String, usize> = rdr
        .headers()?
        .iter()
        .enumerate()
        .map(|(i, h)| (h.trim().to_lowercase(), i))
        .collect();
    let date_idx = headers.get("date").or_else(|| headers.get("日期"));
    let amount_idx = headers.get("amount").or_else(|| headers.get("金额"));
    let note_idx = headers
        .get("note")
        .or_else(|| headers.get("备注"))
        .or_else(|| headers.get("描述"));
    let cat_idx = headers.get("category").or_else(|| headers.get("分类"));
    let mut out = Vec::new();
    for record in rdr.records() {
        let record = record?;
        let date = date_idx
            .and_then(|i| record.get(*i))
            .unwrap_or("")
            .trim()
            .to_string();
        let amount_raw = amount_idx.and_then(|i| record.get(*i)).unwrap_or("").trim();
        let amount_cents = parse_amount_cents(amount_raw)?;
        let note = note_idx
            .and_then(|i| record.get(*i))
            .unwrap_or("")
            .trim()
            .to_string();
        let category_name = cat_idx
            .and_then(|i| record.get(*i))
            .map(|s| s.trim().to_string());
        if date.is_empty() {
            continue;
        }
        out.push(ImportedRow {
            date,
            amount_cents,
            note,
            category_name,
        });
    }
    Ok(out)
}

fn parse_excel(path: &str) -> Result<Vec<ImportedRow>> {
    use calamine::{Reader, open_workbook_auto};
    let mut workbook =
        open_workbook_auto(path).map_err(|e| AppError::Parse(format!("打开 Excel 失败: {e}")))?;
    let sheet = workbook
        .worksheets()
        .first()
        .map(|(name, _)| name.clone())
        .ok_or_else(|| AppError::Parse("Excel 无工作表".into()))?;
    let range = workbook
        .worksheet_range(&sheet)
        .map_err(|e| AppError::Parse(format!("读取工作表失败: {e}")))?;
    let mut iter = range.rows();
    let header = iter
        .next()
        .ok_or_else(|| AppError::Parse("Excel 无表头".into()))?;
    let header_map: std::collections::HashMap<String, usize> = header
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let s = c.to_string().trim().to_lowercase();
            if s.is_empty() { None } else { Some((s, i)) }
        })
        .collect();
    let date_idx = header_map.get("date").or_else(|| header_map.get("日期"));
    let amount_idx = header_map.get("amount").or_else(|| header_map.get("金额"));
    let note_idx = header_map
        .get("note")
        .or_else(|| header_map.get("备注"))
        .or_else(|| header_map.get("描述"));
    let cat_idx = header_map
        .get("category")
        .or_else(|| header_map.get("分类"));
    let mut out = Vec::new();
    for row in iter {
        let cell = |i: &usize| -> String {
            row.get(*i)
                .map(|c| c.to_string())
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        let date = date_idx.map(cell).unwrap_or_default();
        let amount_raw = amount_idx.map(cell).unwrap_or_default();
        let amount_cents = parse_amount_cents(amount_raw.as_str())?;
        let note = note_idx.map(cell).unwrap_or_default();
        let category_name = cat_idx.map(cell);
        if date.is_empty() {
            continue;
        }
        out.push(ImportedRow {
            date,
            amount_cents,
            note,
            category_name,
        });
    }
    Ok(out)
}

/// 将字符串金额转为整数分。支持 "12.34"、"1,234.56"、负数。
fn parse_amount_cents(raw: &str) -> Result<i64> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',')
        .collect();
    if cleaned.is_empty() {
        return Ok(0);
    }
    let parsed: f64 = cleaned
        .parse()
        .map_err(|e| AppError::Parse(format!("无法解析金额 '{raw}': {e}")))?;
    Ok((parsed * 100.0).round() as i64)
}
