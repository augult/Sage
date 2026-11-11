"""The stock checks."""

from .registry import check


@check("negative-stock")
def _check_negative(item):
    if item.qty < 0:
        return f"quantity is {item.qty}"


@check("low-stock")
def _check_low(item):
    if 0 <= item.qty <= item.reorder_at:
        return f"{item.qty} left, reorder at {item.reorder_at}"


@check("missing-price")
def _check_missing_price(item):
    if item.price is None:
        return "no price"


def _check_weight(item):
    if item.weight_kg is None or item.weight_kg <= 0:
        return "no shipping weight"


# @check("stale")
# def _check_stale(item):
#     if item.last_counted is None:
#         return "never counted"
#     if (date.today() - item.last_counted).days > 90:
#         return "not counted for 90 days"
