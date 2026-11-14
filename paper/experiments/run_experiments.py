"""Experiments for the workflow risk-certification paper.

Design: the pooled held-out data of a workflow is treated as the population.
A calibration set is n i.i.d. draws (with replacement) from it, and every
selected threshold vector is scored by its exact risk and review rate on the
population. Validity is therefore measured without test-set noise.

Usage: python run_experiments.py {main,nsweep,posthoc,edits,synth} [--trials T] [--llm]
(--llm runs main or edits on the hosted-language-model workflow built by llm_nodes.py)
"""
import argparse
import json
import os
import time
from multiprocessing import Pool

import numpy as np

import riskcert as rc

HERE = os.path.dirname(__file__)
CACHE = os.environ.get("ANP_CACHE", os.path.join(HERE, "cache"))
DATA = os.path.join(HERE, "data")  # tracked tables of the language-model workflows
RESULTS = os.path.join(HERE, "..", "results")
DELTA = 0.1
DEFAULT_N = {"clinc": 4000, "massive": 8000}
# Language-model workflows: zero-shot, with in-context examples (_fs), and a second provider
# with stated confidences (_claude); each on all traffic and on the in-scope traffic.
LLM_WORKFLOWS = [f"clinc_llm{tag}{part}" for tag in ("", "_fs", "_claude") for part in ("", "_inscope")]
DEFAULT_N.update({name: 4000 for name in LLM_WORKFLOWS})


def llm_workflows():
    return [n for n in LLM_WORKFLOWS if os.path.exists(table_path(n))]
# Token probabilities of the language model saturate, so its workflows get a longer grid tail.
LLM_TAIL = (0.9999, 0.99999, 0.999999)


# Knowledge-graph agents (kg_agent.py). Their runs are labelled by outcome, not by route.
AGENT_WORKFLOWS = ["kg_webqsp", "kg_cwq"]
DEFAULT_N.update({name: 4000 for name in AGENT_WORKFLOWS})
AGENT_EPS = [0.05, 0.10, 0.20]
AGENT_EDIT_EPS = 0.10


def agent_workflows():
    return [n for n in AGENT_WORKFLOWS if os.path.exists(table_path(n))]


def table_path(name):
    return os.path.join(DATA if name.startswith(("clinc_llm", "kg_")) else CACHE, f"{name}.npz")


def load(name, swap=None):
    """Load a workflow; swap = nodes whose model is replaced by an alternative one, given
    as node indices (alternative "alt") or (node, alternative) pairs."""
    d = np.load(table_path(name))
    if name.startswith("kg_"):
        # Stored as free runs. An edited agent is a second set of runs on the same questions.
        assert len(swap or []) <= 1
        suffix = "".join(f"__{alt}" for _, alt in swap or [])
        k = len(d["nodes"])
        tr = rc.Traces(d["path_node" + suffix].astype(np.int32), d["path_conf" + suffix].astype(float),
                       d["dev_pos" + suffix].astype(np.int32), k)
        later = np.array([[u if u > v else -1 for u in range(k)] for v in range(k)])  # the loop is unrolled
        return tr, later
    pred, conf = d["pred"].astype(int).copy(), d["conf"].astype(float).copy()
    for item in swap or []:
        v, alt = item if isinstance(item, tuple) else (item, "alt")
        pred[:, v], conf[:, v] = d[f"pred_{alt}"][:, v], d[f"conf_{alt}"][:, v]
    return rc.traces_from_tables(pred, conf, d["truth"].astype(int), d["nxt"].astype(int)), d["nxt"].astype(int)


def levels(tr):
    """Group nodes by the position at which they appear on a route."""
    depth = np.full(tr.n_nodes, -1)
    for j in range(tr.path_node.shape[1]):
        for v in np.unique(tr.path_node[:, j]):
            if v >= 0 and depth[v] < 0:
                depth[v] = j
    return [list(np.where(depth == j)[0]) for j in range(depth.max() + 1)]


def groups(name, tr):
    """Modules that share a threshold: the levels of a routing workflow, the roles of an
    agent (start, relation, stop, answer), whose unrolled steps are copies of one decision."""
    if not name.startswith("kg_"):
        return levels(tr)
    roles = [str(n).rstrip("0123456789") for n in np.load(table_path(name))["nodes"]]
    return [[v for v, r in enumerate(roles) if r == x] for x in dict.fromkeys(roles)]


def descendants(nxt, w):
    seen, stack = set(), [w]
    while stack:
        for u in nxt[stack.pop()]:
            if u >= 0 and u not in seen:
                seen.add(int(u))
                stack.append(int(u))
    return seen


def run_methods(cal, eps, rng, lv, which):
    """Return {method: (tau, certified bound or None)}."""
    k = cal.n_nodes
    out = {}
    for m in which:
        if m == "fixed-0.7":
            out[m] = (np.full(k, 0.7), None)
        elif m == "per-node-selective":
            out[m] = (rc.per_node_selective(cal, eps, DELTA), None)
        elif m == "plug-in":
            tau, b, _ = rc.select(cal, eps, DELTA, mode="min", plug_in=True)
            out[m] = (tau, None)
        elif m == "global-e2e":
            out[m] = (rc.global_threshold(cal, eps, DELTA), None)
        elif m == "e2e-pareto":
            out[m] = (rc.e2e_pareto(cal, eps, DELTA, rng), None)
        elif m == "e2e-uniform":
            out[m] = (rc.e2e_uniform(cal, eps, DELTA), None)
        elif m == "tf-node":
            out[m] = rc.select(cal, eps, DELTA, mode="tf")[:2]
        elif m in ("fd-node", "min-node"):
            out[m] = rc.select(cal, eps, DELTA, mode=m[:-5])[:2]
        elif m in ("fd-level", "min-level"):
            out[m] = rc.select(cal, eps, DELTA, mode=m[:-6], modules=lv)[:2]
        elif m == "min-global":
            out[m] = rc.select(cal, eps, DELTA, mode="min", modules=[list(range(k))])[:2]
        elif m == "anchored-node":
            out[m] = rc.anchored(cal, eps, DELTA, rng)[:2]
        elif m == "anchored-level":
            out[m] = rc.anchored(cal, eps, DELTA, rng, modules=lv)[:2]
        else:
            raise ValueError(m)
    return out


MAIN_METHODS = ["fixed-0.7", "per-node-selective", "plug-in", "global-e2e", "e2e-pareto", "e2e-uniform",
                "tf-node", "fd-node", "fd-level", "min-node", "min-level", "min-global",
                "anchored-node", "anchored-level"]
# An agent has no oracle route, so the methods that evaluate nodes on oracle-route inputs
# (per-node selective risk, teacher forcing) do not apply.
AGENT_METHODS = [m for m in MAIN_METHODS if m not in ("per-node-selective", "tf-node")]

_POP = {}


def population(name, swap=()):
    key = (name, tuple(swap))
    if key not in _POP:
        _POP[key] = load(name, list(swap))
    return _POP[key]


def summarise(rows, eps):
    risk = np.array([r[0] for r in rows])
    review = np.array([r[1] for r in rows])
    bounds = [r[2] for r in rows if r[2] is not None]
    return dict(risk_mean=float(risk.mean()), risk_q90=float(np.quantile(risk, 0.9)), risk_max=float(risk.max()),
                violation=float((risk > eps + 1e-12).mean()), review_mean=float(review.mean()),
                review_sd=float(review.std()), bound_mean=float(np.mean(bounds)) if bounds else None,
                fd_mass_mean=float(np.mean([r[3] for r in rows])) if len(rows[0]) > 3 else None,
                trials=len(rows))


# -- main comparison and calibration-size sweep ---------------------------


def _main_trial(args):
    name, n, eps_list, methods, seed = args
    pop, _ = population(name)
    rng = np.random.default_rng(seed)
    cal = pop.subset(rng.integers(0, len(pop), n))
    lv = groups(name, pop)
    res = {}
    for eps in eps_list:
        for m, (tau, b) in run_methods(cal, eps, rng, lv, methods).items():
            res[(eps, m)] = (pop.risk(tau), pop.review(tau), b, pop.charged_mass(tau, "fd"))
    return res


def collect(jobs, func, workers=4):
    with Pool(workers) as pool:
        return pool.map(func, jobs, chunksize=1)


def exp_main(trials, names=("clinc", "massive"), eps_list=(0.02, 0.05, 0.10), methods=MAIN_METHODS):
    out = {}
    eps_list = list(eps_list)
    for name in names:
        pop, _ = population(name)
        k = pop.n_nodes
        n = DEFAULT_N[name]
        t0 = time.time()
        res = collect([(name, n, eps_list, methods, s) for s in range(trials)], _main_trial)
        out[name] = dict(n=n, K=k, N=len(pop), free_run_error=pop.risk(np.zeros(k)),
                         oracle={str(e): dict(node=pop.review(rc.e2e_optimize(pop, e)),
                                              shared=pop.review(next(
                                                  (np.full(k, g) for g in rc.default_grid()
                                                   if pop.risk(np.full(k, g)) <= e), np.full(k, rc.INF))))
                                 for e in eps_list},
                         results={f"{e}|{m}": summarise([r[(e, m)] for r in res], e)
                                  for e in eps_list for m in methods})
        print(name, "main done", round(time.time() - t0), "s", flush=True)
    return out


def exp_nsweep(trials):
    out = {}
    methods = ["global-e2e", "e2e-pareto", "e2e-uniform", "fd-node", "min-node", "min-level", "min-global"]
    for name, sizes in [("clinc", [500, 1000, 2000, 4000, 8000]), ("massive", [1000, 2000, 4000, 8000, 16000])]:
        out[name] = {}
        for n in sizes:
            res = collect([(name, n, [0.05], methods, 10_000 + s) for s in range(trials)], _main_trial)
            out[name][str(n)] = {m: summarise([r[(0.05, m)] for r in res], 0.05) for m in methods}
            print(name, "n", n, "done", flush=True)
    return out


# -- post hoc retuning ----------------------------------------------------

SWEEP = [0.02, 0.04, 0.06, 0.08, 0.10]


def _posthoc_trial(args):
    name, n, seed, sweep = args
    pop, _ = population(name)
    rng = np.random.default_rng(seed)
    cal = pop.subset(rng.integers(0, len(pop), n))
    lv = groups(name, pop)
    res = {}
    certs = {"min-level": rc.certify(cal, DELTA, modules=lv, mode="min"),
             "min-node": rc.certify(cal, DELTA, mode="min")}
    split_seed = int(rng.integers(1 << 30))
    for eps in sweep:
        for m, cert in certs.items():
            tau, _ = rc.pick(cert, eps, cal.review, cal.n_nodes)
            res[(eps, m)] = (pop.risk(tau), pop.review(tau))
        # The end-to-end procedures are simply re-run for every target on the same data.
        tau = rc.global_threshold(cal, eps, DELTA)
        res[(eps, "global-e2e")] = (pop.risk(tau), pop.review(tau))
        tau = rc.e2e_pareto(cal, eps, DELTA, np.random.default_rng(split_seed))
        res[(eps, "e2e-pareto")] = (pop.risk(tau), pop.review(tau))
    return res


def exp_posthoc(trials, names=("clinc", "massive"), sweep=SWEEP):
    out = {}
    for name in names:
        res = collect([(name, DEFAULT_N[name], 20_000 + s, sweep) for s in range(trials)], _posthoc_trial)
        out[name] = {}
        for m in ["min-level", "min-node", "global-e2e", "e2e-pareto"]:
            viol = np.array([[r[(e, m)][0] > e + 1e-12 for e in sweep] for r in res])
            out[name][m] = dict(any_violation=float(viol.any(axis=1).mean()),
                                per_eps_violation=[float(v) for v in viol.mean(axis=0)],
                                review=[float(np.mean([r[(e, m)][1] for r in res])) for e in sweep],
                                risk=[float(np.mean([r[(e, m)][0] for r in res])) for e in sweep])
        out[name]["eps"] = list(sweep)
        print(name, "posthoc done", flush=True)
    return out


# -- edits ----------------------------------------------------------------


def _edit_trial(args):
    name, edit, n0, m_new, eps, seed = args
    w = edit[0] if isinstance(edit, tuple) else edit
    pop0, nxt = population(name)
    pop1, _ = population(name, (edit,))
    k = pop1.n_nodes
    rng = np.random.default_rng(seed)
    lv = groups(name, pop1)
    hit = np.where(pop1.through([w]))[0]
    reach = np.unique(pop1.path_node[hit])          # nodes on some route through the edit
    reach = reach[reach >= 0]
    i_old = rng.integers(0, len(pop1), n0)
    i_any = rng.integers(0, len(pop1), m_new)          # fresh labels on random traffic
    i_hit = hit[rng.integers(0, len(hit), m_new)]      # fresh labels on traffic through the edit
    res = {}

    def score(tau):
        return pop1.risk(tau), pop1.review(tau), None

    # (a) Recalibrate from scratch with the fresh labels only.
    fresh_any = pop1.subset(i_any)
    res["scratch: e2e-pareto"] = score(rc.e2e_pareto(fresh_any, eps, DELTA, rng))
    res["scratch: min-level"] = score(rc.select(fresh_any, eps, DELTA, mode="min", modules=lv)[0])
    # (b) Keep the old certificate for the untouched traffic, spend the labels on the rest.
    for label, modules in [("local: min-level", lv), ("local: min-node", None)]:
        cert, review_fn = rc.certify_after_edit(pop0.subset(i_old), pop1.subset(i_hit), [w], DELTA,
                                                modules=modules, reach=reach)
        res[label] = score(rc.pick(cert, eps, review_fn, k)[0])
    # (c) Reference: the old inputs were kept and are re-run through the edited workflow.
    both = pop1.subset(np.r_[i_old, i_any])
    res["rerun: e2e-pareto"] = score(rc.e2e_pareto(both, eps, DELTA, rng))
    res["rerun: min-level"] = score(rc.select(both, eps, DELTA, mode="min", modules=lv)[0])
    return res


LLM_EDITS = dict(eps=0.05, nodes={"upgrade": (2, "alt2"), "classifier": (1, "alt")})


def exp_edits(trials, only=None, names=("clinc", "massive")):
    out = {}
    # MASSIVE has no alternative language model, so its root is not edited. In the language-
    # model workflow one leaf is upgraded to a newer language model and another leaf is
    # replaced by the trained classifier.
    plans = {"clinc": dict(eps=0.05, nodes={"leaf": 3, "middle": 1, "root": 0}),
             "massive": dict(eps=0.05, nodes={"leaf": 7, "middle": 2}),
             "clinc_llm_inscope": LLM_EDITS}
    for name in agent_workflows():   # the edits that kg_agent.py ran: one node gets a newer model
        made = json.loads(str(np.load(table_path(name))["edits"]))
        plans[name] = dict(eps=AGENT_EDIT_EPS, nodes={label: (e["node"], label) for label, e in made.items()})
    plans = {k: v for k, v in plans.items() if k in names}
    if only:
        name, label = only.split(":")
        plans = {name: dict(eps=plans[name]["eps"], nodes={label: plans[name]["nodes"][label]})}
    for name, plan in plans.items():
        pop0, nxt = population(name)
        out[name] = dict(eps=plan["eps"], n0=DEFAULT_N[name], edits={})
        for label, edit in plan["nodes"].items():
            w = edit[0] if isinstance(edit, tuple) else edit
            pop1, _ = population(name, (edit,))
            grid = rc.default_grid()
            hit = pop1.through([w])
            # Deterministic checks of the locality claims on the whole population.
            assert np.array_equal(hit, pop0.through([w]))
            same = lambda a, b: np.array_equal(a[~hit], b[~hit])
            assert same(pop0.path_node, pop1.path_node) and same(pop0.path_conf, pop1.path_conf)
            assert same(pop0.dev_pos, pop1.dev_pos)
            changed = lambda mode: [int(v) for v in range(pop0.n_nodes) if not np.array_equal(
                rc.error_counts(pop0, grid, mode)[v], rc.error_counts(pop1, grid, mode)[v])]
            fd_set = {w} | descendants(nxt, w)
            routed = pop0.tf_on is not None   # footprints are statements about route labels
            info = dict(node=int(w), traffic_share=float(hit.mean()),
                        error_before=pop0.risk(np.zeros(pop0.n_nodes)),
                        error_after=pop1.risk(np.zeros(pop0.n_nodes)),
                        fd_predicted=sorted(fd_set), fd_changed=changed("fd"),
                        tf_changed=changed("tf") if routed else None,
                        min_changed=changed("min"), sizes={})
            assert not routed or (set(info["fd_changed"]) <= fd_set and set(info["tf_changed"]) <= {w})
            assert info["error_before"] != info["error_after"], "the edit changed nothing"
            for m_new in [100, 250, 500, 1000]:
                res = collect([(name, edit, DEFAULT_N[name], m_new, plan["eps"], 30_000 + s)
                               for s in range(trials)], _edit_trial)
                info["sizes"][str(m_new)] = {m: summarise([r[m] for r in res], plan["eps"]) for m in res[0]}
            out[name]["edits"][label] = info
            print(name, label, "edit done", flush=True)
    return out


# -- ablations ---------------------------------------------------------------

ABLATION_GRIDS = {
    "coarse (13)": np.r_[np.arange(10) * 0.1, 0.95, 0.99, rc.INF],
    "default (26)": None,
    "fine (103)": np.r_[np.arange(100) * 0.01, 0.995, 0.999, rc.INF],
}
ABLATION_DELTAS = [0.01, 0.05, 0.1, 0.2]


def _ablation_trial(args):
    name, n, eps, seed = args
    pop, _ = population(name)
    rng = np.random.default_rng(seed)
    cal = pop.subset(rng.integers(0, len(pop), n))
    lv = levels(pop)
    k = pop.n_nodes
    res = {}

    def score(key, tau):
        res[key] = (pop.risk(tau), pop.review(tau), None)

    for label, modules in [("node", None), ("level", lv)]:
        # One pooled binomial bound against one confidence band per module.
        for pooled in (True, False):
            tau = rc.select(cal, eps, DELTA, modules=modules, mode="min", pooled=pooled)[0]
            score(f"bound|{label}|{'pooled' if pooled else 'separate'}", tau)
        # The knapsack solution alone, without the local search on the review rate.
        cert = rc.certify(cal, DELTA, modules=modules, mode="min")
        score(f"search|{label}|knapsack only", cert.thresholds(rc.optimize(cert, eps), k))
        score(f"search|{label}|with refinement", rc.pick(cert, eps, cal.review, k)[0])
        for grid_name, grid in ABLATION_GRIDS.items():
            score(f"grid|{label}|{grid_name}", rc.select(cal, eps, DELTA, grid=grid, modules=modules, mode="min")[0])
        for delta in ABLATION_DELTAS:
            score(f"delta|{label}|{delta}", rc.select(cal, eps, delta, modules=modules, mode="min")[0])
    return res


def exp_ablation(trials):
    out = {}
    for name in ["clinc", "massive"]:
        res = collect([(name, DEFAULT_N[name], 0.05, 50_000 + s) for s in range(trials)], _ablation_trial)
        out[name] = {key: summarise([r[key] for r in res], 0.05) for key in res[0]}
        print(name, "ablation done", flush=True)
    return out


# -- description of the language-model workflows ----------------------------


def exp_llmstats(_trials=None):
    """Population facts about the language-model workflows (no sampling involved)."""
    out = {}
    top = LLM_TAIL[-1]
    for name in llm_workflows():
        d = np.load(table_path(name))
        pop, _ = population(name)
        k = pop.n_nodes
        conf = pop.path_conf[pop.path_node >= 0]
        on = d["truth"][:, 0] >= 0
        root_ok = d["pred"][on, 0] == d["truth"][on, 0]
        root_first = pop.deviated & (pop.dev_node == 0)
        shared = lambda t: dict(risk=pop.risk(np.full(k, t)), review=pop.review(np.full(k, t)))
        out[name] = dict(N=len(pop), K=k, model=str(d["model"]), upgrade=str(d["upgrade"]),
                         free_run_error=pop.risk(np.zeros(k)), root_accuracy=float(root_ok.mean()),
                         saturated_share=float((conf >= top).mean()),
                         root_wrong_saturated=float((root_first & (pop.dev_conf >= top)).mean()),
                         at_default=shared(0.7), at_top=shared(top),
                         conf_when_wrong=float(pop.dev_conf[pop.deviated].mean()))
    # Number of requests answered by each model.
    files = {("clinc_llm", "model"): "{model}", ("clinc_llm", "upgrade"): "{upgrade}",
             ("clinc_llm_fs", "model"): "{model}_fs", ("clinc_llm_claude", "model"): "{model}"}
    for (name, key), pattern in files.items():
        if name not in out:
            continue
        slug = pattern.format(**out[name]).replace("/", "_")
        raw = os.path.join(CACHE, f"llm_raw_{slug}.jsonl")
        if os.path.exists(raw):
            rows = [json.loads(line) for line in open(raw)]
            out[name][f"{key}_requests"] = len(rows)
            out[name][f"{key}_unparsed"] = sum(1 for r in rows if max(r["probs"]) == 0)
    return out


def agent_stats(name):
    """Population facts about an agent (no sampling involved)."""
    d = np.load(table_path(name))
    pop, _ = population(name)
    k = pop.n_nodes
    on = pop.path_node >= 0
    top = LLM_TAIL[-1]
    shared = lambda t: dict(risk=pop.risk(np.full(k, t)), review=pop.review(np.full(k, t)))
    routes = {tuple(r[r >= 0]) for r in pop.path_node}
    marked = lambda u: float((pop.deviated & (pop.dev_node == u)).mean())
    # What a failed run does after its first wrong decision.
    pos = np.arange(pop.path_node.shape[1])[None, :]
    fail = pop.deviated[:, None]
    after = on & fail & (pos > pop.dev_pos[:, None])
    sound = on & ~(fail & (pos >= pop.dev_pos[:, None]))
    least = np.where(on, pop.path_conf, np.inf).argmin(axis=1)
    confident = lambda mask: float((pop.path_conf[mask] >= 0.95).mean())
    return dict(model=str(d["model"]), nodes=[str(x) for x in d["nodes"]], budget=int(d["budget"]),
                skipped=int(d["skipped"]), edits=json.loads(str(d["edits"])),
                decisions_mean=float(on.sum(axis=1).mean()), decisions_max=int(on.sum(axis=1).max()),
                routes=len(routes), saturated_share=float((pop.path_conf[on] >= top).mean()),
                visit_share=[float((pop.path_node == v).any(axis=1).mean()) for v in range(k)],
                first_error_share=[marked(v) for v in range(k)],
                at_default=shared(0.7), at_top=shared(top),
                goes_on=float(after.any(axis=1)[pop.deviated].mean()),
                least_confident_later=float((least > pop.dev_pos)[pop.deviated].mean()),
                confident_sound=confident(sound), confident_after=confident(after))


def agent_requests():
    """Distinct prompts answered by each model for the agents (written by kg_agent.py)."""
    path = os.path.join(DATA, "kg_requests.json")
    return json.load(open(path)) if os.path.exists(path) else {}


# -- synthetic scaling ----------------------------------------------------


def synth_population(k, n, rng, base, temp=1.5, rho=0.6, rescue=0.5):
    """Chain of k decisions. A shared latent difficulty correlates the nodes, temp > 1 makes
    them overconfident, and after a wrong turn later nodes see an off-route input and become
    less confident (rescue). `base` is the logit of the typical per-node accuracy."""
    z = rng.standard_normal((n, 1))
    e = rng.standard_normal((n, k))
    logit = base + 1.5 * (rho * z + np.sqrt(1 - rho ** 2) * e)
    q = 1 / (1 + np.exp(-logit))
    q = np.maximum(q, 1 - q) * 0.999
    correct = rng.random((n, k)) < q
    conf = 1 / (1 + np.exp(-temp * np.log(q / (1 - q))))
    wrong_any = ~correct
    dev_pos = np.where(wrong_any.any(1), wrong_any.argmax(1), -1)
    after = (np.arange(k)[None, :] > dev_pos[:, None]) & (dev_pos[:, None] >= 0)
    conf = np.where(after, 0.5 + (conf - 0.5) * rescue * rng.random((n, k)) * 2, conf).clip(0.5, 1.0)
    node = np.tile(np.arange(k), (n, 1))
    return rc.Traces(node, conf, dev_pos.astype(np.int32), k)


def synth_base(k, target=0.2):
    """Per-node accuracy logit for which the chain's error rate without abstention is `target`."""
    lo, hi = 0.0, 12.0
    for _ in range(30):
        mid = (lo + hi) / 2
        err = synth_population(k, 100_000, np.random.default_rng(7), mid).deviated.mean()
        lo, hi = (mid, hi) if err > target else (lo, mid)
    return (lo + hi) / 2


_SYNTH = {}


def _synth_trial(args):
    k, n, eps, seed = args
    if k not in _SYNTH:
        _SYNTH[k] = synth_population(k, 400_000, np.random.default_rng(1234 + k), synth_base(k))
    pop = _SYNTH[k]
    rng = np.random.default_rng(seed)
    cal = pop.subset(rng.integers(0, len(pop), n))
    methods = ["global-e2e", "e2e-pareto", "fd-node", "min-node", "min-global"]
    return {m: (pop.risk(tau), pop.review(tau), b, pop.charged_mass(tau, "fd"))
            for m, (tau, b) in run_methods(cal, eps, rng, [list(range(k))], methods).items()}


def exp_synth(trials):
    out = {}
    eps = 0.05
    for k in [2, 4, 8, 16, 32]:
        out[str(k)] = {}
        for n in [1000, 4000, 16000]:
            res = collect([(k, n, eps, 40_000 + s) for s in range(trials)], _synth_trial)
            out[str(k)][str(n)] = {m: summarise([r[m] for r in res], eps) for m in res[0]}
        print("synth K", k, "done", flush=True)
    return out


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("experiment", choices=["main", "nsweep", "posthoc", "edits", "synth", "llmstats",
                                           "ablation"])
    ap.add_argument("--trials", type=int, default=200)
    ap.add_argument("--only", help="edits: re-run a single edit, e.g. clinc:root, and merge it")
    ap.add_argument("--llm", action="store_true", help="main / edits on the language-model workflow")
    ap.add_argument("--agent", action="store_true", help="main / edits / posthoc on the knowledge-graph agents")
    a = ap.parse_args()
    t0 = time.time()
    os.makedirs(RESULTS, exist_ok=True)
    path = os.path.join(RESULTS, f"{a.experiment}{'_llm' if a.llm else '_agent' if a.agent else ''}.json")
    if a.agent:
        rc.set_grid_tail(LLM_TAIL)
        if a.experiment == "main":
            data = exp_main(a.trials, agent_workflows(), AGENT_EPS, AGENT_METHODS)
            for name in data:
                data[name].update(agent_stats(name))
        elif a.experiment == "posthoc":
            data = exp_posthoc(a.trials, agent_workflows(), [0.05, 0.10, 0.15, 0.20, 0.25])
        else:
            data = exp_edits(a.trials, names=agent_workflows())
        result = {"delta": DELTA, "trials": a.trials, "data": data, "grid_tail": list(LLM_TAIL)}
        if a.experiment == "main" and agent_requests():
            result["requests"] = agent_requests()
    elif a.experiment == "llmstats":
        result = {"data": exp_llmstats(), "grid_tail": list(LLM_TAIL)}
    elif a.llm:
        rc.set_grid_tail(LLM_TAIL)
        if a.experiment == "main":
            data = exp_main(a.trials, names=llm_workflows())
        else:
            data = exp_edits(a.trials, names=("clinc_llm_inscope",))
        result = {"delta": DELTA, "trials": a.trials, "data": data, "grid_tail": list(LLM_TAIL)}
    elif a.experiment == "edits" and a.only:
        result = json.load(open(path))
        name, label = a.only.split(":")
        result["data"][name]["edits"][label] = exp_edits(a.trials, a.only)[name]["edits"][label]
    else:
        result = {"main": exp_main, "nsweep": exp_nsweep, "posthoc": exp_posthoc, "edits": exp_edits,
                  "synth": exp_synth, "ablation": exp_ablation}[a.experiment](a.trials)
        result = {"delta": DELTA, "trials": a.trials, "data": result}
    with open(path, "w") as f:
        json.dump(result, f, indent=1)
    print(a.experiment, "finished in", round(time.time() - t0), "s")
