"""Build the CLINC150 workflow with a hosted language model at every decision node.

Each node is a zero-shot multiple-choice question; the confidence is the probability the
model assigns to the option letter it answers with, read from the token log-probabilities.
Raw responses are cached so the run can be resumed.

Writes, in the format of prep_data.py, cache/clinc_llm.npz (scope -> domain -> intent on
all traffic) and cache/clinc_llm_inscope.npz (domain -> intent on the in-scope traffic).
Two alternatives are stored for the edit experiments: the trained embedding classifier of
cache/clinc.npz for every node (pred_alt / conf_alt), and a newer language model for one
leaf (pred_alt2 / conf_alt2).

With --shots every prompt also carries labelled examples from the training split (the files
get the suffix _fs); the examples make each prompt long enough for the provider's prompt cache.
With --verbal the model states its own confidence, for providers that expose no token
probabilities (suffix _claude): Anthropic's Messages API by default, or OpenRouter.

The answers are cached under cache/ (not tracked) and the resulting tables are written to
data/ (tracked), so the experiments can be re-run without calling any model. Only the texts
of CLINC150 and data/clinc_base.npz are needed to run this script.

Usage: OPENAI_API_KEY=... python llm_nodes.py [--shots]
       ANTHROPIC_API_KEY=... python llm_nodes.py --verbal [--model claude-haiku-5-5]
       OPENROUTER_API_KEY=... python llm_nodes.py --verbal --provider openrouter
"""
import argparse
import json
import math
import os
import re
import string
import threading
import time
from concurrent.futures import ThreadPoolExecutor

import numpy as np
import pandas as pd
import httpx
from huggingface_hub import hf_hub_download

CACHE = os.environ.get("ANP_CACHE", os.path.join(os.path.dirname(__file__), "cache"))
DATA = os.path.join(os.path.dirname(__file__), "data")
ANTHROPIC_URL = "https://api.anthropic.com/v1/messages"
URL = "https://api.openai.com/v1/chat/completions"
OPENROUTER_URL = "https://openrouter.ai/api/v1/chat/completions"
VERBAL = ('Reply with a JSON object and nothing else, without explanation: '
          '{"answer": "<letter>", "confidence": <number>}, where confidence is the probability '
          "between 0 and 1 that your answer is correct.")
LETTERS = string.ascii_uppercase


def words(name):
    return name.replace("_", " ")


def load_clinc():
    import datasets

    frames = [pd.read_parquet(hf_hub_download("clinc/clinc_oos", f"plus/{s}-00000-of-00001.parquet",
                                              repo_type="dataset")) for s in ("validation", "test")]
    pool = pd.concat(frames, ignore_index=True)
    intent_names = datasets.load_dataset("clinc/clinc_oos", "plus", split="train").features["intent"].names
    domains = json.load(open(os.path.join(DATA, "clinc_domains.json")))
    dom_names = sorted(domains)
    intents = {d: sorted(domains[d]) for d in dom_names}
    return pool, [intent_names[i] for i in pool.intent], dom_names, intents


def train_examples(dom_names, intents, seed=0):
    """In-context examples per node, drawn from the training split: lists of (request, letter)."""
    import datasets

    train = pd.read_parquet(hf_hub_download("clinc/clinc_oos", "plus/train-00000-of-00001.parquet",
                                            repo_type="dataset"))
    names = datasets.load_dataset("clinc/clinc_oos", "plus", split="train").features["intent"].names
    label = np.array([names[i] for i in train.intent])
    rng = np.random.default_rng(seed)
    pick = lambda mask, m: [train.text[i] for i in rng.choice(np.where(mask)[0], m, replace=False)]
    dom_of = {i: d for d in dom_names for i in intents[d]}
    in_scope = label != "oos"
    shots = [[(t, "A") for t in pick(in_scope, 26)] + [(t, "B") for t in pick(~in_scope, 14)]]
    domain = np.array([dom_of.get(l, "") for l in label])
    shots.append([(t, LETTERS[j]) for j, d in enumerate(dom_names) for t in pick(domain == d, 4)])
    for d in dom_names:
        shots.append([(t, LETTERS[j]) for j, i in enumerate(intents[d]) for t in pick(label == i, 5)])
    for group in shots:
        rng.shuffle(group)
    return shots


def questions(dom_names, intents, shots=None):
    """One (system prompt, option count) per node; the request goes in the user message."""
    head = ("You route requests sent to a virtual assistant. Read the request and answer the "
            "question with the letter of the best option. Reply with the letter only.\n\n")
    topics = "\n".join(f"- {words(d)}: {', '.join(words(i) for i in intents[d])}" for d in dom_names)
    out = [(head + "The assistant supports these topics and intents:\n" + topics +
            "\n\nQuestion: Is the request covered by one of the supported intents?\n"
            "A. Yes, it matches a supported intent\nB. No, it is out of scope", 2)]
    opts = "\n".join(f"{LETTERS[j]}. {words(d)}: {', '.join(words(i) for i in intents[d])}"
                     for j, d in enumerate(dom_names))
    out.append((head + "Question: Which topic is the request about?\n" + opts, len(dom_names)))
    for d in dom_names:
        opts = "\n".join(f"{LETTERS[j]}. {words(i)}" for j, i in enumerate(intents[d]))
        out.append((head + f"The request is about {words(d)}.\nQuestion: Which intent is it?\n" + opts,
                    len(intents[d])))
    if shots:
        out = [(system + "\n\nExamples:\n" + "\n".join(f"Request: {t}\nAnswer: {a}" for t, a in group), n_opt)
               for (system, n_opt), group in zip(out, shots)]
    return out


class Client:
    def __init__(self, model, raw_path, workers=16, verbal=False, provider="openai"):
        self.model, self.raw_path, self.workers, self.verbal = model, raw_path, workers, verbal
        self.provider, self.temperature = provider, True
        self.lock = threading.Lock()
        self.done = {}
        if os.path.exists(raw_path):
            for line in open(raw_path):
                r = json.loads(line)
                self.done[(r["node"], r["i"])] = r["probs"]
        self.session = httpx.Client(timeout=60)
        var = {"openai": "OPENAI_API_KEY", "openrouter": "OPENROUTER_API_KEY", "anthropic": "ANTHROPIC_API_KEY"}
        key = os.environ.get(var[provider], "proxy-injected")
        if provider == "anthropic":
            self.headers = {"x-api-key": key, "anthropic-version": "2023-06-01"}
        else:
            self.headers = {"Authorization": "Bearer " + key}
        self.url = {"openai": URL, "openrouter": OPENROUTER_URL, "anthropic": ANTHROPIC_URL}[provider]
        self.tokens = [0, 0, 0]  # prompt, cached, completion

    def ask(self, node, i, system, n_options, text):
        key = (node, i)
        if key in self.done:
            return self.done[key]
        if self.verbal:
            system = system.replace("Reply with the letter only.", VERBAL)
            user = {"role": "user", "content": f"Request: {text}"}
            if self.provider == "anthropic":
                body = {"model": self.model, "max_tokens": 200, "system": system, "messages": [user]}
            else:
                body = {"model": self.model, "max_tokens": 200,
                        "messages": [{"role": "system", "content": system}, user]}
            if self.temperature:
                body["temperature"] = 0
        else:
            body = {"model": self.model, "logprobs": True, "top_logprobs": 5,
                    "messages": [{"role": "system", "content": system},
                                 {"role": "user", "content": f"Request: {text}\nAnswer:"}]}
            if self.model.startswith(("gpt-4", "gpt-3")):
                body.update(temperature=0, max_tokens=1, top_logprobs=20)
            else:  # newer models take neither parameter
                body.update(max_completion_tokens=16)
        for attempt in range(8):
            try:
                resp = self.session.post(self.url, json=body, headers=self.headers)
                if resp.status_code == 200:
                    break
                if resp.status_code in (429, 500, 502, 503, 504, 529):
                    time.sleep(min(30, 1.5 ** attempt))
                    continue
                if resp.status_code == 400 and "temperature" in resp.text and body.pop("temperature", None) == 0:
                    self.temperature = False  # this model takes no temperature
                    continue
                raise RuntimeError(f"{resp.status_code}: {resp.text[:300]}")
            except httpx.HTTPError:
                time.sleep(min(30, 1.5 ** attempt))
        else:
            raise RuntimeError("request kept failing")
        data = resp.json()
        probs = [0.0] * n_options
        if self.verbal:
            # The stated confidence goes to the stated answer; anything unreadable scores zero.
            if self.provider == "anthropic":
                reply = "".join(b.get("text", "") for b in data.get("content", []) if b.get("type") == "text")
            else:
                reply = data["choices"][0]["message"].get("content") or ""
            letter = re.search(r'"answer"\s*:\s*"\s*([A-Za-z])', reply)
            number = re.search(r'"confidence"\s*:\s*([0-9]*\.?[0-9]+)', reply)
            if letter and number and letter.group(1).upper() in LETTERS[:n_options]:
                probs[LETTERS.index(letter.group(1).upper())] = float(number.group(1))
        else:
            content = data["choices"][0]["logprobs"]["content"]
            for cand in (content[0]["top_logprobs"] if content else []):
                tok = cand["token"].strip().upper()
                if len(tok) == 1 and tok in LETTERS[:n_options]:
                    probs[LETTERS.index(tok)] += math.exp(cand["logprob"])
        # Raw mass per option, all zero if no letter came back. Six decimals are kept, in
        # memory as in the cache file, so that a resumed run reproduces the same numbers.
        probs = [round(min(p, 1.0), 6) for p in probs]
        usage = data.get("usage", {})
        with self.lock:
            self.tokens[0] += usage.get("prompt_tokens", usage.get("input_tokens", 0))
            self.tokens[1] += (usage.get("prompt_tokens_details") or {}).get("cached_tokens", 0)
            self.tokens[2] += usage.get("completion_tokens", usage.get("output_tokens", 0))
            self.done[key] = probs
            with open(self.raw_path, "a") as f:
                f.write(json.dumps({"node": node, "i": int(i), "probs": probs}) + "\n")
        return probs

    def run(self, jobs):
        """jobs: list of (node, i, system, n_options, text). Returns {(node, i): probs}."""
        with ThreadPoolExecutor(self.workers) as pool:
            list(pool.map(lambda j: self.ask(*j), jobs))
        return self.done


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="gpt-4.1-mini")
    ap.add_argument("--upgrade", default="gpt-5.4-mini", help="newer model for the edited leaf")
    ap.add_argument("--workers", type=int, default=32)
    ap.add_argument("--shots", action="store_true", help="add in-context examples from the training split")
    ap.add_argument("--verbal", action="store_true", help="the model states its confidence")
    ap.add_argument("--provider", choices=["anthropic", "openrouter"], default="anthropic",
                    help="where --verbal sends its requests")
    ap.add_argument("--limit", type=int, default=0, help="pilot on the first N inputs of a shuffle; saves nothing")
    a = ap.parse_args()
    if a.verbal and a.model == "gpt-4.1-mini":
        a.model = "claude-haiku-5-5" if a.provider == "anthropic" else "anthropic/claude-haiku-5.5"
    provider = a.provider if a.verbal else "openai"
    tag = "_claude" if a.verbal else "_fs" if a.shots else ""
    slug = a.model.replace("/", "_")

    pool, names, dom_names, intents = load_clinc()
    base = np.load(os.path.join(DATA, "clinc_base.npz"))
    truth, nxt = base["truth"].astype(int), base["nxt"].astype(int)
    n, k = truth.shape
    idx = np.sort(np.random.default_rng(0).permutation(n)[: a.limit]) if a.limit else np.arange(n)
    in_scope = truth[:, 0] == 0
    qs = questions(dom_names, intents, train_examples(dom_names, intents) if a.shots else None)
    texts = pool.text.values
    raw = f"llm_raw_{slug}{'_fs' if a.shots else ''}.jsonl"
    os.makedirs(CACHE, exist_ok=True)
    client = Client(a.model, os.path.join(CACHE, raw), a.workers, a.verbal, provider)
    newer = Client(a.upgrade, os.path.join(CACHE, f"llm_raw_{a.upgrade}.jsonl"), a.workers)

    def table():
        return np.zeros((n, k), dtype=int), np.zeros((n, k)), np.zeros((n, k), dtype=bool)

    def collect(node, rows, client, out):
        pred, conf, have = out
        rows = [i for i in rows if not have[i, node]]
        system, n_opt = qs[node]
        client.run([(node, i, system, n_opt, texts[i]) for i in rows])
        for i in rows:
            p = client.done[(node, i)]
            pred[i, node], conf[i, node], have[i, node] = int(np.argmax(p)), float(max(p)), True

    # A node is evaluated on the inputs whose free run or oracle route can visit it, in the
    # three-level workflow (scope -> domain -> intent) and in the two-level one without scope.
    main_out = table()
    pred, conf, have = main_out
    collect(0, idx, client, main_out)
    collect(1, [i for i in idx if pred[i, 0] == 0 or in_scope[i]], client, main_out)
    for d in range(len(dom_names)):
        rows = [i for i in idx if truth[i, 2 + d] >= 0 or (have[i, 1] and pred[i, 1] == d
                                                           and (pred[i, 0] == 0 or in_scope[i]))]
        collect(2 + d, rows, client, main_out)
    print(f"{a.model}: {int(have.sum())} answers, tokens prompt={client.tokens[0]} "
          f"(cached {client.tokens[1]}) completion={client.tokens[2]}")

    def report(out, nodes):
        pred, conf, have = out
        for v in nodes:
            on = have[:, v] & (truth[:, v] >= 0)
            if on.any():
                ok = pred[on, v] == truth[on, v]
                wrong = conf[on, v][~ok].mean() if (~ok).any() else 0
                print(f"  {str(base['names'][v]):28s} n={on.sum():5d} acc={ok.mean():.3f} conf right="
                      f"{conf[on, v][ok].mean():.3f} wrong={wrong:.3f} saturated={(conf[on, v] >= 0.999999).mean():.2f}")

    report(main_out, range(k))

    # The same leaf answered by a newer model, on every input that can reach it
    # (zero-shot only: the edit experiment is not repeated with examples).
    leaf = 3
    pred2, conf2 = pred.copy(), conf.copy()
    if a.limit:
        return
    if not (a.shots or a.verbal):
        new_out = table()
        collect(leaf, [i for i in idx if have[i, leaf]], newer, new_out)
        print(f"{a.upgrade}: tokens prompt={newer.tokens[0]} completion={newer.tokens[2]}")
        report(new_out, [leaf])
        pred2[:, leaf], conf2[:, leaf] = new_out[0][:, leaf], new_out[1][:, leaf]

    common = dict(model=np.array(a.model), upgrade=np.array(a.upgrade))
    np.savez_compressed(os.path.join(DATA, f"clinc_llm{tag}.npz"), pred=pred.astype(np.int16), conf=conf,
                        truth=base["truth"], nxt=base["nxt"], names=base["names"],
                        pred_alt=base["pred"], conf_alt=base["conf"].astype(float),
                        pred_alt2=pred2.astype(np.int16), conf_alt2=conf2, **common)

    # Two-level workflow on the in-scope traffic: drop the scope node and re-index.
    rows = np.where(in_scope)[0]
    shift = lambda a: a[rows][:, 1:]
    nxt2 = nxt[1:].copy()
    nxt2[nxt2 >= 0] -= 1
    np.savez_compressed(os.path.join(DATA, f"clinc_llm{tag}_inscope.npz"), pred=shift(pred).astype(np.int16),
                        conf=shift(conf), truth=shift(truth).astype(np.int16), nxt=nxt2.astype(np.int16),
                        names=base["names"][1:], pred_alt=shift(base["pred"].astype(int)).astype(np.int16),
                        conf_alt=shift(base["conf"].astype(float)), pred_alt2=shift(pred2).astype(np.int16),
                        conf_alt2=shift(conf2), **common)
    print(f"saved data/clinc_llm{tag}.npz and data/clinc_llm{tag}_inscope.npz")


if __name__ == "__main__":
    main()
