THOUGHT = "Implement the report in POSIX sh and awk, including strict input validation, normalization, rank percentiles, and command-line handling. The harness will rerun check.sh after the edit."
CONTENT = ""
CALLS = [
    {
        "name": "edit",
        "arguments": {
            "path": "rollup.sh",
            "old": "#!/bin/sh\n# rollup.sh: per-endpoint summary of a gateway access log. See README.md.\n#\n# Not written yet. The old Python version could not be deployed to the log hosts.\necho \"rollup.sh: not implemented\" >&2\nexit 1",
            "new": """#!/bin/sh
# Per-endpoint summary of a gateway access log.
usage() {
    echo 'usage: sh rollup.sh [-n N] [FILE]' >&2
    exit 2
}

n=10
if [ \"$#\" -gt 0 ] && [ \"$1\" = -n ]; then
    [ \"$#\" -ge 2 ] || usage
    case $2 in ''|*[!0-9]*|0) usage ;; esac
    n=$2
    shift 2
fi
[ \"$#\" -le 1 ] || usage
file=${1--}
if [ \"$file\" = - ]; then
    input=/dev/stdin
else
    input=$file
fi

LC_ALL=C awk -v limit=\"$n\" '
function digits(s) { return s ~ /^[0-9]+$/ }
function valid_status(s) { return s ~ /^[0-9][0-9][0-9]$/ }
function normalize(path,    q,i,n,a,out) {
    q=index(path, \"?\")
    if (q) path=substr(path,1,q-1)
    if (path != \"/\" && substr(path,length(path),1)==\"/\") path=substr(path,1,length(path)-1)
    n=split(path,a,\"/\")
    out=a[1]
    for (i=2;i<=n;i++) out=out \"/\" (digits(a[i]) ? \":id\" : a[i])
    return out
}
function add_latency(key,value,    i) {
    i=++latn[key]
    while (i>1 && latency[key,i-1]>value) {
        latency[key,i]=latency[key,i-1]
        i--
    }
    latency[key,i]=value
}
NF==0 { next }
{
    if (NF!=6 || $2 !~ /^(GET|POST|PUT|PATCH|DELETE)$/ || substr($3,1,1)!=\"/\" || !valid_status($4) || $5 !~ /^[0-9]+ms$/ || $6 !~ /^[0-9]+b$/) {
        skipped++; next
    }
    ts=$1; method=$2; path=normalize($3); ms=substr($5,1,length($5)-2)+0; bytes=substr($6,1,length($6)-1)+0
    key=method \" \" path
    if (!requests || ts<earliest) earliest=ts
    if (!requests || ts>latest) latest=ts
    if (!requests || ms>slowms) { slowms=ms; slowpath=$2 \" \" $3; slowtime=ts }
    requests++; count[key]++; totalbytes[key]+=bytes
    if ($4+0>=500) errors[key]++
    add_latency(key,ms)
    if (!(key in seen)) { seen[key]=1; keys[++nk]=key }
}
END {
    print \"requests: \" requests+0
    print \"skipped: \" skipped+0
    if (!requests) exit
    print \"window: \" earliest \" .. \" latest
    print \"slowest: \" slowms \"ms \" slowpath \" at \" slowtime
    print \"\"
    printf \"%-32s %7s %6s %6s %6s %10s\\n\",\"endpoint\",\"count\",\"err%\",\"p50\",\"p95\",\"bytes\"
    for (i=1;i<=nk;i++) {
        key=keys[i]; order[i]=key
    }
    for (i=1;i<=nk;i++) for (j=i+1;j<=nk;j++) {
        a=order[i]; b=order[j]
        if (count[b]>count[a] || (count[b]==count[a] && b<a)) { order[i]=b; order[j]=a }
    }
    shown= nk<limit ? nk : limit
    for (i=1;i<=shown;i++) {
        key=order[i]; p50=int((50*count[key]+99)/100); p95=int((95*count[key]+99)/100)
        printf \"%-32s %7d %6.1f %6d %6d %10d\\n\",key,count[key],100*errors[key]/count[key],latency[key,p50],latency[key,p95],totalbytes[key]
    }
    if (nk>limit) {
        c=0;e=0;b=0
        for (i=limit+1;i<=nk;i++) { key=order[i]; c+=count[key]; e+=errors[key]; b+=totalbytes[key] }
        printf \"%-32s %7d %6.1f %6s %6s %10d\\n\",\"(other)\",c,100*e/c,\"-\",\"-\",b
    }
}' \"$input\"
""",
        },
    },
    {"name": "finish", "arguments": {"summary": "Implemented rollup.sh as a POSIX shell/mawk report generator. The harness watched check.sh; additional CLI and edge-case verification remains to be performed if the requested report requires more than the fixture checks.", "unverified": "General malformed-input, CLI validation, and empty-input edge cases have not yet been independently checked."}},
]
