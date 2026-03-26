#!/usr/bin/env python3
"""
Migrate Rune data from SQLite to PostgreSQL.

Usage:
    python3 scripts/migrate-sqlite-to-pg.py \
        --sqlite /home/hamza/.rune/db/rune.db \
        --pg "postgres://citus:PASSWORD@host:5432/citus?sslmode=require"

Tables migrated: sessions, turns, transcript_items, jobs, job_runs
"""

import argparse
import sqlite3
import ssl
import sys
from datetime import datetime

try:
    import psycopg2
    import psycopg2.extras
except ImportError:
    print("pip install psycopg2-binary")
    sys.exit(1)


def connect_pg(url: str):
    """Connect to PG with SSL if sslmode=require."""
    # Parse URL manually for Azure Cosmos DB compatibility
    from urllib.parse import urlparse, parse_qs
    parsed = urlparse(url)
    params = {
        "host": parsed.hostname,
        "port": parsed.port or 5432,
        "dbname": parsed.path.lstrip("/"),
        "user": parsed.username,
        "password": parsed.password,
    }
    qs = parse_qs(parsed.query)
    if "sslmode" in qs:
        params["sslmode"] = qs["sslmode"][0]
    return psycopg2.connect(**params)


def get_sqlite_columns(cur, table: str) -> list[str]:
    cur.execute(f"PRAGMA table_info([{table}])")
    return [row[1] for row in cur.fetchall()]


def migrate_table(sqlite_conn, pg_conn, table: str, pg_columns: list[str] | None = None):
    """Migrate a single table from SQLite to PG."""
    sqlite_cur = sqlite_conn.cursor()
    pg_cur = pg_conn.cursor()

    # Get column names from SQLite
    sqlite_cols = get_sqlite_columns(sqlite_cur, table)

    # If PG columns specified, use intersection
    if pg_columns:
        cols = [c for c in sqlite_cols if c in pg_columns]
    else:
        cols = sqlite_cols

    if not cols:
        print(f"  {table}: no matching columns, skipping")
        return 0

    # Read all rows
    col_list = ", ".join(f"[{c}]" for c in cols)
    sqlite_cur.execute(f"SELECT {col_list} FROM [{table}]")
    rows = sqlite_cur.fetchall()

    if not rows:
        print(f"  {table}: 0 rows, skipping")
        return 0

    # Build INSERT statement with ON CONFLICT DO NOTHING
    pg_col_list = ", ".join(f'"{c}"' for c in cols)
    placeholders = ", ".join(["%s"] * len(cols))
    insert_sql = f'INSERT INTO "{table}" ({pg_col_list}) VALUES ({placeholders}) ON CONFLICT DO NOTHING'

    # Batch insert
    try:
        psycopg2.extras.execute_batch(pg_cur, insert_sql, rows, page_size=500)
        pg_conn.commit()
        print(f"  {table}: {len(rows)} rows migrated")
        return len(rows)
    except Exception as e:
        pg_conn.rollback()
        print(f"  {table}: ERROR — {e}")
        # Try row by row to find the problematic row
        errors = 0
        inserted = 0
        for i, row in enumerate(rows):
            try:
                pg_cur.execute(insert_sql, row)
                pg_conn.commit()
                inserted += 1
            except Exception as row_err:
                pg_conn.rollback()
                errors += 1
                if errors <= 3:
                    print(f"    row {i} error: {row_err}")
        print(f"  {table}: {inserted} inserted, {errors} errors (row-by-row fallback)")
        return inserted


def main():
    parser = argparse.ArgumentParser(description="Migrate Rune data from SQLite to PostgreSQL")
    parser.add_argument("--sqlite", required=True, help="Path to SQLite database")
    parser.add_argument("--pg", required=True, help="PostgreSQL connection URL")
    parser.add_argument("--tables", default="sessions,turns,transcript_items,jobs,job_runs",
                       help="Comma-separated list of tables to migrate")
    args = parser.parse_args()

    print(f"Source: {args.sqlite}")
    print(f"Target: PostgreSQL")
    print()

    # Connect
    sqlite_conn = sqlite3.connect(args.sqlite)
    sqlite_conn.row_factory = None  # tuples
    pg_conn = connect_pg(args.pg)

    tables = [t.strip() for t in args.tables.split(",")]
    total = 0

    print("Migrating tables:")
    for table in tables:
        count = migrate_table(sqlite_conn, pg_conn, table)
        total += count

    print(f"\nDone. Total rows migrated: {total}")

    sqlite_conn.close()
    pg_conn.close()


if __name__ == "__main__":
    main()
