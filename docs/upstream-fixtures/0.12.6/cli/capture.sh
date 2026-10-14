#!/bin/sh
cd /wk/out
W=/wk/wkhtmltopdf-linux-amd64
cap() { name=$1; shift; "$W" "$@" >$name.stdout 2>$name.stderr; echo $? > $name.rc; }
cap help --help
cap help_short -h
cap exthelp --extended-help
cap version --version
cap version_V -V
cap readme --readme
cap manpage --manpage
cap htmldoc --htmldoc
cap license --license
cap tocxsl --dump-default-toc-xsl
cap noargs
cap onearg foo.html
cap unknown_long --foo a b
cap unknown_short -z a b
cap bad_value --orientation sideways a b
cap notenough --title
cap wrongloc a.html --margin-top 1cm b.pdf
cap tocopt_in_global --toc-header-text X a b
cap cover_missing cover b.pdf
cap page_missing page b.pdf
cap dashdash -- a b
cap help_after_dpi --dpi 300 --extended-help
cap V_combined -qV
