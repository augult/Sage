#!/bin/bash
# Run one benchmark arm as an isolated account: own /tmp, own home, network only via the logfare filter.
# usage: arm.sh <user> <variant> <out-name> [extra sage bench args...]
set -euo pipefail
u=$1 variant=$2 out=$3
shift 3
key=$(cat /opt/sagebench/.key)
mkdir -p /srv/bench-out && chmod 1733 /srv/bench-out
exec unshare --mount --propagation private sh -c '
mount --bind /srv/btmp/'"$u"' /tmp
exec runuser -u '"$u"' -- env -i \
  HOME=/home/'"$u"' USER='"$u"' LANG=C.UTF-8 \
  PATH=/opt/rust/bin:/usr/local/go/bin:/opt/node22/bin:/usr/local/bin:/usr/bin:/bin \
  HTTPS_PROXY=http://127.0.0.1:18443 https_proxy=http://127.0.0.1:18443 \
  NO_PROXY=localhost,127.0.0.1 no_proxy=localhost,127.0.0.1 \
  SSL_CERT_FILE=/opt/sagebench/ca.crt NODE_EXTRA_CA_CERTS=/opt/sagebench/ca.crt \
  GOTOOLCHAIN=local GOPROXY=off CARGO_NET_OFFLINE=true \
  LOGFARE_API_KEY='"$key"' SAGE_HOME=/home/'"$u"'/sage BENCH_EXT_HOME=/opt/sage-external \
  sh -c "cd /home/'"$u"' && /opt/sagebench/sage bench -p logfare -m deepseek-v4.1-flash-1017 \
    --external-dir /opt/sagebench/external --variants '"$variant"' --tasks polyblind -n 1 \
    -e ratchet --max-turns 60 --time-budget 1200 --max-cost 10 '"$*"' \
    --out /home/'"$u"'/'"$out"'.json > /home/'"$u"'/'"$out"'.txt 2>&1"
'
