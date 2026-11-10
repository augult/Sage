"""Shipping tariff."""
import math

ZONE_RATE = {"A": 1.00, "B": 1.40, "C": 1.90, "D": 2.60, "E": 3.50}
BASE_FEE = 4.00
HEAVY_FROM_KG = 20
HEAVY_SURCHARGE = 12.00
EXPRESS_FACTOR = 1.5


def shipping_cost(weight_kg, zone, express=False):
    """Cost of sending a parcel.

    The base fee plus the zone rate for every started kilogram. Parcels of 20 kg and more pay the
    heavy surcharge. Express multiplies the whole amount by 1.5. The result is rounded to cents.
    """
    kilos = math.ceil(weight_kg)
    cost = BASE_FEE + kilos * ZONE_RATE[zone]
    if weight_kg >= HEAVY_FROM_KG:
        cost += HEAVY_SURCHARGE
    if express:
        cost *= EXPRESS_FACTOR
    return round(cost, 2)
