"""The customer report: what every customer has bought, in euro cents."""

import json


def load_rates(path):
    """currency -> millionths of a euro per unit, e.g. {"USD": 921500}."""
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def _half_up(numerator, denominator):
    return (2 * numerator + denominator) // (2 * denominator)


def order_total(conn, order_id):
    """The order's total in its own currency: each line is rounded to a whole cent, halves up."""
    total = 0
    rows = conn.execute("SELECT qty, unit_cents, discount_pct FROM items WHERE order_id = ?", (order_id,))
    for qty, unit_cents, discount_pct in rows:
        total += _half_up(qty * unit_cents * (100 - (discount_pct or 0)), 100)
    return total


def top_sku(conn, customer_id):
    """The SKU the customer bought most units of; among equals the alphabetically first."""
    units = {}
    orders = conn.execute("SELECT id FROM orders WHERE customer_id = ? AND status != 'cancelled'", (customer_id,))
    for (order_id,) in orders.fetchall():
        for sku, qty in conn.execute("SELECT sku, qty FROM items WHERE order_id = ?", (order_id,)):
            units[sku] = units.get(sku, 0) + qty
    if not units:
        return None
    return min(units, key=lambda sku: (-units[sku], sku))


def customer_report(conn, rates_path="rates.json"):
    """One dict per customer, including those who never ordered:

    customer     the name
    region       the region
    orders       how many orders, not counting cancelled ones
    total_cents  what those orders come to in euro cents: each order's total is converted with its
                 currency's rate and rounded to a whole cent, halves up, and the results are added
    last_order   the latest `placed_on` among those orders, or None
    top_sku      see top_sku(), over those orders

    Cancelled orders count for nothing. The list is ordered by total_cents, largest first, then by
    name, then by customer id.
    """
    report = []
    customers = conn.execute("SELECT id, name, region FROM customers").fetchall()
    for customer_id, name, region in customers:
        orders = conn.execute(
            "SELECT id, placed_on, currency FROM orders WHERE customer_id = ? AND status != 'cancelled'",
            (customer_id,),
        ).fetchall()
        total = 0
        last_order = None
        for order_id, placed_on, currency in orders:
            rates = load_rates(rates_path)
            total += _half_up(order_total(conn, order_id) * rates[currency], 1_000_000)
            if last_order is None or placed_on > last_order:
                last_order = placed_on
        report.append(
            {
                "id": customer_id,
                "customer": name,
                "region": region,
                "orders": len(orders),
                "total_cents": total,
                "last_order": last_order,
                "top_sku": top_sku(conn, customer_id),
            }
        )
    report.sort(key=lambda row: (-row["total_cents"], row["customer"], row["id"]))
    for row in report:
        del row["id"]
    return report
