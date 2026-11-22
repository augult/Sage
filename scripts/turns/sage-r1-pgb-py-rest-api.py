THOUGHT = "Reciprocal netting is now corrected; run the direct Python verification and finish."
CONTENT = ""
CALLS = [
    {"name": "shell", "arguments": {"cmd": "python3 -c \"from rest_api import RestAPI; a=RestAPI(); [a.post('/add', {'user':n}) for n in ('Adam','Bob')]; a.post('/iou', {'lender':'Adam','borrower':'Bob','amount':5.25}); a.post('/iou', {'lender':'Bob','borrower':'Adam','amount':2}); u=a.get('/users')['users']; assert u == [{'name':'Adam','owes':{},'owed_by':{'Bob':3.25},'balance':3.25},{'name':'Bob','owes':{'Adam':3.25},'owed_by':{},'balance':-3.25}], u; print('reciprocal debt verified')\"", "expect_exit": "0", "expect_has": "reciprocal debt verified", "timeout": 30}},
    {"name": "finish", "arguments": {"summary": "Implemented the IOU REST API and fixed reciprocal IOU netting; a direct Python check verified final ledgers and balances.", "unverified": "No repository test cases are present."}},
]
