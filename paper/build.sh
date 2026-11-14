#!/bin/sh
# Build main.pdf and an arXiv source package (arxiv.tar.gz) from the files in this folder.
set -e
cd "$(dirname "$0")"
pdflatex -interaction=nonstopmode main.tex > /dev/null
bibtex main > /dev/null
pdflatex -interaction=nonstopmode main.tex > /dev/null
pdflatex -interaction=nonstopmode main.tex > build.log
grep -E "^!|undefined" build.log && { echo "LaTeX reported problems, see build.log"; exit 1; }
# arXiv compiles the sources itself: ship the .bbl instead of the .bib and no generated PDF.
tar -czf arxiv.tar.gz main.tex experiments.tex appendix.tex numbers.tex main.bbl tables/*.tex figures/*.pdf
echo "built main.pdf ($(grep -o 'Output written on main.pdf ([0-9]* pages' build.log | grep -o '[0-9]* pages')) and arxiv.tar.gz"
