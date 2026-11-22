THOUGHT = "B's new IOU offsets part of A's original IOU, so A is owed the net three."
CONTENT = ""
CALLS = [{"name":"bash","arguments":{"command":"python3 - <<'PY'\nfrom rest_api import RestAPI\na=RestAPI(); a.post('/add', {'user':'A'}); a.post('/iou', {'lender':'A','borrower':'B','amount':5})\nusers=a.post('/iou', {'lender':'B','borrower':'A','amount':2})['users']\nassert users[0]['owed_by']=={'B':3} and users[1]['owes']=={'A':3}\nassert a.get('/users')['users'][0]['balance']==3\nprint('API smoke checks passed')\nPY","timeout":2000,"workdir":"."}}]
