# stockr

Stock reports for a small workshop.

    python3 -m stockr report sample.csv --format text
    python3 -m stockr check sample.csv
    python3 scripts/nightly.py sample.csv reports/

Inventory files are `.csv` or `.tsv`. Reports come as `text`, `csv` or `json`.
Exporter plugins are listed in `plugins.json`.

Tests: `python3 -m unittest discover -s tests -t .`
