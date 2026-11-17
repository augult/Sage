#!/bin/bash
# (Re)build the bench isolation after a machine restart: CA, filtering proxy chained to the session proxy, firewall.
set -euo pipefail
up=${HTTPS_PROXY##*:}; up=${up%/}
S=/tmp/claude-0/-home-user-Sage/7cc046a5-3cd8-5b9a-989c-c3f9ed722f91/scratchpad
cp /root/.ccr/ca-bundle.crt /opt/sagebench/ca.crt; chmod 644 /opt/sagebench/ca.crt
chmod 711 /opt/sagebench/sage
pkill -f allowproxy.py || true; sleep 1
setsid nohup python3 -I /opt/sagebench/allowproxy.py 18443 "$up" >>$S/run/allowproxy.log 2>&1 </dev/null &
iptables -F OUTPUT
for u in sb-pro1 sb-pro2 sb-eco1 sb-eco2 sb-oc1 sb-oc2 sb-uh1 sb-uh2 sb-build; do
  iptables -A OUTPUT -m owner --uid-owner $u ! -o lo -j REJECT
  for p in 44683 37931 42121 "$up"; do iptables -A OUTPUT -m owner --uid-owner $u -o lo -p tcp --dport $p -j REJECT; done
done
sleep 1
runuser -u sb-pro1 -- sh -c "curl -sS -m 10 --noproxy '*' https://example.com -o /dev/null 2>&1 | head -c 80; echo; curl -sS -m 10 -x http://127.0.0.1:$up https://logfare.ai -o /dev/null 2>&1 | head -c 80; echo; curl -sS -m 10 -x http://127.0.0.1:18443 https://github.com -o /dev/null 2>&1 | head -c 80; echo; curl -sS -m 20 -x http://127.0.0.1:18443 --cacert /opt/sagebench/ca.crt https://logfare.ai/v1/models -o /dev/null -w 'logfare %{http_code}' 2>&1; echo; strings /opt/sagebench/sage 2>&1 | head -1"
