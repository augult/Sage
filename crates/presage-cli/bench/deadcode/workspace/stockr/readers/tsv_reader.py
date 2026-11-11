"""Reads tab-separated inventory files, with the same columns as the CSV ones."""

from . import csv_reader


def read(path):
    return csv_reader.read(path, delimiter="\t")
