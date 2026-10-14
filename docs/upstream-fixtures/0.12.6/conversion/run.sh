#!/bin/sh
cd /wk/conv
W=/wk/wkhtmltopdf-linux-amd64
cap() { name=$1; shift; "$W" "$@" >$name.stdout 2>$name.stderr; echo $? > $name.rc; }
cap basic a.html basic.pdf
cap quiet -q a.html quiet.pdf
cap warn --log-level warn b.html warn.pdf
cap debug --log-level debug --debug-javascript b.html debug.pdf
cap info_b --debug-javascript --enable-local-file-access b.html info_b.pdf
cap missing_page nonexist.html out.pdf
cap missing_page_ignore --load-error-handling ignore nonexist.html out2.pdf
cap missing_page_skip a.html --load-error-handling skip nonexist.html out3.pdf
cap toc --dump-outline outline.xml toc a.html b.html toc.pdf
cap stdout a.html -
cat a.html | "$W" -q - - > stdin.pdf 2>stdin.stderr; echo $? > stdin.rc
cap hdr --header-left "[page]/[topage] [webpage] [section] [title] [doctitle] [isodate] [date] [time] [sitepage] [sitepages] [frompage] [subsection] [foo]" --header-html hdr.html a.html hdr.pdf
cap badhost http://nonexistent.invalid/ net.pdf
cap unwritable a.html /nonexistent/dir/x.pdf
cap twoout a.html b.html
cap marginmix -T 1cm -B 10mm a.html mm.pdf
cap loglevel_bad --log-level verbose a.html x.pdf
printf 'a.html rs1.pdf\n-q b.html rs2.pdf\n' | "$W" --read-args-from-stdin > rs.stdout 2> rs.stderr; echo $? > rs.rc
"$W" --dump-default-toc-xsl --toc-header-text Foo > tocxsl2.stdout 2>&1
"$W" toc --toc-header-text Foo --dump-default-toc-xsl > tocxsl3.stdout 2>&1; echo $? >> tocxsl3.stdout
