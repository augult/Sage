"""The shop's database."""

import sqlite3

SCHEMA = """
CREATE TABLE customers (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    region TEXT NOT NULL
);
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    customer_id INTEGER NOT NULL,
    placed_on TEXT NOT NULL,      -- YYYY-MM-DD
    status TEXT NOT NULL,         -- open, paid, shipped or cancelled
    currency TEXT NOT NULL
);
CREATE TABLE items (
    id INTEGER PRIMARY KEY,
    order_id INTEGER NOT NULL,
    sku TEXT NOT NULL,
    qty INTEGER NOT NULL,
    unit_cents INTEGER NOT NULL,
    discount_pct INTEGER          -- whole percent, NULL for none
);
"""


def connect(path=":memory:"):
    return sqlite3.connect(path)


def create(conn):
    conn.executescript(SCHEMA)
