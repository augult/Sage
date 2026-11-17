#!/bin/bash
# GLM-5.3 round: Ecosage and OpenCode, two accounts each (one per repeat), 3 runs at a time per account.
# On a Logfare request-budget hit: stop, archive, recompute what is left, wait for the key, go again.
T=/tmp/claude-0/-home-user-Sage/7cc046a5-3cd8-5b9a-989c-c3f9ed722f91/scratchpad/tally_glm.py
key=$(cat /opt/sagebench/.key); n=${1:-1}; log=/srv/glm/loop.log
declare -A V=([sb-eco1]=ecosage [sb-eco2]=ecosage [sb-oc1]=ext:opencode [sb-oc2]=ext:opencode)
probe() { curl -sS -m 120 https://logfare.ai/v1/chat/completions -H "Authorization: Bearer $key" -H 'Content-Type: application/json' \
  -d '{"model":"glm-5.3","messages":[{"role":"user","content":"hi"}],"max_tokens":200}' 2>&1; }
while :; do
  until probe | grep -q '"choices"'; do echo "$(date -u +%T) waiting" >>$log; sleep 120; done
  out=glm$n; any=0
  for u in "${!V[@]}"; do
    t=$(cat /srv/glm/part$n/$u.remaining); [ -n "$t" ] || continue; any=1
    /opt/sagebench/arm-glm.sh $u "${V[$u]}" $out --tasks "$t" -j 3 &
  done
  [ $any = 1 ] || { echo "$(date -u +%T) all done" >>$log; exit 0; }
  echo "$(date -u +%T) launched part$n" >>$log; sleep 20
  while pgrep -f '/opt/sagebench/sage bench' >/dev/null; do
    if grep -lq 'request budget' /home/sb-*/$out.txt 2>/dev/null; then
      echo "$(date -u +%T) budget hit" >>$log; pkill -f '/opt/sagebench/sage bench'; sleep 5
      pkill -9 -u sb-eco1,sb-eco2,sb-oc1,sb-oc2; break
    fi
    sleep 30
  done
  wait
  for u in "${!V[@]}"; do mkdir -p /srv/glm/part$n/$u; [ -d /home/$u/sage ] && mv /home/$u/sage /srv/glm/part$n/$u/; mv /home/$u/$out.* /srv/glm/part$n/$u/ 2>/dev/null; rm -rf /srv/btmp/$u/* /srv/btmp/$u/.[!.]* 2>/dev/null; done
  n=$((n+1)); mkdir -p /srv/glm/part$n; chmod 700 /srv/glm/part$n
  python3 -I $T --remaining /srv/glm/part$n | head -4 >>$log
done
