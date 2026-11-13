"""Separable risk certificates for decision workflows.

A workflow is a DAG of decision nodes. Node v commits to its top choice when
its confidence is at least tau_v and escalates the run to a human otherwise.
A run is an *error* when it completes without escalation on a route that
differs from the ground-truth (oracle) route.

Everything here works on free-run traces: the route an input takes when every
node commits (all thresholds zero). That route does not depend on the
thresholds, so one pass over the calibration data is enough to evaluate any
threshold vector.
"""
from dataclasses import dataclass, field

import numpy as np
from scipy.stats import beta, binom

INF = np.inf


def cp_upper(k, n, alpha):
    """One-sided Clopper-Pearson upper confidence bound for a binomial proportion.

    Returns U with P(p <= U) >= 1 - alpha for k successes out of n.
    """
    k = np.atleast_1d(np.asarray(k, dtype=float))
    out = np.ones_like(k)
    ok = k < n
    out[ok] = beta.isf(alpha, k[ok] + 1, n - k[ok])  # isf stays accurate for tiny alpha
    return out


@dataclass
class Traces:
    """Free-run traces of N inputs through a workflow with K decision nodes.

    path_node[i, j]  j-th decision node visited by input i (-1 = padding)
    path_conf[i, j]  its confidence
    dev_pos[i]       position of the first wrong decision, -1 if the free run is correct
    tf_on[i, v]      v lies on the oracle route of input i          (optional)
    tf_wrong[i, v]   v is wrong on its oracle-route input           (optional)
    tf_conf[i, v]    confidence of v on its oracle-route input      (optional)
    """
    path_node: np.ndarray
    path_conf: np.ndarray
    dev_pos: np.ndarray
    n_nodes: int
    tf_on: np.ndarray = None
    tf_wrong: np.ndarray = None
    tf_conf: np.ndarray = None

    def __len__(self):
        return len(self.dev_pos)

    @property
    def deviated(self):
        return self.dev_pos >= 0

    @property
    def dev_node(self):
        rows = np.arange(len(self))
        return np.where(self.deviated, self.path_node[rows, np.maximum(self.dev_pos, 0)], -1)

    @property
    def dev_conf(self):
        rows = np.arange(len(self))
        return np.where(self.deviated, self.path_conf[rows, np.maximum(self.dev_pos, 0)], np.nan)

    def subset(self, idx):
        opt = lambda a: None if a is None else a[idx]
        return Traces(self.path_node[idx], self.path_conf[idx], self.dev_pos[idx], self.n_nodes,
                      opt(self.tf_on), opt(self.tf_wrong), opt(self.tf_conf))

    def through(self, nodes):
        """Runs whose free route visits any of the given nodes."""
        return np.isin(self.path_node, list(nodes)).any(axis=1)

    # -- evaluation of a threshold vector ---------------------------------

    def completes(self, tau):
        tau = np.asarray(tau, dtype=float)
        pad = self.path_node < 0
        thr = tau[np.maximum(self.path_node, 0)]
        return (pad | (self.path_conf >= thr)).all(axis=1)

    def risk(self, tau):
        """Fraction of inputs that complete on a wrong route."""
        return float((self.completes(tau) & self.deviated).mean())

    def review(self, tau):
        """Fraction of inputs escalated to a human."""
        return float(1.0 - self.completes(tau).mean())

    def charged_mass(self, tau, rule="fd", anchor=None):
        """Empirical value of the separable bound: erroneous runs whose charged node commits."""
        tau = np.asarray(tau, dtype=float)
        node, c = charge(self, rule, anchor)
        return float((self.deviated & (c >= tau[np.maximum(node, 0)])).mean())


def traces_from_tables(pred, conf, truth, nxt, root=0):
    """Build free-run traces from per-node prediction tables (see prep_data.py)."""
    n, k = pred.shape
    path_node = -np.ones((n, k), dtype=np.int32)
    path_conf = np.ones((n, k), dtype=np.float64)
    dev_pos = -np.ones(n, dtype=np.int32)
    cur = np.full(n, root, dtype=np.int64)
    alive = np.ones(n, dtype=bool)
    rows = np.arange(n)
    depth = 0
    for j in range(k):
        if not alive.any():
            break
        depth = j + 1
        r = rows[alive]
        v = cur[alive]
        choice = pred[r, v]
        path_node[r, j] = v
        path_conf[r, j] = conf[r, v]
        wrong = (truth[r, v] != choice) & (dev_pos[r] < 0)
        dev_pos[r[wrong]] = j
        step = nxt[v, choice]
        cur[r] = step
        alive[r] = step >= 0
    on = truth >= 0
    return Traces(path_node[:, :depth], path_conf[:, :depth], dev_pos, k,
                  tf_on=on, tf_wrong=on & (pred != truth), tf_conf=conf.astype(np.float64))


# -- charging rules -------------------------------------------------------
#
# A charging rule assigns every erroneous free run to one node on its route.
# The assignment must not depend on the thresholds. For any such rule,
#     risk(tau) <= sum_v P(run is erroneous, charged to v, conf_v >= tau_v),
# because a run that completes has every node on its route above threshold.


def charge(tr, rule="fd", anchor=None):
    """Return (node, conf) of the node each erroneous run is charged to (-1 / nan otherwise).

    rule="fd":   the first wrong decision. Depends only on the node and its ancestors.
    rule="min":  the least confident node on the route (anchor=None), or the node with the
                 smallest margin conf_v - anchor_v. Exact for thresholds anchor + const.
    """
    if rule == "fd":
        return tr.dev_node, tr.dev_conf
    if rule == "min":
        ref = np.zeros(tr.n_nodes) if anchor is None else np.minimum(np.asarray(anchor, dtype=float), 2.0)
        margin = tr.path_conf - ref[np.maximum(tr.path_node, 0)]
        margin[tr.path_node < 0] = np.inf
        pos = margin.argmin(axis=1)
        rows = np.arange(len(tr))
        dev = tr.deviated
        return (np.where(dev, tr.path_node[rows, pos], -1), np.where(dev, tr.path_conf[rows, pos], np.nan))
    raise ValueError(rule)


def default_grid():
    """Threshold grid: steps of 0.05 up to 0.95, a finer tail where confident models
    concentrate, and +inf (never commit)."""
    return np.r_[np.arange(20) * 0.05, 0.97, 0.98, 0.99, 0.995, 0.999, INF]


def error_counts(tr, grid, mode="fd", anchor=None, mask=None):
    """k[v, g]: calibration inputs charged to node v that node v commits at threshold grid[g].

    mode="fd" / "min": one charge per erroneous run (see charge()).
    mode="tf": every decision that is wrong on its oracle-route input is charged (needs
               teacher-forced evaluation; the count does not depend on any other node's model).
    mask restricts the count to a subset of the runs.
    """
    k = np.zeros((tr.n_nodes, len(grid)), dtype=np.int64)
    keep = np.ones(len(tr), dtype=bool) if mask is None else mask
    if mode == "tf":
        for v in range(tr.n_nodes):
            cv = np.sort(tr.tf_conf[tr.tf_wrong[:, v] & keep, v])
            k[v] = len(cv) - np.searchsorted(cv, grid, side="left")
        return k
    node, c = charge(tr, mode, anchor)
    for v in range(tr.n_nodes):
        cv = np.sort(c[(node == v) & keep])
        k[v] = len(cv) - np.searchsorted(cv, grid, side="left")
    return k


def review_counts(tr, grid, mask=None):
    """h[v, g]: calibration inputs whose free run visits v with confidence below grid[g]."""
    h = np.zeros((tr.n_nodes, len(grid)), dtype=np.int64)
    rows = slice(None) if mask is None else mask
    node, conf = tr.path_node[rows], tr.path_conf[rows]
    for v in range(tr.n_nodes):
        cv = np.sort(conf[node == v])
        h[v] = np.searchsorted(cv, grid, side="left")
    return h


# -- certificates ---------------------------------------------------------


@dataclass
class Part:
    """Count curves from one calibration sample.

    Every run of the sample is charged to at most one module, so for a fixed
    threshold assignment the charges add up to one binomial count out of n.
    The bound of the part is scale * CP(count, n, delta / |grid| ** exponent):
    the union bound ranges over the assignments of `exponent` modules.
    """
    counts: np.ndarray      # [M, G]
    n: int
    delta: float
    exponent: int
    scale: float = 1.0
    _table: np.ndarray = field(default=None, repr=False)

    def upper(self, total, grid_size, plug_in=False):
        total = np.atleast_1d(np.asarray(total, dtype=np.int64))
        if plug_in:
            return self.scale * total / self.n
        if self._table is None or total.max() >= len(self._table):
            size = max(int(total.max()) + 1, int(self.counts.max(axis=1).sum()) + 1)
            self._table = cp_upper(np.arange(size), self.n, self.delta / float(grid_size) ** self.exponent)
        return self.scale * self._table[total]


@dataclass
class Certificate:
    """With probability at least 1 - sum of the parts' delta over the calibration samples,
    simultaneously for every threshold assignment on the grid,
        risk(tau) <= sum over parts of part.upper(sum_m part.counts[m, g_m]).
    Modules are groups of nodes that share one threshold.
    """
    grid: np.ndarray
    modules: list
    review: np.ndarray      # [M, G] separable proxy of the review rate (optimisation objective only)
    parts: list
    plug_in: bool = False

    def bound(self, choice):
        idx = np.arange(len(choice))
        if all(g == len(self.grid) - 1 for g in choice):
            return 0.0  # a workflow that never commits cannot complete a wrong run
        return float(sum(p.upper(p.counts[idx, choice].sum(), len(self.grid), self.plug_in)[0]
                         for p in self.parts))

    def thresholds(self, choice, n_nodes):
        tau = np.zeros(n_nodes)
        for m, g in enumerate(choice):
            tau[self.modules[m]] = self.grid[g]
        return tau


def certify(tr, delta, grid=None, modules=None, mode="fd", plug_in=False, anchor=None, pooled=True):
    """Certify a workflow from calibration traces.

    modules: partition of the nodes into groups that share a threshold (default: one per
    node). pooled=False gives every module its own confidence band (required for mode="tf",
    where one run can be charged to several nodes). plug_in=True uses raw frequencies
    instead of confidence bounds (no guarantee; a baseline).
    """
    grid = default_grid() if grid is None else grid
    modules = [[v] for v in range(tr.n_nodes)] if modules is None else modules
    k_node = error_counts(tr, grid, mode, anchor)
    n = len(tr)
    counts = np.stack([k_node[m].sum(0) for m in modules])
    h_node = review_counts(tr, grid)
    review = np.stack([h_node[m].sum(0) for m in modules]) / n
    m_count = len(modules)
    if pooled and mode != "tf":
        parts = [Part(counts, n, delta, m_count)]
    else:
        parts = []
        for m in range(m_count):
            own = np.zeros_like(counts)
            own[m] = counts[m]
            parts.append(Part(own, n, delta / m_count, 1))
    return Certificate(grid, modules, review, parts, plug_in)


def certify_after_edit(old, fresh, edited, delta, grid=None, modules=None, mode="min", anchor=None,
                       reach=None):
    """Recertify after editing the nodes in `edited`, reusing the old calibration sample.

    A run whose free route avoids every edited node behaves identically before and after
    the edit, and whether a route reaches an edited node is decided by unedited nodes. So
        risk = P(error, route avoids the edit) + P(route meets the edit) * P(error | meets),
    where the first term is certified by the old sample (`old`: traces of the old workflow
    on all traffic) and the last by `fresh`: traces of the edited workflow on new labelled
    inputs drawn from the traffic that reaches an edited node. The traffic share needs no
    labels and is bounded from the old sample. `reach` is the set of nodes that can occur on
    a route through the edit (a property of the workflow, not of the sample); the union bound
    of the fresh part ranges over the modules that contain one of them.
    """
    grid = default_grid() if grid is None else grid
    modules = [[v] for v in range(old.n_nodes)] if modules is None else modules
    hit = old.through(edited)
    assert fresh.through(edited).all(), "fresh sample must come from traffic through the edit"
    stack = lambda k: np.stack([k[m].sum(0) for m in modules])
    k_old = stack(error_counts(old, grid, mode, anchor, mask=~hit))
    k_new = stack(error_counts(fresh, grid, mode, anchor))
    share = float(cp_upper(int(hit.sum()), len(old), delta / 3)[0])
    reach = set(range(old.n_nodes)) if reach is None else set(int(v) for v in reach)
    touched = sum(1 for m in modules if reach & set(int(v) for v in m))
    parts = [Part(k_old, len(old), delta / 3, len(modules)),
             Part(k_new, len(fresh), delta / 3, max(touched, 1), scale=share)]
    review = (stack(review_counts(old, grid, mask=~hit)) / len(old)
              + hit.mean() * stack(review_counts(fresh, grid)) / len(fresh))
    cert = Certificate(grid, modules, review, parts)
    rest = old.subset(~hit)
    review_fn = lambda tau: (1 - hit.mean()) * rest.review(tau) + hit.mean() * fresh.review(tau)
    return cert, review_fn


# -- threshold selection --------------------------------------------------


def _exact(cert, eps):
    """Single part: multiple-choice knapsack over integer error counts (exact)."""
    part = cert.parts[0]
    m_count, g_count = part.counts.shape
    never = g_count - 1
    hi = int(part.counts.max(axis=1).sum())
    ok = part.upper(np.arange(hi + 1), len(cert.grid), cert.plug_in) <= eps
    if not ok.any():
        return [never] * m_count
    cmax = int(np.where(ok)[0].max())
    f = np.zeros(cmax + 1)
    back = np.zeros((m_count, cmax + 1), dtype=np.int16)
    for m in range(m_count):
        new = np.full(cmax + 1, np.inf)
        arg = np.full(cmax + 1, never, dtype=np.int16)
        for g in range(g_count):
            c = int(part.counts[m, g])
            if c > cmax:
                continue
            cand = np.full(cmax + 1, np.inf)
            cand[c:] = f[: cmax + 1 - c] + cert.review[m, g]
            better = cand < new
            new[better] = cand[better]
            arg[better] = g
        f, back[m] = new, arg
    choice, c = [never] * m_count, cmax
    for m in range(m_count - 1, -1, -1):
        g = int(back[m, c])
        choice[m] = g
        c -= int(part.counts[m, g])
    return choice


def _greedy(cert, eps):
    """Several parts: start from 'never commit' and repeatedly lower the threshold with the
    best review reduction per unit of bound, while the bound stays within eps."""
    m_count, g_count = cert.review.shape
    choice = [g_count - 1] * m_count
    current = cert.bound(choice)
    while True:
        best, move = 0.0, None
        for m in range(m_count):
            for g in range(choice[m]):
                trial = choice[:m] + [g] + choice[m + 1:]
                b = cert.bound(trial)
                if b > eps:
                    continue
                gain = cert.review[m, choice[m]] - cert.review[m, g]
                score = gain / max(b - current, 1e-12)
                if gain > 0 and score > best:
                    best, move = score, (trial, b)
        if move is None:
            return choice
        choice, current = move


def optimize(cert, eps):
    """Minimise the separable review proxy subject to the certified bound <= eps.
    Always feasible: 'never commit' has bound zero."""
    return _exact(cert, eps) if len(cert.parts) == 1 else _greedy(cert, eps)


def refine(cert, choice, eps, review_fn, n_nodes, sweeps=3):
    """Coordinate descent on an estimate of the true review rate, keeping the bound <= eps.
    Any outcome is valid because the certificate holds uniformly over the grid."""
    choice = list(choice)
    best = review_fn(cert.thresholds(choice, n_nodes))
    for _ in range(sweeps):
        improved = False
        for m in range(len(choice)):
            for g in range(cert.review.shape[1]):
                if g == choice[m]:
                    continue
                trial = choice[:m] + [g] + choice[m + 1:]
                if cert.bound(trial) > eps:
                    continue
                r = review_fn(cert.thresholds(trial, n_nodes))
                if r < best - 1e-12:
                    best, choice, improved = r, trial, True
        if not improved:
            break
    return choice


def pick(cert, eps, review_fn, n_nodes):
    """Thresholds and certified bound for a target risk, from an existing certificate."""
    choice = refine(cert, optimize(cert, eps), eps, review_fn, n_nodes)
    return cert.thresholds(choice, n_nodes), cert.bound(choice)


def select(tr, eps, delta, grid=None, modules=None, mode="fd", plug_in=False, anchor=None, pooled=True):
    """Certify and pick thresholds. Returns (tau, certified bound, certificate)."""
    cert = certify(tr, delta, grid, modules, mode, plug_in, anchor, pooled)
    tau, b = pick(cert, eps, tr.review, tr.n_nodes)
    return tau, b, cert


# -- baselines ------------------------------------------------------------


def per_node_selective(tr, eps, delta, grid=None):
    """Common practice: tune every node on its own so that its error rate among the
    inputs it commits (on its oracle-route inputs) is certified below eps."""
    grid = default_grid() if grid is None else grid
    tau = np.full(tr.n_nodes, INF)
    finite = np.where(np.isfinite(grid))[0]
    for v in range(tr.n_nodes):
        on = tr.tf_on[:, v]
        c, w = tr.tf_conf[on, v], tr.tf_wrong[on, v]
        for g in finite:
            commit = c >= grid[g]
            n_commit = int(commit.sum())
            if n_commit == 0:
                continue
            if cp_upper(int((commit & w).sum()), n_commit, delta / len(finite))[0] <= eps:
                tau[v] = grid[g]
                break
    return tau


def fixed_sequence(tr, candidates, eps, delta):
    """Learn-then-Test with fixed-sequence testing on the exact end-to-end risk.

    candidates must be ordered from most to least conservative. Returns the last
    candidate whose null hypothesis risk > eps is rejected, or None.
    """
    n = len(tr)
    chosen = None
    for tau in candidates:
        k = int(round(tr.risk(tau) * n))
        if binom.cdf(k, n, eps) <= delta:
            chosen = tau
        else:
            break
    return chosen


def global_threshold(tr, eps, delta, grid=None):
    """One shared threshold on the joint score (the minimum confidence along the run),
    calibrated end to end with fixed-sequence testing."""
    grid = default_grid() if grid is None else grid
    cands = [np.full(tr.n_nodes, g) for g in grid[::-1]]
    tau = fixed_sequence(tr, cands, eps, delta)
    return np.full(tr.n_nodes, INF) if tau is None else tau


def e2e_optimize(tr, eps, grid=None, sweeps=4):
    """Heuristic minimiser of the empirical review rate subject to empirical risk <= eps.

    Starts from the best shared threshold and lowers one node at a time. No guarantee by
    itself: it proposes candidates for e2e_pareto and anchors for the "min" charging rule.
    """
    grid = default_grid() if grid is None else grid
    tau = np.full(tr.n_nodes, INF)
    for g in grid:
        if tr.risk(np.full(tr.n_nodes, g)) <= eps:
            tau = np.full(tr.n_nodes, g)
            break
    best = tr.review(tau)
    for _ in range(sweeps):
        improved = False
        for v in range(tr.n_nodes):
            for g in grid:
                if g >= tau[v]:
                    break
                trial = tau.copy()
                trial[v] = g
                if tr.risk(trial) <= eps:
                    r = tr.review(trial)
                    if r < best - 1e-12:
                        best, tau, improved = r, trial, True
                    break
        if not improved:
            break
    return tau


def e2e_pareto(tr, eps, delta, rng, grid=None, n_candidates=24):
    """End-to-end multi-threshold calibration (Pareto-testing style).

    Half of the calibration data proposes a sequence of threshold vectors of
    increasing empirical risk; the other half tests the exact end-to-end risk
    of each with fixed-sequence testing.
    """
    idx = rng.permutation(len(tr))
    a, b = tr.subset(idx[: len(tr) // 2]), tr.subset(idx[len(tr) // 2:])
    cands = [np.full(tr.n_nodes, INF)]
    cands += [e2e_optimize(a, e, grid) for e in np.geomspace(eps / 10, eps * 1.5, n_candidates)]
    cands.sort(key=lambda t: (a.risk(t), -a.review(t)))
    tau = fixed_sequence(b, cands, eps, delta)
    return np.full(tr.n_nodes, INF) if tau is None else tau


def e2e_uniform(tr, eps, delta, grid=None):
    """End-to-end certificate that is uniform over the whole threshold grid: the exact
    end-to-end error count is bounded with a Bonferroni correction over all assignments."""
    grid = default_grid() if grid is None else grid
    n = len(tr)
    alpha = delta / float(len(grid)) ** tr.n_nodes
    ok = cp_upper(np.arange(int(eps * n) + 2), n, alpha) <= eps
    if not ok.any():
        return np.full(tr.n_nodes, INF)
    return e2e_optimize(tr, np.where(ok)[0].max() / n + 1e-12, grid)


def anchored(tr, eps, delta, rng, grid=None, tune_frac=0.3, modules=None):
    """Our certificate with a tuned charging rule: a tuning split proposes reference
    thresholds, the rest certifies with the "min" rule anchored at them."""
    idx = rng.permutation(len(tr))
    cut = int(len(tr) * tune_frac)
    tune, cert_part = tr.subset(idx[:cut]), tr.subset(idx[cut:])
    anchor = e2e_optimize(tune, eps, grid)
    return select(cert_part, eps, delta, grid, modules, "min", anchor=anchor)
