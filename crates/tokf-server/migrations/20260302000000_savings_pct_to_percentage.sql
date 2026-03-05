-- Convert savings_pct from fraction (0.0–1.0) to percentage (0–100).
UPDATE filter_stats SET savings_pct = savings_pct * 100.0;
