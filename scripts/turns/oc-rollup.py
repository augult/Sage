THOUGHT = "The exact expected reports and README agree that slowest includes the method; the current output omits it, so correct that and rerun the checks."
CONTENT = ""
CALLS = [
    {"name": "apply_patch", "arguments": {"patchText": "*** Begin Patch\n*** Update File: rollup.sh\n@@\n-    if(!have) { lo=hi=ts; slow=ms; slowts=ts; slowpath=path; have=1 }\n+    if(!have) { lo=hi=ts; slow=ms; slowts=ts; slowmethod=$2; slowpath=path; have=1 }\n@@\n-    if(ms>slow) {slow=ms; slowts=ts; slowpath=path}\n+    if(ms>slow) {slow=ms; slowts=ts; slowmethod=$2; slowpath=path}\n@@\n-    print \"slowest: \" slow \"ms \" slowpath \" at \" slowts\n+    print \"slowest: \" slow \"ms \" slowmethod \" \" slowpath \" at \" slowts\n*** End Patch"}},
    {"name": "bash", "arguments": {"command": "sh check.sh; sh rollup.sh -n 3 access.log", "timeout": 30, "workdir": "."}},
]
