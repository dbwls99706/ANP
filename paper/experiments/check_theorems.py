"""Direct numerical check of the two guarantees.

The experiments score the thresholds a method selects. The theorems say more: the bound
holds for every threshold assignment on the grid at once. This script checks that
statement as it is implemented, on the populations of the paper:

  certificate   (Theorem 4)  U(g) >= R(tau_g) for all g, in at least 1 - delta of the draws
  recertified   (Theorem 7)  the same for the bound issued after an edit
  two_edits     the same after two successive edits, with the evidence kept as described
                below Theorem 7
  single        the Clopper-Pearson bound of one fixed assignment, without the union over
                the grid, fails in at most delta of the draws and not much less often

R is the exact risk on the population, computed for all G^M assignments at once.

    python check_theorems.py [--draws 1000]      # a few minutes on 4 cores
"""
import argparse
import json
import os
from functools import reduce
from multiprocessing import Pool

import numpy as np

import riskcert as rc
import run_experiments as rx

DELTA = rx.DELTA
CLASSIC_EDITS = {"clinc": {"leaf": 3, "middle": 1, "root": 0}, "massive": {"leaf": 7, "middle": 2}}


def commit_index(tr, modules, grid):
    """a[i, m]: the largest grid index at which every node of module m on run i commits."""
    module_of = np.empty(tr.n_nodes, dtype=int)
    for m, nodes in enumerate(modules):
        module_of[list(nodes)] = m
    lowest = np.full((len(tr), len(modules)), np.inf)
    for j in range(tr.path_node.shape[1]):
        rows = np.where(tr.path_node[:, j] >= 0)[0]
        np.minimum.at(lowest, (rows, module_of[tr.path_node[rows, j]]), tr.path_conf[rows, j])
    return np.searchsorted(grid, lowest, side="right") - 1


def all_risks(tr, modules, grid):
    """Exact risk of every threshold assignment: R[g_1, ..., g_M]."""
    a = commit_index(tr, modules, grid)[tr.deviated]
    hist = np.zeros((len(grid),) * len(modules))
    np.add.at(hist, tuple(a.T), 1)
    for axis in range(len(modules)):   # a run completes at g when a >= g in every module
        hist = np.flip(np.cumsum(np.flip(hist, axis), axis), axis)
    return hist / len(tr)


def totals(counts):
    """counts[m, g] -> sum over modules for every assignment."""
    m = len(counts)
    return reduce(np.add, [counts[i].reshape([-1 if j == i else 1 for j in range(m)]) for i in range(m)])


def all_bounds(cert):
    """The certified bound of every assignment, as Certificate.bound computes it one by one."""
    g = len(cert.grid)
    out = 0.0
    for part in cert.parts:
        tot = totals(part.counts)
        out = out + part.upper(tot.ravel(), g).reshape(tot.shape)
    out[(g - 1,) * len(cert.modules)] = 0.0
    return out


def some_commit(g, m):
    """Mask of the assignments in which at least one module commits (the others have bound 0)."""
    mask = np.ones((g,) * m, dtype=bool)
    mask[(g - 1,) * m] = False
    return mask


def same_as_library(cert, bounds, rng, tries=25):
    for _ in range(tries):
        choice = [int(x) for x in rng.integers(0, len(cert.grid), len(cert.modules))]
        assert abs(cert.bound(choice) - bounds[tuple(choice)]) < 1e-12
    return True


def summary(fails, slack, extra=None):
    out = dict(draws=len(fails), violations=int(np.sum(fails)), violation_rate=float(np.mean(fails)),
               smallest_slack=float(np.min(slack)))
    out.update(extra or {})
    return out


def check_certificate(name, modules, mode, draws, grid):
    pop, _ = rx.population(name)
    risk = all_risks(pop, modules, grid)
    # The exact risk must agree with the direct evaluation of a threshold vector.
    rng = np.random.default_rng(0)
    for _ in range(25):
        choice = [int(x) for x in rng.integers(0, len(grid), len(modules))]
        tau = np.zeros(pop.n_nodes)
        for m, g in enumerate(choice):
            tau[list(modules[m])] = grid[g]
        assert abs(pop.risk(tau) - risk[tuple(choice)]) < 1e-12
    k_pop = rc.error_counts(pop, grid, mode)
    mass = totals(np.stack([k_pop[list(m)].sum(0) for m in modules])) / len(pop)   # B_u(tau_g)
    assert (mass >= risk - 1e-12).all()   # the charging lemma, on the population
    n = rx.DEFAULT_N[name]
    commits = some_commit(len(grid), len(modules))
    fails, fails_mass, slack = [], [], []
    for s in range(draws):
        rng = np.random.default_rng(50_000 + s)
        cal = pop.subset(rng.integers(0, len(pop), n))
        cert = rc.certify(cal, DELTA, grid, modules, mode=mode)
        bounds = all_bounds(cert)
        if s == 0:
            same_as_library(cert, bounds, rng)
        fails.append(bool((risk > bounds + 1e-12).any()))
        never = (len(grid) - 1,) * len(modules)
        gap = bounds - mass
        gap[never] = np.inf   # nothing is charged when no node commits
        fails_mass.append(bool((gap < -1e-12).any()))
        slack.append(float((bounds - risk)[commits].min()))
    return summary(fails, slack, dict(assignments=int(risk.size), modules=len(modules), n=n,
                                      mass_violation_rate=float(np.mean(fails_mass))))


def check_recertified(name, edit, modules, m_new, draws, grid):
    w = edit[0] if isinstance(edit, tuple) else edit
    pop0, _ = rx.population(name)
    pop1, _ = rx.population(name, (edit,))
    risk = all_risks(pop1, modules, grid)
    hit = np.where(pop1.through([w]))[0]
    reach = np.unique(pop1.path_node[hit])
    reach = reach[reach >= 0]
    n = rx.DEFAULT_N[name]
    commits = some_commit(len(grid), len(modules))
    fails, slack = [], []
    for s in range(draws):
        rng = np.random.default_rng(60_000 + s)
        old = pop0.subset(rng.integers(0, len(pop0), n))
        fresh = pop1.subset(hit[rng.integers(0, len(hit), m_new)])
        cert, _ = rc.certify_after_edit(old, fresh, [w], DELTA, grid, modules, reach=reach)
        bounds = all_bounds(cert)
        if s == 0:
            same_as_library(cert, bounds, rng)
        fails.append(bool((risk > bounds + 1e-12).any()))
        slack.append(float((bounds - risk)[commits].min()))
    return summary(fails, slack, dict(assignments=int(risk.size), modules=len(modules), n=n, m=m_new,
                                      traffic_share=float(len(hit) / len(pop1))))


def check_two_edits(name, first, second, modules, m_new, draws, grid):
    """The bound after two successive edits (the iteration described below Theorem 7).

    Runs fall into three groups: those that avoid both edited nodes (old sample), those
    that meet the first edit and then avoid the second (fresh sample of the first repair,
    restricted to its routes that avoid the second node), and those that meet the second
    edit (fresh sample of the second repair). The share of the last group is bounded from
    the old sample by the runs whose recorded route meets either node."""
    pop0, _ = rx.population(name)
    pop1, _ = rx.population(name, (first,))
    pop2, _ = rx.population(name, (first, second))
    risk = all_risks(pop2, modules, grid)
    hit1 = np.where(pop1.through([first]))[0]
    hit2 = np.where(pop2.through([second]))[0]
    assert np.array_equal(pop1.through([second]), pop2.through([second]))
    stack = lambda k: np.stack([k[list(m)].sum(0) for m in modules])
    touched = lambda pop, hit: max(1, sum(1 for m in modules if set(np.unique(pop.path_node[hit])) & set(m)))
    n, parts_delta = rx.DEFAULT_N[name], DELTA / 5
    commits = some_commit(len(grid), len(modules))
    fails, slack = [], []
    for s in range(draws):
        rng = np.random.default_rng(80_000 + s)
        old = pop0.subset(rng.integers(0, len(pop0), n))
        fresh1 = pop1.subset(hit1[rng.integers(0, len(hit1), m_new)])
        fresh2 = pop2.subset(hit2[rng.integers(0, len(hit2), m_new)])
        met1, met2 = old.through([first]), old.through([second])
        share1 = float(rc.cp_upper(int(met1.sum()), n, parts_delta)[0])
        share2 = float(rc.cp_upper(int((met1 | met2).sum()), n, parts_delta)[0])
        parts = [rc.Part(stack(rc.error_counts(old, grid, "min", mask=~(met1 | met2))), n, parts_delta, len(modules)),
                 rc.Part(stack(rc.error_counts(fresh1, grid, "min", mask=~fresh1.through([second]))), m_new,
                         parts_delta, touched(pop1, hit1), scale=share1),
                 rc.Part(stack(rc.error_counts(fresh2, grid, "min")), m_new, parts_delta, touched(pop2, hit2),
                         scale=share2)]
        cert = rc.Certificate(grid, modules, None, parts)
        bounds = all_bounds(cert)
        if s == 0:
            same_as_library(cert, bounds, rng)
        fails.append(bool((risk > bounds + 1e-12).any()))
        slack.append(float((bounds - risk)[commits].min()))
    return summary(fails, slack, dict(assignments=int(risk.size), modules=len(modules), n=n, m=m_new,
                                      share_first=float(len(hit1) / len(pop1)),
                                      share_second=float(len(hit2) / len(pop2))))


def check_single(name, draws, grid):
    """One fixed shared threshold at a time, at level delta without a union bound."""
    pop, _ = rx.population(name)
    everything = [list(range(pop.n_nodes))]
    mass = rc.error_counts(pop, grid, "min").sum(0) / len(pop)   # = the exact risk (Proposition 3)
    n = rx.DEFAULT_N[name]
    fail = np.zeros(len(grid))
    for s in range(draws):
        rng = np.random.default_rng(70_000 + s)
        cal = pop.subset(rng.integers(0, len(pop), n))
        k = rc.error_counts(cal, grid, "min").sum(0)
        fail += mass > rc.cp_upper(k, n, DELTA) + 1e-12
    rate = fail[:-1] / draws   # the last grid value never commits
    assert np.allclose(mass, [pop.risk(np.full(pop.n_nodes, g)) for g in grid])
    return dict(draws=draws, largest_violation_rate=float(rate.max()), mean_violation_rate=float(rate.mean()),
                modules=len(everything))


def job(args):
    kind, name, draws, rest = args
    agent = name.startswith("kg_")
    rc.set_grid_tail(rx.LLM_TAIL if agent else ())
    grid = rc.default_grid()
    pop, _ = rx.population(name)
    modules = rx.groups(name, pop)
    if kind == "certificate":
        return kind, f"{name}|{rest}", check_certificate(name, modules, rest, draws, grid)
    if kind == "single":
        return kind, name, check_single(name, 5 * draws, grid)
    if kind == "two_edits":
        first, second = rest
        return kind, f"{name}|{first}|{second}", check_two_edits(name, first, second, modules, 500, draws, grid)
    label, edit, m_new = rest
    return kind, f"{name}|{label}|{m_new}", check_recertified(name, edit, modules, m_new, draws, grid)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--draws", type=int, default=1000)
    a = ap.parse_args()
    jobs = []
    for name in ["clinc", "massive"] + rx.agent_workflows():
        jobs += [("certificate", name, a.draws, mode) for mode in ("min", "fd")]
        jobs.append(("single", name, a.draws, None))
        if name.startswith("kg_"):
            made = json.loads(str(np.load(rx.table_path(name))["edits"]))
            edits = {label: (e["node"], label) for label, e in made.items()}
        else:
            edits = CLASSIC_EDITS[name]
        jobs += [("recertified", name, a.draws, (label, edit, m_new))
                 for label, edit in edits.items() for m_new in (100, 1000)]
    # Two successive edits: a leaf then the node above it, the reverse, and two leaves.
    jobs += [("two_edits", "clinc", a.draws, pair) for pair in [(3, 1), (1, 3), (3, 4)]]
    jobs += [("two_edits", "massive", a.draws, pair) for pair in [(7, 2), (2, 7)]]
    out = {"delta": DELTA, "draws": a.draws, "certificate": {}, "recertified": {}, "single": {}, "two_edits": {}}
    with Pool(4) as pool:
        for kind, key, result in pool.imap(job, jobs, chunksize=1):
            out[kind][key] = result
            print(kind, key, result, flush=True)
    with open(os.path.join(rx.RESULTS, "coverage.json"), "w") as f:
        json.dump(out, f, indent=1)


if __name__ == "__main__":
    main()
