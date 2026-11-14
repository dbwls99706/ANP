"""A knowledge-graph agent as a decision workflow (WebQSP and CWQ).

The agent answers a question by walking a knowledge graph. The language model only
decides: which entity to start from, which relation to follow, whether the entities
reached answer the question, and which of them to return. The program executes the
walk. Every decision is a closed choice, and its confidence is the probability of the
answer token, as in llm_nodes.py.

Unlike the routing workflows, an agent has no unique correct route. A run is an error
when the returned entity is not a gold answer, which is all the certificate with
least-confident charging needs. For first-error charging a decision is marked wrong when
it puts the gold answers out of reach:

    start, relation   no gold answer within the remaining number of relations
    stop              says "answered" without a gold answer among the entities shown,
                      or "keep walking" although one has been reached
    answer            the returned entity is not a gold answer

Data: the question subgraphs of Freebase released with RoG (rmanluo/RoG-webqsp and
rmanluo/RoG-cwq on the Hugging Face hub), as parquet files in --source.

Usage:
    python kg_agent.py webqsp --source DIR            # all splits, about 4,700 questions
    python kg_agent.py cwq --source DIR               # validation and test, about 7,000
    python kg_agent.py webqsp --source DIR --limit 200   # pilot, saves nothing

The answers of the model are cached in cache/kg_raw_<model>.jsonl by prompt, so a run
can be resumed and an edited workflow only pays for the prompts that changed. The
resulting traces are written to data/kg_<name>.npz and are what run_experiments.py reads.
"""
import argparse
import glob
import hashlib
import json
import math
import os
import re
import string
import threading
import time
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor

import httpx
import numpy as np

CACHE = os.environ.get("ANP_CACHE", os.path.join(os.path.dirname(__file__), "cache"))
DATA = os.path.join(os.path.dirname(__file__), "data")
URL = "https://api.openai.com/v1/chat/completions"
LETTERS = string.ascii_uppercase
UNNAMED = re.compile(r"^[mg]\.[0-9a-z_]+$")      # compound nodes of Freebase have no name
TOP_RELATIONS = 10
TOP_ENTITIES = 10
BUDGET = {"webqsp": 3, "cwq": 4}                 # relations a run may follow
SPLITS = {"webqsp": ("train", "val", "test"), "cwq": ("val", "test")}
# Dollars per million tokens (input, cached input, output); only used to stop a run that
# would cost more than --max-cost.
PRICE = {"gpt-4.1-mini": (0.40, 0.10, 1.60), "gpt-4.1-nano": (0.10, 0.025, 0.40)}
PRICE_UNKNOWN = (1.00, 1.00, 6.00)

INTRO = "You answer a question by walking a knowledge graph one relation at a time. "
SYSTEM = {
    "start": INTRO + "The question mentions several entities. Choose the entity to start the walk from. "
             "Reply with the letter only.",
    "rel": INTRO + "Given the question, the path so far and the relations that leave the current entities, "
           "choose the relation that leads towards the answer. Reply with the letter only.",
    "stop": INTRO + "Given the question, the path so far and the entities reached, decide whether the answer "
            "to the question is among these entities or another relation has to be followed. "
            "Reply with the letter only.",
    "answer": INTRO + "Given the question, the path followed and the entities reached, choose the entity "
              "that answers the question. If several do, choose any of them. Reply with the letter only.",
}
STOP_OPTIONS = ["The answer is among these entities", "Another relation has to be followed"]


def node_names(budget):
    names = ["start"]
    for k in range(1, budget + 1):
        names.append(f"rel{k}")
        if k < budget:
            names.append(f"stop{k}")
    return names + ["answer"]


def role(name):
    return name.rstrip("0123456789")


# -- the language model ------------------------------------------------------


class Client:
    """Closed-choice questions with token probabilities, cached by prompt."""

    def __init__(self, workers=32, max_cost=6.0):
        self.workers, self.max_cost = workers, max_cost
        self.lock = threading.Lock()
        self.done, self.files = {}, {}
        self.session = httpx.Client(timeout=60)
        self.headers = {"Authorization": "Bearer " + os.environ.get("OPENAI_API_KEY", "proxy-injected")}
        self.cost, self.requests = 0.0, 0

    def _load(self, model):
        if model not in self.files:
            path = os.path.join(CACHE, f"kg_raw_{model}.jsonl")
            if os.path.exists(path):
                for line in open(path):
                    r = json.loads(line)
                    self.done[(model, r["k"])] = r["p"]
            self.files[model] = path

    def ask(self, model, system, user, n_options):
        key = (model, hashlib.sha1((system + "\n" + user).encode()).hexdigest()[:20])
        with self.lock:
            self._load(model)
            if key in self.done:
                return self.done[key]
            if self.cost > self.max_cost:
                raise RuntimeError(f"stopped: spent about ${self.cost:.2f}, above --max-cost")
        body = {"model": model, "logprobs": True, "top_logprobs": 5,
                "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}]}
        if model.startswith(("gpt-4", "gpt-3")):
            body.update(temperature=0, max_tokens=1, top_logprobs=20)
        else:  # newer models take neither parameter
            body.update(max_completion_tokens=16)
        for attempt in range(8):
            try:
                resp = self.session.post(URL, json=body, headers=self.headers)
                if resp.status_code == 200:
                    break
                if resp.status_code in (429, 500, 502, 503, 504):
                    time.sleep(min(30, 1.5 ** attempt))
                    continue
                raise RuntimeError(f"{resp.status_code}: {resp.text[:300]}")
            except httpx.HTTPError:
                time.sleep(min(30, 1.5 ** attempt))
        else:
            raise RuntimeError("request kept failing")
        data = resp.json()
        probs = [0.0] * n_options
        content = (data["choices"][0].get("logprobs") or {}).get("content") or []
        for cand in (content[0]["top_logprobs"] if content else []):
            tok = cand["token"].strip().upper()
            if len(tok) == 1 and tok in LETTERS[:n_options]:
                probs[LETTERS.index(tok)] += math.exp(cand["logprob"])
        probs = [round(min(p, 1.0), 6) for p in probs]  # all zero if no letter came back
        usage = data.get("usage", {})
        cached = (usage.get("prompt_tokens_details") or {}).get("cached_tokens", 0)
        p_in, p_cached, p_out = PRICE.get(model, PRICE_UNKNOWN)
        with self.lock:
            self.cost += ((usage.get("prompt_tokens", 0) - cached) * p_in + cached * p_cached
                          + usage.get("completion_tokens", 0) * p_out) / 1e6
            self.requests += 1
            self.done[key] = probs
            with open(self.files[model], "a") as f:
                f.write(json.dumps({"k": key[1], "p": probs}) + "\n")
        return probs


# -- the environment ---------------------------------------------------------


class Embedder:
    """Sentence embeddings that pre-rank the relations offered to the model."""

    def __init__(self):
        from sentence_transformers import SentenceTransformer
        self.model = SentenceTransformer("all-MiniLM-L6-v2")
        self.cache = {}

    def __call__(self, texts):
        new = [t for t in dict.fromkeys(texts) if t not in self.cache]
        if new:
            vecs = self.model.encode([t.replace(".", " ").replace("_", " ") for t in new],
                                     normalize_embeddings=True, batch_size=256)
            self.cache.update(zip(new, vecs))
        return np.stack([self.cache[t] for t in texts]) if texts else np.zeros((0, 384))  # an empty subgraph


def prepare(row, budget, embed):
    """Adjacency of the question subgraph and the distance of every entity to a gold answer."""
    adj = defaultdict(lambda: defaultdict(set))
    for h, r, t in row["graph"]:
        adj[h][r].add(t)
        adj[t][r].add(h)
    gold = set(row["a_entity"])
    dist = {e: 0 for e in gold if e in adj}
    frontier = list(dist)
    while frontier and max(dist.values()) < budget:
        nxt = []
        for u in frontier:
            for targets in adj[u].values():
                for v in targets:
                    if v not in dist:
                        dist[v] = dist[u] + 1
                        nxt.append(v)
        frontier = nxt
    topics = [e for e in dict.fromkeys(row["q_entity"]) if e in adj]
    relations = sorted({r for rels in adj.values() for r in rels})
    return dict(id=row["id"], question=row["question"], topics=topics, gold=gold, adj=adj, dist=dist,
                qvec=embed([row["question"]])[0], rvec=dict(zip(relations, embed(relations))))


def show(entities, limit=5):
    names = [e if not UNNAMED.match(e) else "(unnamed node)" for e in entities[:limit]]
    more = f"; and {len(entities) - limit} more" if len(entities) > limit else ""
    return "; ".join(n[:60] for n in names) + more


def options(items):
    return "\n".join(f"{LETTERS[i]}. {x}" for i, x in enumerate(items))


def episode(q, ask, budget, models):
    """One free run. Returns (steps, success) with steps = [(node, confidence, wrong)]."""
    adj, dist, gold = q["adj"], q["dist"], q["gold"]
    steps = []
    alive = lambda entities, left: min((dist.get(e, 99) for e in entities), default=99) <= left

    def decide(node, user, n):
        probs = ask(models.get(node, models["*"]), SYSTEM[role(node)], user, n)
        c = int(np.argmax(probs))
        return c, float(probs[c])

    head = f"Question: {q['question']}\n"
    topics = q["topics"][:TOP_ENTITIES]
    start = topics[0]
    if len(topics) > 1:
        c, conf = decide("start", head + "Entities:\n" + options(topics) + "\nAnswer:", len(topics))
        start = topics[c]
        steps.append(("start", conf, not alive([start], budget)))
    current, visited, path = {start}, {start}, [start]
    for k in range(1, budget + 1):
        reach = defaultdict(set)
        for e in current:
            for r, targets in adj[e].items():
                reach[r] |= targets - visited
        cands = sorted(r for r, t in reach.items() if t)
        if not cands:
            break
        sims = np.stack([q["rvec"][r] for r in cands]) @ q["qvec"]
        cands = [cands[i] for i in np.argsort(-sims, kind="stable")[:TOP_RELATIONS]]
        state = head + f"Path so far: {' -> '.join(path)}\nCurrent entities: {show(sorted(current))}\n"
        c = 0
        if len(cands) > 1 or k == 1:
            c, conf = decide(f"rel{k}", state + "Relations:\n" + options(cands) + "\nAnswer:", len(cands))
        current = reach[cands[c]]
        if len(cands) > 1 or k == 1:
            steps.append((f"rel{k}", conf, not alive(current, budget - k)))
        visited |= current
        path.append(cands[c])
        named = sorted(e for e in current if not UNNAMED.match(e))
        if not named or k == budget:
            continue   # a compound node has no name to stop at; the last relation ends the walk
        c, conf = decide(f"stop{k}", head + f"Path so far: {' -> '.join(path)}\nEntities reached: "
                         f"{show(named, 8)}\n" + options(STOP_OPTIONS) + "\nAnswer:", 2)
        steps.append((f"stop{k}", conf, (not gold & set(named[:TOP_ENTITIES])) if c == 0 else bool(gold & set(named))))
        if c == 0:
            break
    final = sorted(e for e in current if not UNNAMED.match(e))[:TOP_ENTITIES]
    if len(path) == 1 or not final:
        return steps, False
    pick = final[0]
    if len(final) > 1:
        c, conf = decide("answer", head + f"Path followed: {' -> '.join(path)}\nEntities:\n" + options(final)
                         + "\nAnswer:", len(final))
        pick = final[c]
        steps.append(("answer", conf, pick not in gold))
    return steps, pick in gold


# -- running a dataset --------------------------------------------------------


def rows_of(source, name, limit=0):
    import pyarrow.parquet as pq
    files = [f for split in SPLITS[name] for f in sorted(glob.glob(os.path.join(source, f"{name}_{split}*.parquet")))]
    if not files:
        raise SystemExit(f"no parquet files for {name} in {source}")
    seen = 0
    for path in files:
        pf = pq.ParquetFile(path)
        for g in range(pf.num_row_groups):
            for batch in pf.read_row_group(g).to_batches(max_chunksize=64):
                rows = batch.to_pylist()
                yield rows[:limit - seen] if limit else rows
                seen += len(rows)
                if limit and seen >= limit:
                    return


def run(name, source, client, embed, variants, limit=0):
    """Free runs of the base workflow and of every edited variant, on the same questions."""
    budget = BUDGET[name]
    out = {label: [] for label in variants}
    ids, skipped = [], 0
    with ThreadPoolExecutor(client.workers) as pool:
        for rows in rows_of(source, name, limit):
            qs = [prepare(r, budget, embed) for r in rows]
            skipped += sum(1 for q in qs if not q["topics"])
            qs = [q for q in qs if q["topics"]]    # no entity of the question in the graph: nothing to run
            ids += [q["id"] for q in qs]
            for label, models in variants.items():
                out[label] += list(pool.map(lambda q: episode(q, client.ask, budget, models), qs))
            print(f"\r{name}: {len(ids)} questions, {client.requests} requests, ${client.cost:.2f}",
                  end="", flush=True)
    print()
    return ids, out, skipped


def to_arrays(runs, names):
    index = {n: i for i, n in enumerate(names)}
    width = max(len(steps) for steps, _ in runs)
    path_node = -np.ones((len(runs), width), dtype=np.int32)
    path_conf = np.ones((len(runs), width))
    dev_pos = -np.ones(len(runs), dtype=np.int32)
    for i, (steps, ok) in enumerate(runs):
        for j, (node, conf, _) in enumerate(steps):
            path_node[i, j], path_conf[i, j] = index[node], conf
        if not ok:
            wrong = [j for j, s in enumerate(steps) if s[2]]
            dev_pos[i] = wrong[0] if wrong else len(steps) - 1   # no decision marked: charge the last one
    return path_node, path_conf, dev_pos


def describe(runs, names):
    n = len(runs)
    ok = np.array([s for _, s in runs])
    visits = defaultdict(list)
    for steps, _ in runs:
        for node, conf, wrong in steps:
            visits[node].append((conf, wrong))
    print(f"  {n} runs, success {ok.mean():.3f}, decisions per run "
          f"{np.mean([len(s) for s, _ in runs]):.2f} (max {max(len(s) for s, _ in runs)})")
    for node in names:
        if visits[node]:
            v = np.array(visits[node])
            print(f"  {node:7s} share {len(v) / n:.3f}  wrong {v[:, 1].mean():.3f}  conf {v[:, 0].mean():.3f}  "
                  f"conf>=0.999 {np.mean(v[:, 0] >= 0.999):.2f}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("name", choices=sorted(BUDGET))
    ap.add_argument("--source", required=True, help="directory with the parquet files")
    ap.add_argument("--model", default="gpt-4.1-mini")
    ap.add_argument("--edit", action="append", default=[], metavar="LABEL=NODE:MODEL",
                    help="also run the workflow with the model of one node replaced, e.g. answer=answer:gpt-5.4-mini")
    ap.add_argument("--workers", type=int, default=32)
    ap.add_argument("--limit", type=int, default=0, help="pilot on the first N questions; saves nothing")
    ap.add_argument("--max-cost", type=float, default=6.0, help="stop when the requests of this run cost more")
    a = ap.parse_args()
    os.makedirs(CACHE, exist_ok=True)
    names = node_names(BUDGET[a.name])
    variants = {"base": {"*": a.model}}
    edits = {}
    for item in a.edit:
        label, spec = item.split("=")
        node, model = spec.split(":")
        assert node in names, node
        variants[label] = {"*": a.model, node: model}
        edits[label] = dict(node=names.index(node), model=model)
    client = Client(a.workers, a.max_cost)
    ids, out, skipped = run(a.name, a.source, client, Embedder(), variants, a.limit)
    print(f"{client.requests} new requests, about ${client.cost:.2f}; {skipped} questions without a start entity")
    for label, runs in out.items():
        print(label)
        describe(runs, names)
    if a.limit:
        return
    arrays = {}
    for label, runs in out.items():
        suffix = "" if label == "base" else f"__{label}"
        for key, value in zip(("path_node", "path_conf", "dev_pos"), to_arrays(runs, names)):
            arrays[key + suffix] = value
    os.makedirs(DATA, exist_ok=True)
    np.savez_compressed(os.path.join(DATA, f"kg_{a.name}.npz"), ids=np.array(ids), nodes=np.array(names),
                        model=a.model, edits=json.dumps(edits), skipped=skipped, budget=BUDGET[a.name], **arrays)
    print("wrote", os.path.join(DATA, f"kg_{a.name}.npz"))
    # Number of distinct prompts each model has answered so far, over all benchmarks.
    counts = {os.path.basename(path)[len("kg_raw_"):-len(".jsonl")]: sum(1 for _ in open(path))
              for path in sorted(glob.glob(os.path.join(CACHE, "kg_raw_*.jsonl")))}
    json.dump(counts, open(os.path.join(DATA, "kg_requests.json"), "w"), indent=1)


if __name__ == "__main__":
    main()
