#!/bin/sh
# Reproduce every experiment of the paper (about two hours on 4 cores).
set -e
cd "$(dirname "$0")"
PY=${PYTHON:-python3}
$PY prep_data.py
$PY run_experiments.py main --trials 300
$PY run_experiments.py edits --trials 100
$PY run_experiments.py posthoc --trials 200
$PY run_experiments.py nsweep --trials 60
$PY run_experiments.py synth --trials 60
$PY make_figures.py
