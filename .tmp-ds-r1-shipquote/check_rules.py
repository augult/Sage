from shipping.errors import QuoteError
from shipping.quote import quote
from shipping.zones import lookup


def item(**over):
    base = {"sku": "X", "qty": 1, "price": 1000, "weight_g": 400, "dims_cm": [10, 10, 10]}
    base.update(over)
    return base


def raises(fn):
    try:
        fn()
    except QuoteError:
        return True
    return False


# Rule 1/2: normalisation and longest prefix.
assert lookup(" ab1 2cd ") == ("C", True)
assert lookup("68410") == ("C", True)
assert lookup("68140") == ("B", False)
assert lookup("29 123") == ("B", True)
assert lookup("10115") == ("A", False)
assert raises(lambda: lookup("ZZ9"))

# Rules 3/4: volumetric ceil per line, shipment max, 500 g rounding.
q = quote({"destination": "10115", "service": "standard", "items": [item(dims_cm=[1, 1, 6])]})
assert q["billable_g"] == 500  # 6/5 = 1.2 -> 2 g -> 500 g
q = quote({"destination": "10115", "service": "standard", "items": [item(qty=2, dims_cm=[5, 5, 5])]})
assert q["billable_g"] == 1000  # volumetric 50 g loses to 800 g actual -> 1000 g
q = quote({"destination": "30159", "service": "standard", "items": [item(qty=3, weight_g=700)]})
assert q["billable_g"] == 2500 and q["base"] == 695 + 4 * 165

# Rule 5/6: express rounding half up, unavailable in D.
q = quote({"destination": "10115", "service": "express", "items": [item(weight_g=900)]})
assert q["billable_g"] == 1000 and q["base"] == 908
assert raises(lambda: quote({"destination": "98123", "service": "express", "items": [item()]}))

# Rule 7: free shipping only for standard and non-remote.
q = quote({"destination": "10115", "service": "standard", "items": [item(price=10000)]})
assert q["base"] == 0
q = quote({"destination": "68410", "service": "standard", "items": [item(price=10000)]})
assert q["zone"] == "C" and q["base"] == 925
q = quote({"destination": "10115", "service": "express", "items": [item(price=20000)]})
assert q["base"] == (495 * 3 + 1) // 2

# Rule 8: coupons.
q = quote({"destination": "10115", "service": "standard", "items": [item()], "coupon": "SHIP50"})
assert q["base"] == 247  # 495 / 2 rounds down
q = quote({"destination": "10115", "service": "express", "items": [item()], "coupon": "FREESHIP"})
assert q["base"] == 0
q = quote({"destination": "68410", "service": "standard", "items": [item()], "coupon": "FREESHIP"})
assert q["base"] == 925  # FREESHIP does nothing for a remote destination
assert raises(lambda: quote({"destination": "10115", "service": "standard", "items": [item()], "coupon": "ship50"}))

# Rules 9-11: surcharges.
q = quote({"destination": "68410", "service": "standard", "items": [item()]})
assert q["surcharges"] == {"remote": 139} and q["total"] == 925 + 139
q = quote({"destination": "10115", "service": "standard", "items": [item(fragile=True)]})
assert q["surcharges"] == {"fragile": 250} and q["total"] == 745
q = quote({"destination": "10115", "service": "standard", "items": [item(qty=7, fragile=True)]})
assert q["surcharges"] == {"fragile": 1500}
q = quote({"destination": "10115", "service": "standard", "items": [item(dims_cm=[121, 10, 10])]})
assert q["surcharges"] == {"oversize": 1200}
q = quote({"destination": "10115", "service": "standard", "items": [item(weight_g=30001)]})
assert q["surcharges"] == {"oversize": 1200}
q = quote({"destination": "10115", "service": "standard", "items": [item(dims_cm=[120, 10, 10], weight_g=30000)]})
assert q["surcharges"] == {}

# Rule 12: validation.
assert raises(lambda: quote({"destination": "10115", "service": "standard", "items": []}))
assert raises(lambda: quote({"destination": "10115", "service": "standard", "items": [item(qty=0)]}))
assert raises(lambda: quote({"destination": "10115", "service": "standard", "items": [item(qty=1.5)]}))
assert raises(lambda: quote({"destination": "10115", "service": "standard", "items": [item(weight_g=-1)]}))
assert raises(lambda: quote({"destination": "10115", "service": "standard", "items": [item(dims_cm=[10, 0, 10])]}))
assert raises(lambda: quote({"destination": "10115", "service": "overnight", "items": [item()]}))

print("ALL RULE CHECKS PASSED")
