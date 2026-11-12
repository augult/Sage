---
name: security
title: Security audit
description: Find real, exploitable vulnerabilities, and say so plainly when there are none.
---
Audit the code for security vulnerabilities. Report only what you can demonstrate from the code. "No vulnerabilities found" is a valid and valuable result: never invent, pad or inflate findings to have something to show.

## Method
1. Map the attack surface. List the entry points (HTTP handlers, CLI arguments, files, environment, network input, queue consumers, deserialisers), which data an attacker controls, where authentication and authorisation happen, and the sinks: SQL, shell, file paths, templates and HTML, eval and deserialisation, outbound requests, crypto, redirects, logs.
2. Trace both ways. From each entry point follow attacker-controlled data to every sink it can reach, through every function in between. From each sink work back to who can reach it. Read the body of every helper and validator on the way: a name like `sanitize` or `safe_join` proves nothing.
3. Go through the classes one by one, do not stop at the first hits:
   - injection: SQL, command, template, header, log
   - path traversal, unsafe file writes and archive extraction
   - authentication: password storage, token generation, session handling, comparison of secrets
   - authorisation: compare each handler with its siblings; one that skips the ownership or role check the others make is a finding
   - unsafe deserialisation and dynamic execution: pickle, `yaml.load`, `eval`, `exec`
   - SSRF and open redirects; XSS and CSRF where HTML is produced
   - secrets in code or logs; crypto misuse (fast hashes for passwords, static salt or IV, `random` for tokens, non-constant-time comparison)
   - races and time-of-check/time-of-use; unbounded input; unsafe defaults such as debug mode
4. Verify every candidate before it goes in the report. A finding needs all three:
   - source: a concrete input the attacker controls
   - path: the lines it travels, each of which you have read, with no effective validation on the way
   - impact: what the attacker gains
   Then try to refute it. Is the value a constant, whitelisted, or cast to a number? Is the query parameterised? Is the path resolved and checked against its base? Does a caller validate earlier? Is the code reachable at all? Where the workspace allows, prove it with a harmless proof of concept (a small test or one-liner that calls the function). A candidate without an established source and path is not a finding.

## Looks dangerous, usually is not: check, then leave it out
- SQL assembled from constants or from a whitelist of identifiers, with the values still bound as parameters
- `subprocess` with an argument list and no shell
- a hash used for a cache key, an ETag or deduplication; `random` used for jitter or sampling
- a path that is joined, resolved, and then checked against its base directory
- example credentials in tests, fixtures and documentation

## Report
- Per finding: file and line, the class (with CWE number if you know it), severity by impact and reachability, the source-to-sink path in a sentence or two, a concrete example of exploitation, and the smallest fix. Most severe first. One finding per root cause, not one per call site.
- Then say what you covered and what you could not (files not read, configuration unknown at rest).
- If nothing survived verification, say "No vulnerabilities found" and give the coverage. Do not turn speculation into "low severity" findings. Hardening ideas, if any, go in a separate, clearly labelled list and never into a findings file.
- Follow any output format the task specifies exactly.
- Change no code unless asked to fix. When asked, make the smallest fix and add a test that fails without it.
