#!/bin/sh
# Runs every test page with its expectations, then times each page (median of 5 runs, release build).
set -u
cd "$(dirname "$0")"
B=./target/release/jsdom-spike
mkdir -p out
fail=0
check() { name=$1; shift; if "$B" "$@" >/dev/null 2>out/$name.log; then echo "PASS $name"; else echo "FAIL $name (see out/$name.log)"; fail=1; fi; }
check 1-footer-subst pages/1-footer-subst.html --query 'page=2&topage=5&section=Intro' --out out/1.html \
  --expect '<td class="section">Intro</td>' --expect '<span class="page">2</span>' --expect '<span class="topage">5</span>'
check 2-jquery pages/2-jquery.html --window-status jq-done --out out/2.html \
  --expect '<div id="x" class="done" data-n="3">ok</div>' --expect '<tr class="r" style="background-color: rgb(238, 238, 238);"><td>1</td>' \
  --expect '<p id="made">built by jQuery</p>' --expect '<li>a #0</li><li>b #1</li>'
check 3-table-500 pages/3-table-500.html --out out/3.html \
  --expect 'Total: 1667907.50 (500 rows)' --expect '<tr class="odd"><td>500</td><td>Customer 499</td><td style="text-align: right;">6671.63</td></tr>'
check 4-timers pages/4-timers.html --window-status ready --out out/4.html \
  --expect '<span>step5@150 </span></div>' --expect 'data-ticks="3"' --expect 'data-dcl="1"' --expect 'data-load="complete"' \
  --expect-not '</span>NEVER' --expect-not '</span>LATE'
check 5-chart4-recorder pages/5-chart.html --canvas-recorder --window-status chart-ok --out out/5.html
check 5b-chart2-recorder pages/5b-chart2.html --canvas-recorder --window-status chart-ok --out out/5b.html
# document.write is emulated (fragment inserted after the running script). Known divergence: a head
# script sees body elements (blitz-html parses the whole input first), so data-early is not "null".
check 8-docwrite pages/8-docwrite.html --out out/8.html \
  --expect '</script><p id="w1">written 1</p><p id="w2">written 2</p><script>' --expect 'data-inner="ran"'
check 6-slow pages/6-slow.html --loop-limit 1000000 --out out/6.html --expect 'next script ran'
echo
echo "median total ms over 5 runs:"
for spec in "1-footer-subst --query page=2&topage=5&section=Intro" "2-jquery" "3-table-500" "4-timers --window-status ready" \
            "5-chart --canvas-recorder" "5b-chart2 --canvas-recorder"; do
  set -- $spec; p=$1; shift
  for i in 1 2 3 4 5; do "$B" pages/$p.html "$@" --out /dev/null 2>&1 | sed -n 's/.*scripts+events=\([0-9.]*\).*total=\([0-9.]*\).*/\2 \1/p'; done \
    | sort -n | sed -n 3p | awk -v p="$p" '{printf "  %-16s total=%s ms  scripts+events=%s ms\n", p, $1, $2}'
done
exit $fail
