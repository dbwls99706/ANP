"""Build decision workflows from public datasets and cache every node's
prediction and confidence for every example.

Each workflow is stored as an .npz with:
  pred[N, K]   argmax choice of node k on example i (every node sees the raw text)
  conf[N, K]   top-1 probability of that choice
  truth[N, K]  ground-truth choice of node k, or -1 when k is not on the oracle route
  nxt[K, C]    successor node of each choice, -1 for a sink, -2 for an unused slot
  names[K]     node names
pred_alt / conf_alt hold an alternative model for every node (used by the
edit and heterogeneity experiments).
"""
import gzip
import json
import os
import sys

import numpy as np
import pandas as pd
from huggingface_hub import hf_hub_download
from sklearn.feature_extraction.text import TfidfVectorizer
from sklearn.linear_model import LogisticRegression

CACHE = os.environ.get("ANP_CACHE", os.path.join(os.path.dirname(__file__), "cache"))
os.makedirs(CACHE, exist_ok=True)


def embed(texts, model_name, tag):
    path = os.path.join(CACHE, f"emb_{tag}.npy")
    if os.path.exists(path):
        return np.load(path)
    from sentence_transformers import SentenceTransformer

    model = SentenceTransformer(model_name, device="cpu")
    out = model.encode(list(texts), batch_size=128, show_progress_bar=False, normalize_embeddings=True)
    np.save(path, out)
    return out


def fit(x, y, c=10.0):
    clf = LogisticRegression(C=c, max_iter=2000)
    clf.fit(x, y)
    return clf


def top(clf, x, n_classes):
    """Return (argmax class index, top probability) with classes mapped to 0..n-1."""
    proba = np.zeros((x.shape[0], n_classes))
    proba[:, clf.classes_] = clf.predict_proba(x)
    return proba.argmax(1), proba.max(1)


def save(name, pred, conf, truth, nxt, names, extra=None):
    out = dict(pred=pred.astype(np.int16), conf=conf.astype(np.float32), truth=truth.astype(np.int16),
               nxt=nxt.astype(np.int16), names=np.array(names))
    out.update(extra or {})
    np.savez_compressed(os.path.join(CACHE, f"{name}.npz"), **out)
    n, k = pred.shape
    on = truth >= 0
    acc = [(pred[on[:, j], j] == truth[on[:, j], j]).mean() if on[:, j].any() else np.nan for j in range(k)]
    print(f"[{name}] N={n} K={k} on-route accuracy per node: min={np.nanmin(acc):.3f} "
          f"mean={np.nanmean(acc):.3f} root={acc[0]:.3f}")


def build_clinc():
    """scope (in-scope / out-of-scope) -> domain (10) -> intent within domain (15)."""
    frames = {}
    for split in ["train", "validation", "test"]:
        p = hf_hub_download("clinc/clinc_oos", f"plus/{split}-00000-of-00001.parquet", repo_type="dataset")
        frames[split] = pd.read_parquet(p)
    import datasets

    intent_names = datasets.load_dataset("clinc/clinc_oos", "plus", split="train").features["intent"].names
    domains_path = os.path.join(CACHE, "clinc_domains.json")
    if not os.path.exists(domains_path):
        import urllib.request

        urllib.request.urlretrieve(
            "https://raw.githubusercontent.com/clinc/oos-eval/master/data/domains.json", domains_path)
    domains = json.load(open(domains_path))
    dom_names = sorted(domains)
    dom_of, idx_in_dom = {}, {}
    for d, name in enumerate(dom_names):
        for j, intent in enumerate(sorted(domains[name])):
            dom_of[intent], idx_in_dom[intent] = d, j

    def labels(df):
        names = [intent_names[i] for i in df.intent]
        oos = np.array([n == "oos" for n in names])
        dom = np.array([dom_of.get(n, -1) for n in names])
        sub = np.array([idx_in_dom.get(n, -1) for n in names])
        return oos, dom, sub

    train = frames["train"]
    pool = pd.concat([frames["validation"], frames["test"]], ignore_index=True)
    xtr = embed(train.text, "sentence-transformers/all-MiniLM-L6-v2", "clinc_train")
    xpo = embed(pool.text, "sentence-transformers/all-MiniLM-L6-v2", "clinc_pool")
    oos_tr, dom_tr, sub_tr = labels(train)
    oos_po, dom_po, sub_po = labels(pool)

    k = 2 + len(dom_names)
    n = len(pool)
    pred = np.zeros((n, k), int)
    conf = np.zeros((n, k))
    truth = -np.ones((n, k), int)
    nxt = -2 * np.ones((k, 15), int)

    scope = LogisticRegression(C=10.0, max_iter=2000, class_weight="balanced").fit(xtr, oos_tr.astype(int))
    pred[:, 0], conf[:, 0] = top(scope, xpo, 2)
    truth[:, 0] = oos_po.astype(int)
    nxt[0, 0], nxt[0, 1] = 1, -1

    ins = ~oos_tr
    domain = fit(xtr[ins], dom_tr[ins])
    pred[:, 1], conf[:, 1] = top(domain, xpo, len(dom_names))
    truth[~oos_po, 1] = dom_po[~oos_po]
    nxt[1, : len(dom_names)] = 2 + np.arange(len(dom_names))

    for d in range(len(dom_names)):
        m = ins & (dom_tr == d)
        clf = fit(xtr[m], sub_tr[m])
        pred[:, 2 + d], conf[:, 2 + d] = top(clf, xpo, 15)
        on = (~oos_po) & (dom_po == d)
        truth[on, 2 + d] = sub_po[on]
        nxt[2 + d, :15] = -1

    # Alternative model for every node (edit and heterogeneity experiments): bag-of-words
    # logistic regression with weak regularisation, a different and more overconfident family.
    tfidf = TfidfVectorizer(ngram_range=(1, 2), min_df=2, sublinear_tf=True)
    ttr, tpo = tfidf.fit_transform(train.text), tfidf.transform(pool.text)
    p_alt = np.zeros((n, k), int)
    c_alt = np.zeros((n, k))
    alt = LogisticRegression(C=200.0, max_iter=3000, class_weight="balanced").fit(ttr, oos_tr.astype(int))
    p_alt[:, 0], c_alt[:, 0] = top(alt, tpo, 2)
    p_alt[:, 1], c_alt[:, 1] = top(fit(ttr[ins], dom_tr[ins], c=200.0), tpo, len(dom_names))
    for d in range(len(dom_names)):
        m = ins & (dom_tr == d)
        p_alt[:, 2 + d], c_alt[:, 2 + d] = top(fit(ttr[m], sub_tr[m], c=200.0), tpo, 15)

    names = ["scope", "domain"] + [f"intent[{d}]" for d in dom_names]
    save("clinc", pred, conf, truth, nxt, names,
         dict(pred_alt=p_alt.astype(np.int16), conf_alt=c_alt.astype(np.float32)))


def build_massive(langs=("en", "de", "es", "ko")):
    """language -> scenario (one model per language) -> intent within scenario (shared, a join)."""
    def load(split):
        rows = []
        for lang in langs:
            pi = hf_hub_download("mteb/amazon_massive_intent", f"{split}/{lang}.json.gz", repo_type="dataset")
            ps = hf_hub_download("mteb/amazon_massive_scenario", f"{split}/{lang}.json.gz", repo_type="dataset")
            intents = [json.loads(l) for l in gzip.open(pi, "rt")]
            scen = {r["id"]: r["label_text"] for r in (json.loads(l) for l in gzip.open(ps, "rt"))}
            for r in intents:
                rows.append(dict(text=r["text"], lang=lang, intent=r["label_text"], scenario=scen[r["id"]]))
        return pd.DataFrame(rows)

    train = load("train")
    pool = pd.concat([load("validation"), load("test")], ignore_index=True)
    model = "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2"
    xtr = embed(train.text, model, "massive_train")
    xpo = embed(pool.text, model, "massive_pool")

    scen_names = sorted(train.scenario.unique())
    intents_of = {s: sorted(train.intent[train.scenario == s].unique()) for s in scen_names}
    multi = [s for s in scen_names if len(intents_of[s]) > 1]
    n_l, n_s = len(langs), len(scen_names)
    k = 1 + n_l + len(multi)
    cmax = max(n_s, max(len(v) for v in intents_of.values()))
    n = len(pool)
    pred = np.zeros((n, k), int)
    conf = np.zeros((n, k))
    truth = -np.ones((n, k), int)
    nxt = -2 * np.ones((k, cmax), int)

    lang_idx = {l: i for i, l in enumerate(langs)}
    ytr_l = train.lang.map(lang_idx).values
    ypo_l = pool.lang.map(lang_idx).values
    chars = TfidfVectorizer(analyzer="char_wb", ngram_range=(1, 3), min_df=5, sublinear_tf=True, max_features=50000)
    lang_clf = fit(chars.fit_transform(train.text), ytr_l, c=1.0)
    pred[:, 0], conf[:, 0] = top(lang_clf, chars.transform(pool.text), n_l)
    truth[:, 0] = ypo_l
    nxt[0, :n_l] = 1 + np.arange(n_l)

    scen_idx = {s: i for i, s in enumerate(scen_names)}
    ytr_s = train.scenario.map(scen_idx).values
    ypo_s = pool.scenario.map(scen_idx).values
    intent_node = {s: 1 + n_l + j for j, s in enumerate(multi)}
    for l in range(n_l):
        clf = fit(xtr[ytr_l == l], ytr_s[ytr_l == l])
        pred[:, 1 + l], conf[:, 1 + l] = top(clf, xpo, n_s)
        truth[ypo_l == l, 1 + l] = ypo_s[ypo_l == l]
        for s in scen_names:
            nxt[1 + l, scen_idx[s]] = intent_node.get(s, -1)

    for s in multi:
        v = intent_node[s]
        idx = {name: i for i, name in enumerate(intents_of[s])}
        m = train.scenario.values == s
        clf = fit(xtr[m], train.intent[m].map(idx).values)
        pred[:, v], conf[:, v] = top(clf, xpo, len(idx))
        on = pool.scenario.values == s
        truth[on, v] = pool.intent[on].map(idx).fillna(-1).astype(int).values
        nxt[v, : len(idx)] = -1

    # Alternative model for every node: character n-gram logistic regression.
    cpo, ctr = chars.transform(pool.text), chars.transform(train.text)
    p_alt, c_alt = pred.copy(), conf.copy()
    for l in range(n_l):
        p_alt[:, 1 + l], c_alt[:, 1 + l] = top(fit(ctr[ytr_l == l], ytr_s[ytr_l == l], c=50.0), cpo, n_s)
    for s in multi:
        v = intent_node[s]
        idx = {name: i for i, name in enumerate(intents_of[s])}
        m = train.scenario.values == s
        p_alt[:, v], c_alt[:, v] = top(fit(ctr[m], train.intent[m].map(idx).values, c=50.0), cpo, len(idx))

    names = ["language"] + [f"scenario[{l}]" for l in langs] + [f"intent[{s}]" for s in multi]
    save("massive", pred, conf, truth, nxt, names,
         dict(pred_alt=p_alt.astype(np.int16), conf_alt=c_alt.astype(np.float32)))


if __name__ == "__main__":
    which = sys.argv[1:] or ["clinc", "massive"]
    if "clinc" in which:
        build_clinc()
    if "massive" in which:
        build_massive()
