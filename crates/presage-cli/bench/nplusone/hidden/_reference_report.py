"""The customer report: what every customer has bought, in euro cents."""

import json


def load_rates(path):
    """currency -> millionths of a euro per unit, e.g. {"USD": 921500}."""
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def _half_up(numerator, denominator):
    return (2 * numerator + denominator) // (2 * denominator)


def customer_report(conn, rates_path="rates.json"):
    """One dict per customer, including those who never ordered:

    customer     the name
    region       the region
    orders       how many orders, not counting cancelled ones
    total_cents  what those orders come to in euro cents: each order's total is converted with its
                 currency's rate and rounded to a whole cent, halves up, and the results are added
    last_order   the latest `placed_on` among those orders, or None
    top_sku      the SKU the customer bought most units of in those orders; among equals the
                 alphabetically first; None without any

    Cancelled orders count for nothing. The list is ordered by total_cents, largest first, then by
    name, then by customer id.

    Three queries, whatever the number of customers, and the rates are read once.
    """
    rates = load_rates(rates_path)

    # Each live order with its total: every line rounded to a whole cent, halves up, in SQL's
    # integer arithmetic. An order without items still counts, with a total of 0.
    figures = {}
    orders = conn.execute(
        """
        SELECT o.customer_id, o.placed_on, o.currency,
               COALESCE(SUM((2 * i.qty * i.unit_cents * (100 - COALESCE(i.discount_pct, 0)) + 100) / 200), 0)
        FROM orders o LEFT JOIN items i ON i.order_id = o.id
        WHERE o.status != 'cancelled'
        GROUP BY o.id
        """
    )
    for customer_id, placed_on, currency, total in orders:
        row = figures.setdefault(customer_id, [0, 0, None])
        row[0] += 1
        row[1] += _half_up(total * rates[currency], 1_000_000)
        if row[2] is None or placed_on > row[2]:
            row[2] = placed_on

    # Units per customer and SKU; the best SKU is the one with most units, then the first by name.
    best = {}
    units = conn.execute(
        """
        SELECT o.customer_id, i.sku, SUM(i.qty)
        FROM items i JOIN orders o ON o.id = i.order_id
        WHERE o.status != 'cancelled'
        GROUP BY o.customer_id, i.sku
        """
    )
    for customer_id, sku, qty in units:
        current = best.get(customer_id)
        if current is None or (-qty, sku) < current:
            best[customer_id] = (-qty, sku)

    report = []
    for customer_id, name, region in conn.execute("SELECT id, name, region FROM customers"):
        count, total, last_order = figures.get(customer_id, (0, 0, None))
        report.append(
            {
                "id": customer_id,
                "customer": name,
                "region": region,
                "orders": count,
                "total_cents": total,
                "last_order": last_order,
                "top_sku": best[customer_id][1] if customer_id in best else None,
            }
        )
    report.sort(key=lambda row: (-row["total_cents"], row["customer"], row["id"]))
    for row in report:
        del row["id"]
    return report
