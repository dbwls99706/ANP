# Risk Certificates That Survive Edits

Manuscript and experiments for the risk certification method implemented in
`anp certify` (see `crates/anp-core/src/graph/risk.rs`).

- `main.tex`, `experiments.tex`, `appendix.tex`, `refs.bib` — the paper; `main.pdf` is the built copy.
- `numbers.tex`, `tables/`, `figures/` — generated from `results/*.json`; do not edit by hand.
- `results/` — raw results of every experiment.
- `experiments/` — code: `riskcert.py` (the method and baselines), `prep_data.py` (datasets and
  node models), `llm_nodes.py` (the same workflow answered by a hosted language model),
  `run_experiments.py`, `make_figures.py`, `export_example.py` (writes `examples/certify/`).

## Reproduce

```bash
python3 -m venv venv && ./venv/bin/pip install numpy scipy scikit-learn pandas matplotlib \
    datasets huggingface_hub sentence-transformers httpx
PYTHON=./venv/bin/python experiments/run_all.sh     # under two hours on 4 CPU cores
pdflatex main && bibtex main && pdflatex main && pdflatex main
```

The language-model experiment is optional and needs an OpenAI API key (about 24,000 short
requests to `gpt-4.1-mini` and 830 to `gpt-5.4-mini`; answers are cached and the run resumes):

```bash
OPENAI_API_KEY=... ./venv/bin/python experiments/llm_nodes.py
cd experiments && ../venv/bin/python run_experiments.py llmstats
../venv/bin/python run_experiments.py main --llm --trials 300
../venv/bin/python run_experiments.py edits --llm --trials 100 && ../venv/bin/python make_figures.py
```

The datasets (CLINC150, MASSIVE) and the sentence-embedding models are downloaded from the
Hugging Face Hub on first use and cached under `experiments/cache/` (not tracked).
