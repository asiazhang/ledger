use std::collections::HashMap;

use crate::error::{AppError, Result};
use crate::models::ImportedRow;

struct ColumnIndices {
    date: Option<usize>,
    amount: Option<usize>,
    note: Option<usize>,
    category: Option<usize>,
}

fn match_columns(header: &HashMap<String, usize>) -> ColumnIndices {
    ColumnIndices {
        date: header.get("date").or_else(|| header.get("日期")).copied(),
        amount: header.get("amount").or_else(|| header.get("金额")).copied(),
        note: header
            .get("note")
            .or_else(|| header.get("备注"))
            .or_else(|| header.get("描述"))
            .copied(),
        category: header
            .get("category")
            .or_else(|| header.get("分类"))
            .copied(),
    }
}

pub fn parse_amount_cents(raw: &str) -> Result<i64> {
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

fn make_imported_row(
    date: String,
    amount_cents: i64,
    note: String,
    category_name: Option<String>,
) -> ImportedRow {
    let kind = if amount_cents >= 0 { "income" } else { "expense" };
    ImportedRow {
        date,
        kind: kind.to_string(),
        amount_cents: amount_cents.abs(),
        note,
        category_name,
    }
}

pub fn parse_csv(path: &str) -> Result<Vec<ImportedRow>> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)?;
    let headers: HashMap<String, usize> = rdr
        .headers()?
        .iter()
        .enumerate()
        .map(|(i, h)| (h.trim().to_lowercase(), i))
        .collect();
    let idx = match_columns(&headers);
    let mut out = Vec::new();
    for record in rdr.records() {
        let record = record?;
        let date = idx
            .date
            .and_then(|i| record.get(i))
            .unwrap_or("")
            .trim()
            .to_string();
        let amount_raw = idx.amount.and_then(|i| record.get(i)).unwrap_or("").trim();
        let amount_cents = parse_amount_cents(amount_raw)?;
        let note = idx
            .note
            .and_then(|i| record.get(i))
            .unwrap_or("")
            .trim()
            .to_string();
        let category_name = idx
            .category
            .and_then(|i| record.get(i))
            .map(|s| s.trim().to_string());
        if date.is_empty() {
            continue;
        }
        out.push(make_imported_row(date, amount_cents, note, category_name));
    }
    Ok(out)
}

pub fn parse_excel(path: &str) -> Result<Vec<ImportedRow>> {
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
    let header_map: HashMap<String, usize> = header
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let s = c.to_string().trim().to_lowercase();
            if s.is_empty() { None } else { Some((s, i)) }
        })
        .collect();
    let idx = match_columns(&header_map);
    let mut out = Vec::new();
    for row in iter {
        let cell = |i: usize| -> String {
            row.get(i)
                .map(|c| c.to_string())
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        let date = idx.date.map(cell).unwrap_or_default();
        let amount_raw = idx.amount.map(cell).unwrap_or_default();
        let amount_cents = parse_amount_cents(amount_raw.as_str())?;
        let note = idx.note.map(cell).unwrap_or_default();
        let category_name = idx.category.map(cell);
        if date.is_empty() {
            continue;
        }
        out.push(make_imported_row(date, amount_cents, note, category_name));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_amount_cents_basic() {
        assert_eq!(parse_amount_cents("12.34").unwrap(), 1234);
        assert_eq!(parse_amount_cents("0").unwrap(), 0);
        assert_eq!(parse_amount_cents("100").unwrap(), 10000);
    }

    #[test]
    fn test_parse_amount_cents_thousands_separator() {
        assert_eq!(parse_amount_cents("1,234.56").unwrap(), 123456);
        assert_eq!(parse_amount_cents("10,000").unwrap(), 1_000_000);
    }

    #[test]
    fn test_parse_amount_cents_negative() {
        assert_eq!(parse_amount_cents("-50.00").unwrap(), -5000);
        assert_eq!(parse_amount_cents("-1,234.56").unwrap(), -123456);
    }

    #[test]
    fn test_parse_amount_cents_empty() {
        assert_eq!(parse_amount_cents("").unwrap(), 0);
        assert_eq!(parse_amount_cents("   ").unwrap(), 0);
    }

    #[test]
    fn test_parse_amount_cents_whitespace() {
        assert_eq!(parse_amount_cents(" 12.34 ").unwrap(), 1234);
    }

    #[test]
    fn test_parse_amount_cents_invalid() {
        assert!(parse_amount_cents("abc").is_err());
        assert!(parse_amount_cents("12.34.56").is_err());
    }

    #[test]
    fn test_match_columns_chinese() {
        let mut h = HashMap::new();
        h.insert("日期".into(), 0);
        h.insert("金额".into(), 1);
        h.insert("备注".into(), 2);
        h.insert("分类".into(), 3);
        let idx = match_columns(&h);
        assert_eq!(idx.date, Some(0));
        assert_eq!(idx.amount, Some(1));
        assert_eq!(idx.note, Some(2));
        assert_eq!(idx.category, Some(3));
    }

    #[test]
    fn test_match_columns_english() {
        let mut h = HashMap::new();
        h.insert("date".into(), 0);
        h.insert("amount".into(), 1);
        h.insert("note".into(), 2);
        h.insert("category".into(), 3);
        let idx = match_columns(&h);
        assert_eq!(idx.date, Some(0));
        assert_eq!(idx.amount, Some(1));
        assert_eq!(idx.note, Some(2));
        assert_eq!(idx.category, Some(3));
    }

    #[test]
    fn test_match_columns_partial() {
        let mut h = HashMap::new();
        h.insert("date".into(), 0);
        h.insert("金额".into(), 1);
        let idx = match_columns(&h);
        assert_eq!(idx.date, Some(0));
        assert_eq!(idx.amount, Some(1));
        assert_eq!(idx.note, None);
        assert_eq!(idx.category, None);
    }

    #[test]
    fn test_match_columns_note_aliases() {
        let mut h = HashMap::new();
        h.insert("描述".into(), 2);
        let idx = match_columns(&h);
        assert_eq!(idx.note, Some(2));
    }

    #[test]
    fn test_parse_csv_no_file() {
        let result = parse_csv("/nonexistent/file.csv");
        assert!(result.is_err());
    }

    #[test]
    fn test_make_imported_row_income() {
        let row = make_imported_row("2026-01-01".into(), 5000, "salary".into(), None);
        assert_eq!(row.kind, "income");
        assert_eq!(row.amount_cents, 5000);
        assert_eq!(row.date, "2026-01-01");
    }

    #[test]
    fn test_make_imported_row_expense() {
        let row = make_imported_row("2026-01-02".into(), -1234, "lunch".into(), None);
        assert_eq!(row.kind, "expense");
        assert_eq!(row.amount_cents, 1234);
    }

    #[test]
    fn test_make_imported_row_zero() {
        let row = make_imported_row("2026-01-03".into(), 0, "free".into(), None);
        assert_eq!(row.kind, "income");
        assert_eq!(row.amount_cents, 0);
    }

    #[test]
    fn test_make_imported_row_with_category() {
        let row = make_imported_row(
            "2026-01-04".into(),
            -9999,
            "groceries".into(),
            Some("Food".into()),
        );
        assert_eq!(row.kind, "expense");
        assert_eq!(row.amount_cents, 9999);
        assert_eq!(row.category_name.as_deref(), Some("Food"));
    }
}
