#!/bin/sh
# Build main.pdf and an arXiv source package (arxiv.tar.gz) from the files in this folder.
set -e
cd "$(dirname "$0")"
# Date everything by the last commit of the sources, so that an unchanged paper rebuilds
# to the same files.
SOURCES="main.tex experiments.tex appendix.tex numbers.tex refs.bib tables figures"
SOURCE_DATE_EPOCH=$(git log -1 --format=%ct -- $SOURCES 2>/dev/null || true)
: "${SOURCE_DATE_EPOCH:=$(date +%s)}"
export SOURCE_DATE_EPOCH FORCE_SOURCE_DATE=1
pdflatex -interaction=nonstopmode main.tex > /dev/null
bibtex main > /dev/null
pdflatex -interaction=nonstopmode main.tex > /dev/null
pdflatex -interaction=nonstopmode main.tex > build.log
grep -E "^!|undefined" build.log && { echo "LaTeX reported problems, see build.log"; exit 1; }
# arXiv compiles the sources itself: ship the .bbl instead of the .bib and no generated PDF.
tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner -cf - \
    main.tex experiments.tex appendix.tex numbers.tex main.bbl tables/*.tex figures/*.pdf | gzip -n > arxiv.tar.gz
echo "built main.pdf ($(grep -o 'Output written on main.pdf ([0-9]* pages' build.log | grep -o '[0-9]* pages')) and arxiv.tar.gz"
