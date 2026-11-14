#!/bin/sh
# Reproduce every experiment of the paper (under three hours on 4 cores).
set -e
cd "$(dirname "$0")"
PY=${PYTHON:-python3}
$PY prep_data.py
$PY run_experiments.py main --trials 300
$PY run_experiments.py edits --trials 100
$PY run_experiments.py posthoc --trials 200
$PY run_experiments.py nsweep --trials 60
$PY run_experiments.py synth --trials 60
$PY run_experiments.py ablation --trials 200
# Language-model workflows: their tables are in data/, no API key needed.
$PY run_experiments.py llmstats
$PY run_experiments.py main --llm --trials 300
$PY run_experiments.py edits --llm --trials 100
# Knowledge-graph agents: their runs are in data/, no API key needed.
$PY run_experiments.py main --agent --trials 300
$PY run_experiments.py posthoc --agent --trials 200
$PY run_experiments.py edits --agent --trials 100
$PY make_figures.py
