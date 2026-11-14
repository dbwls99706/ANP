"""The deterministic statements of the paper, checked on random workflows.

Lemma 2 and Propositions 3, 6 and 8 are statements about sets of runs, with no
probability in them once the population is fixed. This script draws small random
workflows (DAGs with joins, two to four choices per node, confidences with many ties)
and checks each statement exactly on every one of them:

  charging     R(tau) <= B_u(tau), with the stated gap, for first-error and
               least-confident charging with and without an anchor          (Lemma 2)
  exactness    B_u(a + s 1) = R(a + s 1) for least-confident charging       (Proposition 3)
  monotone     raising any threshold cannot raise the risk (what makes rounding a
               configured threshold down to the grid safe)
  locality     an edit leaves the runs that avoid the edited node unchanged, and the
               risk of the edited workflow splits as stated                 (Proposition 6)
  footprints   the count curves an edit can change, per charging rule       (Proposition 8)
  outcome      charging and exactness when a run is bad by an arbitrary outcome label
               instead of a wrong route                                     (Section 2)

It also counts the workflows on which three stronger statements fail (first-error charging
changes only the edited node; least-confident charging changes only the node and its
descendants; first-error charging is exact for a shared threshold), to show that the
checks can fail.

    python test_claims.py [--workflows 2000]

A failed check stops the script with the offending workflow's seed.
"""
import argparse
import json
import os

import numpy as np

import riskcert as rc

RESULTS = os.path.join(os.path.dirname(__file__), "..", "results")
LEVELS = np.array([0.2, 0.4, 0.6, 0.8, 1.0])   # a coarse confidence scale produces ties


def random_workflow(rng):
    """Tables (pred, conf, truth, nxt) of a random routing DAG, and an alternative model
    for every node."""
    k = int(rng.integers(2, 9))
    width = int(rng.integers(2, 5))
    n = int(rng.integers(60, 400))
    nxt = -np.ones((k, width), dtype=int)
    for v in range(k - 1):
        for c in range(width):
            if rng.random() < 0.8:
                nxt[v, c] = int(rng.integers(v + 1, k))   # any later node: joins are allowed
    truth = -np.ones((n, k), dtype=int)
    for i in range(n):
        v = 0
        while v >= 0:
            truth[i, v] = int(rng.integers(width))
            v = nxt[v, truth[i, v]]

    def model():
        accuracy = rng.uniform(0.5, 0.95, size=k)
        pred = rng.integers(0, width, size=(n, k))
        right = (rng.random((n, k)) < accuracy) & (truth >= 0)
        pred[right] = truth[right]
        conf = LEVELS[rng.integers(0, len(LEVELS), size=(n, k))] if rng.random() < 0.5 else rng.random((n, k))
        return pred, conf

    (pred, conf), (pred_alt, conf_alt) = model(), model()
    return pred, conf, truth, nxt, pred_alt, conf_alt


def reachable(nxt, start, forward=True):
    k = len(nxt)
    edges = [(v, int(u)) for v in range(k) for u in nxt[v] if u >= 0]
    seen, stack = set(), [start]
    while stack:
        x = stack.pop()
        for a, b in edges:
            y = b if (forward and a == x) else a if (not forward and b == x) else None
            if y is not None and y not in seen:
                seen.add(y)
                stack.append(y)
    return seen


def random_tau(rng, k, grid):
    return grid[rng.integers(0, len(grid), size=k)]


def check_charging(tr, rng, grid, count):
    k = tr.n_nodes
    for rule, anchor in [("fd", None), ("min", None), ("min", rng.random(k))]:
        node, conf = rc.charge(tr, rule, anchor)
        # A charging rule assigns every bad run, and only those, to a node on its route.
        on_route = (node >= 0) & (tr.path_node == node[:, None]).any(axis=1)   # -1 is padding
        assert (on_route == tr.deviated).all()
        for _ in range(6):
            tau = random_tau(rng, k, grid)
            charged = tr.deviated & (conf >= tau[np.maximum(node, 0)])
            completes = tr.completes(tau)
            assert (~(tr.deviated & completes) | charged).all()                      # the inclusion
            gap = (charged & ~completes).mean()
            assert abs(tr.charged_mass(tau, rule, anchor) - tr.risk(tau) - gap) < 1e-12   # the gap
            count[0] += 2
        if rule == "min":   # exact along the ray a + s 1
            a = np.zeros(k) if anchor is None else anchor
            for s in rng.uniform(-0.5, 1.2, size=6):
                assert abs(tr.charged_mass(a + s, "min", anchor) - tr.risk(a + s)) < 1e-12
                count[0] += 1


def check_monotone(tr, rng, grid, count):
    k = tr.n_nodes
    for _ in range(6):
        tau = random_tau(rng, k, grid)
        up = tau.copy()
        v = int(rng.integers(k))
        up[v] = max(up[v], grid[rng.integers(0, len(grid))])
        assert tr.risk(up) <= tr.risk(tau) + 1e-12
        # Between two grid values a threshold behaves no worse than the lower one.
        finite = np.where(np.isfinite(tau), np.minimum(tau + rng.uniform(0, 0.04, size=k), 1.0), tau)
        assert tr.risk(finite) <= tr.risk(tau) + 1e-12
        count[0] += 2


def check_edit(tables, rng, grid, count):
    pred, conf, truth, nxt, pred_alt, conf_alt = tables
    k = pred.shape[1]
    old = rc.traces_from_tables(pred, conf, truth, nxt)
    w = int(rng.integers(k))
    pred2, conf2 = pred.copy(), conf.copy()
    pred2[:, w], conf2[:, w] = pred_alt[:, w], conf_alt[:, w]
    new = rc.traces_from_tables(pred2, conf2, truth, nxt)

    # Proposition 6 (i): the edit does not change which runs reach the edited node.
    hit = old.through([w])
    assert (hit == new.through([w])).all()
    # (ii): runs that avoid it are the same runs. Routes may differ in padding width.
    width = max(old.path_node.shape[1], new.path_node.shape[1])
    pad = lambda a, fill: np.pad(a, ((0, 0), (0, width - a.shape[1])), constant_values=fill)
    same = lambda a, b, fill: (pad(a, fill)[~hit] == pad(b, fill)[~hit]).all()
    assert same(old.path_node, new.path_node, -1) and same(old.path_conf, new.path_conf, 1.0)
    assert (old.dev_pos[~hit] == new.dev_pos[~hit]).all()
    count[0] += 3
    # The identity for the risk of the edited workflow.
    for _ in range(4):
        tau = random_tau(rng, k, grid)
        untouched = (old.deviated & old.completes(tau) & ~hit).mean()
        through = (new.deviated & new.completes(tau))[hit].mean() if hit.any() else 0.0
        assert abs(new.risk(tau) - (untouched + hit.mean() * through)) < 1e-12
        count[0] += 1

    # Proposition 8: which count curves can change.
    after = {w} | reachable(nxt, w, forward=True)
    related = after | reachable(nxt, w, forward=False)
    changed = lambda mode: {v for v in range(k) if not np.array_equal(
        rc.error_counts(old, grid, mode)[v], rc.error_counts(new, grid, mode)[v])}
    assert changed("tf") <= {w}
    assert changed("fd") <= after
    assert changed("min") <= related
    count[0] += 3
    # Stronger statements than the proposition makes are false, and these workflows show it.
    shared = np.full(k, rng.uniform(0.2, 0.9))
    return dict(fe_beyond_w=not changed("fd") <= {w}, lc_beyond_descendants=not changed("min") <= after,
                fe_inexact=abs(old.charged_mass(shared, "fd") - old.risk(shared)) > 1e-12)


def check_outcome(tr, rng, grid, count):
    """A run is bad by an arbitrary label on the run, not by its route."""
    bad = rng.random(len(tr)) < rng.uniform(0.05, 0.6)
    length = (tr.path_node >= 0).sum(axis=1)
    dev_pos = np.where(bad, rng.integers(0, 10 ** 6, size=len(tr)) % np.maximum(length, 1), -1)
    labelled = rc.Traces(tr.path_node, tr.path_conf, dev_pos.astype(np.int32), tr.n_nodes)
    check_charging(labelled, rng, grid, count)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--workflows", type=int, default=2000)
    a = ap.parse_args()
    grid = rc.default_grid()
    count = [0]
    joins = ties = 0
    refuted = dict(fe_beyond_w=0, lc_beyond_descendants=0, fe_inexact=0)
    for seed in range(a.workflows):
        rng = np.random.default_rng(seed)
        tables = random_workflow(rng)
        pred, conf, truth, nxt = tables[:4]
        tr = rc.traces_from_tables(pred, conf, truth, nxt)
        try:
            check_charging(tr, rng, grid, count)
            check_monotone(tr, rng, grid, count)
            foot = check_edit(tables, rng, grid, count)
            check_outcome(tr, rng, grid, count)
        except AssertionError:
            print("FAILED on workflow seed", seed)
            raise
        parents = [len({v for v in range(len(nxt)) if u in nxt[v]}) for u in range(len(nxt))]
        joins += max(parents) > 1
        ties += bool(len(np.unique(conf)) <= len(LEVELS))
        for key in refuted:
            refuted[key] += bool(foot[key])
    out = dict(workflows=a.workflows, checks=count[0], with_joins=int(joins), with_tied_confidences=int(ties),
               failures=0, false_variants_refuted=refuted)
    os.makedirs(RESULTS, exist_ok=True)
    with open(os.path.join(RESULTS, "claims.json"), "w") as f:
        json.dump(out, f, indent=1)
    print(out)


if __name__ == "__main__":
    main()
