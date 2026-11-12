"""Everything that is reachable must still work after the clean-up."""

import importlib
import io
import json
import re
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

import stockr
from stockr import Inventory, Item

ROOT = Path(__file__).resolve().parent.parent
SAMPLE = ROOT / "sample.csv"


class LiveTest(unittest.TestCase):
    def test_public_names(self):
        self.assertEqual(sorted(stockr.__all__), ["Inventory", "Item", "build_report", "median", "percentile"])
        for name in stockr.__all__:
            self.assertTrue(hasattr(stockr, name), name)
        self.assertEqual(stockr.percentile([5, 1, 3, 2, 4], 40), 2.0)
        self.assertEqual(stockr.percentile([5, 1, 3, 2, 4], 100), 5.0)

    def test_every_format(self):
        inventory = Inventory.load(SAMPLE)
        text = stockr.build_report(inventory, "text")
        self.assertIn("items: 5\nunits: 89\nvalue: 590.00\n", text)
        self.assertIn("! D-404: quantity is -3 [negative-stock]", text)
        self.assertEqual(
            stockr.build_report(inventory, "csv"),
            "items,units,value,mean_qty,median_qty,problems\n5,89,590.00,17.2,4.0,3\n",
        )
        data = json.loads(stockr.build_report(inventory, "json"))
        self.assertEqual(data["units"], 89)
        self.assertEqual(len(data["problems"]), 3)

    def test_the_checks_are_registered(self):
        from stockr.registry import check_names, run_checks

        importlib.import_module("stockr.report")
        self.assertEqual(check_names(), ["low-stock", "missing-price", "negative-stock"])
        problems = run_checks(Inventory.load(SAMPLE))
        self.assertEqual(
            problems,
            [
                ("low-stock", "B-220", "0 left, reorder at 1"),
                ("missing-price", "C-310", "no price"),
                ("negative-stock", "D-404", "quantity is -3"),
            ],
        )

    def test_the_command_reports_problems_in_a_fresh_process(self):
        result = subprocess.run(
            [sys.executable, "-m", "stockr", "check", str(SAMPLE)], cwd=ROOT, capture_output=True, text=True
        )
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(len(result.stdout.splitlines()), 3)
        result = subprocess.run(
            [sys.executable, "-m", "stockr", "report", str(SAMPLE), "--format", "json"],
            cwd=ROOT,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(json.loads(result.stdout)["problems"]), 3)

    def test_tsv_files(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "stock.tsv"
            path.write_text("sku\tname\tqty\tprice\treorder_at\nz-9\tZip ties\t7\t0.5\t2\n", encoding="utf-8")
            inventory = Inventory.load(path)
        self.assertEqual([(item.sku, item.qty, item.price) for item in inventory], [("Z-9", 7, 0.5)])

    def test_the_entry_points_in_pyproject_resolve(self):
        text = (ROOT / "pyproject.toml").read_text(encoding="utf-8")
        targets = re.findall(r'^[\w-]+ = "([\w.]+):(\w+)"$', text, re.MULTILINE)
        self.assertEqual(len(targets), 2)
        for module, function in targets:
            self.assertTrue(callable(getattr(importlib.import_module(module), function)), f"{module}:{function}")

    def test_prune_entry_point(self):
        from stockr.maintenance import prune_main

        with tempfile.TemporaryDirectory() as folder:
            for day in range(1, 6):
                (Path(folder) / f"report-2024-01-0{day}.txt").write_text("x")
            out = io.StringIO()
            with redirect_stdout(out):
                self.assertEqual(prune_main([folder, "--keep", "2"]), 0)
            self.assertEqual(sorted(p.name for p in Path(folder).iterdir()), ["report-2024-01-04.txt", "report-2024-01-05.txt"])
            self.assertEqual(len(out.getvalue().splitlines()), 3)

    def test_the_nightly_script(self):
        with tempfile.TemporaryDirectory() as folder:
            for day in range(10, 20):
                (Path(folder) / f"report-2001-01-{day}.txt").write_text("x")
            result = subprocess.run(
                [sys.executable, str(ROOT / "scripts" / "nightly.py"), str(SAMPLE), folder],
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            names = sorted(p.name for p in Path(folder).iterdir())
            self.assertEqual(len(names), 7)
            self.assertIn("items: 5", (Path(folder) / names[-1]).read_text())

    def test_the_plugins_in_plugins_json_load_and_run(self):
        from stockr.loader import load_exporters

        exporters = load_exporters(ROOT / "plugins.json")
        self.assertEqual(sorted(exporters), ["currency"])
        self.assertEqual(exporters["currency"]({"value": 10.0, "items": 2}, 1.5), {"value": 15.0, "items": 2})
        result = subprocess.run(
            [sys.executable, "-m", "stockr", "report", str(SAMPLE), "--export", "currency", "--rate", "2"],
            cwd=ROOT,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["value"], 1180.0)


if __name__ == "__main__":
    unittest.main()
