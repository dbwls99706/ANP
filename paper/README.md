# Risk Certificates That Survive Edits

Manuscript and experiments for the risk certification method implemented in
`anp certify` (see `crates/anp-core/src/graph/risk.rs`).

- `main.tex`, `experiments.tex`, `appendix.tex`, `refs.bib` — the paper; `main.pdf` is the built copy.
- `numbers.tex`, `tables/`, `figures/` — generated from `results/*.json`; do not edit by hand.
- `results/` — raw results of every experiment.
- `experiments/` — code: `riskcert.py` (the method and baselines), `prep_data.py` (datasets and
  node models), `run_experiments.py`, `make_figures.py`, `export_example.py` (writes `examples/certify/`).

## Reproduce

```bash
python3 -m venv venv && ./venv/bin/pip install numpy scipy scikit-learn pandas matplotlib \
    datasets huggingface_hub sentence-transformers
PYTHON=./venv/bin/python experiments/run_all.sh     # under two hours on 4 CPU cores
pdflatex main && bibtex main && pdflatex main && pdflatex main
```

The datasets (CLINC150, MASSIVE) and the sentence-embedding models are downloaded from the
Hugging Face Hub on first use and cached under `experiments/cache/` (not tracked).
