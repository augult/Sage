THOUGHT = "The implementation nets correctly; the prior assertion expected the debt on the wrong account. Correct the assertion and rerun the smoke test."
CONTENT = ""
CALLS = [
    {"name": "bash", "arguments": {"command": "python3 - <<'PY'\nfrom rest_api import RestAPI\na = RestAPI()\nassert a.post('/add', {'user': 'Bob'})['balance'] == 0\na.post('/add', {'user': 'Adam'})\nr = a.post('/iou', {'lender': 'Adam', 'borrower': 'Bob', 'amount': 5.25})\nassert r['users'][0]['name'] == 'Adam' and r['users'][0]['owed_by']['Bob'] == 5.25\nassert a.get('/users')['users'][0]['name'] == 'Adam'\na.post('/iou', {'lender': 'Bob', 'borrower': 'Adam', 'amount': 2})\nu = a.get('/users')['users']\nassert u[0]['owed_by'] == {'Bob': 3.25} and u[1]['owes'] == {'Adam': 3.25}\nprint('smoke tests passed')\nPY", "timeout": 20}},
]
