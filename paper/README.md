# Risk Certificates That Survive Edits

Manuscript and experiments for the risk certification method implemented in
`anp certify` (see `crates/anp-core/src/graph/risk.rs`).

- `main.tex`, `experiments.tex`, `appendix.tex`, `refs.bib` — the paper; `main.pdf` is the built copy.
- `numbers.tex`, `tables/`, `figures/` — generated from `results/*.json`; do not edit by hand.
- `build.sh` — builds `main.pdf` and the arXiv source package `arxiv.tar.gz`.
- `results/` — raw results of every experiment.
- `experiments/data/` — the answers of the hosted language models as tables (and the CLINC150
  structure they refer to) and the runs of the knowledge-graph agents (`kg_*.npz`), so these
  experiments run without calling a model.
- `experiments/` — code: `riskcert.py` (the method and baselines), `prep_data.py` (datasets and
  node models), `llm_nodes.py` (the same workflow answered by a hosted language model),
  `kg_agent.py` (a knowledge-graph agent on WebQSP and CWQ), `check_theorems.py` (the two
  guarantees checked for every threshold assignment at once), `test_claims.py` (the lemma and
  the propositions checked exactly on random workflows),
  `run_experiments.py`, `make_figures.py`, `export_example.py` (writes `examples/certify/`).

## Reproduce

```bash
python3 -m venv venv && ./venv/bin/pip install numpy scipy scikit-learn pandas matplotlib \
    datasets huggingface_hub sentence-transformers httpx
PYTHON=./venv/bin/python experiments/run_all.sh     # under three hours on 4 CPU cores
./build.sh                                        # main.pdf and arxiv.tar.gz
```

The language-model tables in `experiments/data/` are enough to re-run those experiments.
Regenerating them needs an OpenAI API key (about 24,000 requests to `gpt-4.1-mini` per variant
and 830 to `gpt-5.4-mini`; answers are cached under `experiments/cache/` and the run resumes):

```bash
OPENAI_API_KEY=... ./venv/bin/python experiments/llm_nodes.py           # zero-shot
OPENAI_API_KEY=... ./venv/bin/python experiments/llm_nodes.py --shots   # with in-context examples
cd experiments && ../venv/bin/python run_experiments.py llmstats
../venv/bin/python run_experiments.py main --llm --trials 300
../venv/bin/python run_experiments.py edits --llm --trials 100 && ../venv/bin/python make_figures.py
```

The runs of the knowledge-graph agents are regenerated from the question subgraphs released
with RoG (`rmanluo/RoG-webqsp`, all splits, and `rmanluo/RoG-cwq`, validation and test, on the
Hugging Face Hub), saved as `webqsp_{train,val,test}*.parquet` and `cwq_{val,test}*.parquet`
in one directory (about 53,000 requests to `gpt-4.1-mini` and 8,700 to `gpt-5.4-mini`; the
answers are cached by prompt, so an interrupted run resumes and an edited agent only pays for
new prompts):

```bash
OPENAI_API_KEY=... ./venv/bin/python experiments/kg_agent.py webqsp --source DIR \
    --edit answer=answer:gpt-5.4-mini --edit late=rel3:gpt-5.4-mini
OPENAI_API_KEY=... ./venv/bin/python experiments/kg_agent.py cwq --source DIR \
    --edit answer=answer:gpt-5.4-mini --edit late=rel4:gpt-5.4-mini
cd experiments && ../venv/bin/python run_experiments.py main --agent --trials 300
../venv/bin/python run_experiments.py posthoc --agent --trials 200
../venv/bin/python run_experiments.py edits --agent --trials 100 && ../venv/bin/python make_figures.py
```

The datasets (CLINC150, MASSIVE) and the sentence-embedding models are downloaded from the
Hugging Face Hub on first use and cached under `experiments/cache/` (not tracked).
