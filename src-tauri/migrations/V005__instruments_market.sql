ALTER TABLE instruments ADD COLUMN market TEXT NOT NULL CHECK(market IN ('sh','sz','hk','unknown')) DEFAULT 'unknown';
