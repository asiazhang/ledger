use tauri::State;

use crate::db::DbState;
use crate::db::query::query_all;
use crate::error::Result;
use crate::models::Currency;

#[tauri::command]
pub fn list_currencies(db: State<'_, DbState>) -> Result<Vec<Currency>> {
    let conn = db
        .conn
        .lock()
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    query_all(
        &conn,
        "SELECT code,name,symbol,decimal_places FROM currencies ORDER BY code",
        [],
    )
}

#[cfg(test)]
mod tests {
    use crate::db::query::query_all;
    use crate::models::Currency;

    fn setup() -> rusqlite::Connection {
        let mut conn = crate::db::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    #[test]
    fn list_currencies_returns_all_seed_currencies() {
        let conn = setup();
        let currencies: Vec<Currency> = query_all(
            &conn,
            "SELECT code,name,symbol,decimal_places FROM currencies ORDER BY code",
            [],
        )
        .unwrap();
        assert_eq!(currencies.len(), 11);
        assert!(currencies.iter().any(|c| c.code == "CNY"));
        assert!(currencies.iter().any(|c| c.code == "USD"));
        assert!(currencies.iter().any(|c| c.code == "EUR"));
        assert!(currencies.iter().any(|c| c.code == "HKD"));
    }

    #[test]
    fn currencies_have_correct_decimal_places() {
        let conn = setup();
        let currencies: Vec<Currency> = query_all(
            &conn,
            "SELECT code,name,symbol,decimal_places FROM currencies ORDER BY code",
            [],
        )
        .unwrap();
        for c in &currencies {
            assert!(
                c.decimal_places >= 0,
                "{} decimal_places is negative",
                c.code
            );
        }
        let cny = currencies.iter().find(|c| c.code == "CNY").unwrap();
        assert_eq!(cny.decimal_places, 2);
        let usd = currencies.iter().find(|c| c.code == "USD").unwrap();
        assert_eq!(usd.decimal_places, 2);
    }
}
