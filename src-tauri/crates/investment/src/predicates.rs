pub const INVESTED_EXISTS: &str = "EXISTS (SELECT 1 FROM security_lots l WHERE l.instrument_id=i.id \
     AND l.remaining_quantity > 0 \
     AND l.account_id IN (SELECT id FROM accounts WHERE is_deleted = 0))";
