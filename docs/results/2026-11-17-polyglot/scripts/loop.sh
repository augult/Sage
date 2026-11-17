#!/bin/bash
# Run the remaining tasks; on a Logfare request-budget hit, stop, archive, recompute what's left, wait, and go again.
T=/tmp/claude-0/-home-user-Sage/7cc046a5-3cd8-5b9a-989c-c3f9ed722f91/scratchpad/tally.py
key=$(cat /opt/sagebench/.key); n=5; log=/srv/loop.log
declare -A V=([sb-pro1]=prosage [sb-pro2]=prosage [sb-eco1]=ecosage [sb-eco2]=ecosage [sb-oc1]=ext:opencode [sb-oc2]=ext:opencode [sb-uh1]=ext:unharnessed [sb-uh2]=ext:unharnessed)
probe() { curl -sS -m 60 https://logfare.ai/v1/chat/completions -H "Authorization: Bearer $key" -H 'Content-Type: application/json' \
  -d '{"model":"deepseek-v4.1-flash-1017","messages":[{"role":"user","content":"hi"}],"max_tokens":5}' 2>&1; }
while :; do
  until probe | grep -q '"choices"'; do echo "$(date -u +%T) waiting" >>$log; sleep 120; done
  out=main$n; any=0
  for u in "${!V[@]}"; do
    t=$(cat /srv/part$n/$u.remaining); [ -n "$t" ] || continue; any=1
    j=1; [[ $u == sb-uh* ]] && j=2
    /opt/sagebench/arm.sh $u "${V[$u]}" $out --tasks "$t" -j $j &
  done
  [ $any = 1 ] || { echo "$(date -u +%T) all done" >>$log; exit 0; }
  echo "$(date -u +%T) launched part$n" >>$log; sleep 20
  while pgrep -f '/opt/sagebench/sage bench' >/dev/null; do
    if grep -lq 'request budget' /home/sb-*/$out.txt 2>/dev/null; then
      echo "$(date -u +%T) budget hit" >>$log; pkill -f '/opt/sagebench/sage bench'; sleep 5
      pkill -9 -u sb-pro1,sb-pro2,sb-eco1,sb-eco2,sb-oc1,sb-oc2,sb-uh1,sb-uh2; break
    fi
    sleep 30
  done
  wait
  for u in "${!V[@]}"; do mkdir -p /srv/part$n/$u; [ -d /home/$u/sage ] && mv /home/$u/sage /srv/part$n/$u/; mv /home/$u/$out.* /srv/part$n/$u/ 2>/dev/null; rm -rf /srv/btmp/$u/* /srv/btmp/$u/.[!.]* 2>/dev/null; done
  n=$((n+1)); mkdir -p /srv/part$n; chmod 700 /srv/part$n
  python3 -I $T --remaining /srv/part$n | head -8 >>$log
done
