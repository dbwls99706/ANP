//! Risk certificates for decision workflows.
//!
//! A `decision` node commits to its top choice when its confidence reaches the
//! node's threshold and escalates the run to a human otherwise. A run is an
//! *error* when it completes without escalation on a route that differs from
//! the ground-truth route. This module turns labelled calibration traces into
//! a certificate: an upper bound on the error rate that holds, with
//! probability at least `1 - delta`, simultaneously for every threshold
//! assignment on a fixed grid. Thresholds can therefore be tuned, and the
//! target risk changed, after the certificate has been issued.
//!
//! The certificate stores only counts, indexed by route. Editing a node
//! changes the behaviour of the runs that pass through it and of no others,
//! so after an edit the counts of the untouched routes are kept and only the
//! traffic through the edited node has to be labelled again.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::graph::spec::AnpGraph;

/// Thresholds a certificate ranges over. Steps of 0.05 up to 0.95, then a
/// finer tail where confident models concentrate. One more option, "never
/// commit", is always available and is addressed by the index `GRID.len()`.
pub const GRID: [f64; 25] = [
    0.0, 0.05, 0.10, 0.15, 0.20, 0.25, 0.30, 0.35, 0.40, 0.45, 0.50, 0.55, 0.60, 0.65, 0.70, 0.75,
    0.80, 0.85, 0.90, 0.95, 0.97, 0.98, 0.99, 0.995, 0.999,
];

/// Extra thresholds for models whose confidences saturate at 1, such as the
/// token probabilities of a language model.
pub const FINE_TAIL: [f64; 3] = [0.9999, 0.99999, 0.999999];

const DEFAULT_THRESHOLD: f64 = 0.7;

#[derive(Debug, thiserror::Error)]
pub enum RiskError {
    #[error("no calibration traces")]
    NoTraces,
    #[error("trace {index}: {message}")]
    BadTrace { index: usize, message: String },
    #[error("delta must be between 0 and 1")]
    BadDelta,
    #[error("epsilon must be between 0 and 1")]
    BadEpsilon,
    #[error("the graph has no decision nodes")]
    NoDecisions,
    #[error("unknown node '{0}' is not a decision node of the graph")]
    UnknownNode(String),
    #[error("{0}")]
    Other(String),
}

/// One labelled run through the workflow with every node committing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunTrace {
    /// Decisions in the order they were taken.
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    /// Id of the decision node.
    pub node: String,
    /// Probability of the choice the node took.
    pub confidence: f64,
    /// Whether the choice matches the ground truth. Only the first `false`
    /// of a run matters: after it the run is off its correct route.
    #[serde(default = "yes")]
    pub correct: bool,
}

fn yes() -> bool {
    true
}

/// Which node an erroneous run is charged to. Any rule that ignores the
/// thresholds yields a valid bound; the rules differ in tightness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// The least confident node on the route (ties: the first wrong decision if it
    /// is among them). Exact when all thresholds are equal.
    Min,
    /// The first wrong decision. Ignores that later nodes may still escalate.
    FirstError,
}

/// How nodes are grouped into modules that share one threshold. Fewer modules
/// give a tighter bound; more modules give finer control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Node,
    Level,
    Global,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskCertificate {
    pub version: u32,
    pub rule: Rule,
    pub grid: Vec<f64>,
    /// Groups of decision nodes that share one threshold.
    pub modules: Vec<Vec<String>>,
    /// Fingerprint of every decision node when its evidence was collected.
    pub fingerprints: BTreeMap<String, String>,
    pub parts: Vec<Part>,
}

/// Evidence from one calibration sample.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Part {
    /// Number of calibration runs.
    pub n: u64,
    /// Failure probability of this part's own bound.
    pub delta: f64,
    /// Failure probability spent on other estimates taken from this sample: the traffic
    /// shares of later edits. It counts against the confidence of the certificate but
    /// must not loosen the bound of this part.
    #[serde(default)]
    pub spent: f64,
    /// Number of modules the union bound ranges over.
    pub exponent: u32,
    /// Share of the traffic this sample was drawn from (1 for a sample of all traffic).
    pub scale: f64,
    /// Nodes whose traffic the sample was drawn from (empty for all traffic).
    #[serde(default)]
    pub edited: Vec<String>,
    pub routes: Vec<RouteStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteStats {
    pub route: Vec<String>,
    pub runs: u64,
    /// Set when a later edit touched this route: its counts no longer apply.
    #[serde(default)]
    pub stale: bool,
    /// `errors[m][g]`: erroneous runs charged to module `m` that commit at threshold `g`.
    pub errors: Vec<Vec<u64>>,
    /// `review[m][g]`: visits of module `m` with confidence below threshold `g`.
    pub review: Vec<Vec<u64>>,
}

/// Thresholds chosen for a target risk.
#[derive(Debug, Clone)]
pub struct Selection {
    /// Grid index per module; `grid.len()` means never commit.
    pub choice: Vec<usize>,
    pub bound: f64,
    /// Estimated fraction of runs escalated (sum of per-node rates, an upper estimate).
    pub review: f64,
}

/// Result of checking a graph against a certificate.
#[derive(Debug, Clone)]
pub struct RiskCheck {
    pub epsilon: f64,
    pub bound: f64,
    pub confidence: f64,
    /// Decision nodes that changed since their evidence was collected.
    pub stale_nodes: Vec<String>,
    /// Threshold the bound was evaluated at, per module (`None` = never commit).
    pub thresholds: Vec<Option<f64>>,
    pub pass: bool,
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// `ln P(Bin(n, p) <= k)`.
fn ln_binom_cdf(k: u64, n: u64, p: f64) -> f64 {
    if k >= n {
        return 0.0;
    }
    let (lp, lq) = (p.ln(), (-p).ln_1p());
    let mut term = n as f64 * lq;
    let mut acc = term;
    for i in 1..=k {
        term += ((n - i + 1) as f64 / i as f64).ln() + lp - lq;
        let (hi, lo) = if acc > term { (acc, term) } else { (term, acc) };
        acc = hi + (lo - hi).exp().ln_1p();
    }
    acc.min(0.0)
}

/// One-sided Clopper-Pearson upper confidence bound: the largest `p` that is
/// consistent, at level `alpha`, with observing at most `k` successes in `n` trials.
pub fn clopper_pearson_upper(k: u64, n: u64, ln_alpha: f64) -> f64 {
    if n == 0 || k >= n {
        return 1.0;
    }
    let (mut lo, mut hi) = (k as f64 / n as f64, 1.0);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if ln_binom_cdf(k, n, mid) > ln_alpha {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo < 1e-13 {
            break;
        }
    }
    hi
}

// ---------------------------------------------------------------------------
// Graph structure
// ---------------------------------------------------------------------------

fn is_decision(node_type: &str) -> bool {
    node_type == "decision"
}

fn canonical(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for key in keys {
                out.push_str(&format!("{:?}:", key));
                canonical(&map[key], out);
                out.push(',');
            }
            out.push('}');
        }
        serde_json::Value::Array(items) => {
            out.push('[');
            for item in items {
                canonical(item, out);
                out.push(',');
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

fn fnv1a(text: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

/// For every decision node and each of its output ports: the decision nodes
/// reached before any other decision.
fn decision_links(graph: &AnpGraph) -> BTreeMap<String, BTreeMap<String, BTreeSet<String>>> {
    let kinds: HashMap<&str, &str> = graph
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.node_type.as_str()))
        .collect();
    let mut out_edges: HashMap<&str, Vec<&crate::graph::spec::Edge>> = HashMap::new();
    for edge in &graph.edges {
        out_edges
            .entry(edge.from_node.as_str())
            .or_default()
            .push(edge);
    }

    let mut links = BTreeMap::new();
    for node in graph.nodes.iter().filter(|n| is_decision(&n.node_type)) {
        let mut ports: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for edge in out_edges.get(node.id.as_str()).into_iter().flatten() {
            let reached = ports.entry(edge.from_port.clone()).or_default();
            let mut queue = VecDeque::from([edge.to_node.as_str()]);
            let mut seen = HashSet::new();
            while let Some(id) = queue.pop_front() {
                if !seen.insert(id) {
                    continue;
                }
                if kinds.get(id).is_some_and(|t| is_decision(t)) {
                    reached.insert(id.to_string());
                    continue;
                }
                for next in out_edges.get(id).into_iter().flatten() {
                    queue.push_back(next.to_node.as_str());
                }
            }
        }
        links.insert(node.id.clone(), ports);
    }
    links
}

/// Fingerprint of every decision node: its configuration without the tunable
/// thresholds, plus the decision nodes each of its output ports leads to. A
/// node keeps its fingerprint exactly when the runs through it are unaffected.
pub fn decision_fingerprints(graph: &AnpGraph) -> BTreeMap<String, String> {
    let links = decision_links(graph);
    let mut result = BTreeMap::new();
    for node in graph.nodes.iter().filter(|n| is_decision(&n.node_type)) {
        let mut config = node.config.clone();
        if let Some(map) = config.as_object_mut() {
            for tunable in ["threshold", "never_commit", "uncertain_port"] {
                map.remove(tunable);
            }
        }
        let mut text = String::new();
        canonical(&config, &mut text);
        text.push_str(&format!("|{:?}", links[&node.id]));
        result.insert(node.id.clone(), fnv1a(&text));
    }
    result
}

/// Decision nodes that can occur on a route through one of `edited`: the
/// edited nodes, everything downstream of them and everything upstream.
fn nodes_sharing_a_route(graph: &AnpGraph, edited: &HashSet<&str>) -> HashSet<String> {
    let links = decision_links(graph);
    let next = |id: &str| -> Vec<String> {
        links
            .get(id)
            .into_iter()
            .flat_map(|ports| ports.values().flatten().cloned())
            .collect()
    };
    let mut reach: HashSet<String> = edited.iter().map(|s| s.to_string()).collect();
    let mut stack: Vec<String> = reach.iter().cloned().collect();
    while let Some(id) = stack.pop() {
        for succ in next(&id) {
            if reach.insert(succ.clone()) {
                stack.push(succ);
            }
        }
    }
    // Upstream: nodes from which an edited node is reachable.
    loop {
        let before = reach.len();
        for id in links.keys() {
            if !reach.contains(id) && next(id).iter().any(|s| reach.contains(s)) {
                reach.insert(id.clone());
            }
        }
        if reach.len() == before {
            return reach;
        }
    }
}

// ---------------------------------------------------------------------------
// Building certificates
// ---------------------------------------------------------------------------

fn validate_traces(traces: &[RunTrace], decisions: &BTreeSet<&str>) -> Result<(), RiskError> {
    if traces.is_empty() {
        return Err(RiskError::NoTraces);
    }
    for (index, trace) in traces.iter().enumerate() {
        if trace.steps.is_empty() {
            return Err(RiskError::BadTrace {
                index,
                message: "run has no steps".into(),
            });
        }
        for step in &trace.steps {
            if !decisions.contains(step.node.as_str()) {
                return Err(RiskError::BadTrace {
                    index,
                    message: format!("'{}' is not a decision node of the graph", step.node),
                });
            }
            if !(0.0..=1.0).contains(&step.confidence) {
                return Err(RiskError::BadTrace {
                    index,
                    message: format!("confidence {} is outside [0, 1]", step.confidence),
                });
            }
        }
    }
    Ok(())
}

fn build_modules(
    traces: &[RunTrace],
    decisions: &BTreeSet<&str>,
    granularity: Granularity,
) -> Vec<Vec<String>> {
    let all: Vec<String> = decisions.iter().map(|s| s.to_string()).collect();
    match granularity {
        Granularity::Node => all.into_iter().map(|n| vec![n]).collect(),
        Granularity::Global => vec![all],
        Granularity::Level => {
            let mut depth: BTreeMap<&str, usize> = BTreeMap::new();
            for trace in traces {
                for (pos, step) in trace.steps.iter().enumerate() {
                    let entry = depth.entry(step.node.as_str()).or_insert(pos);
                    *entry = (*entry).min(pos);
                }
            }
            let levels = depth.values().copied().max().map_or(0, |d| d + 1);
            let mut modules: Vec<Vec<String>> = vec![Vec::new(); levels];
            let mut unseen = Vec::new();
            for node in all {
                match depth.get(node.as_str()) {
                    Some(&d) => modules[d].push(node),
                    None => unseen.push(node),
                }
            }
            if !unseen.is_empty() {
                modules.push(unseen);
            }
            modules.retain(|m| !m.is_empty());
            modules
        }
    }
}

/// Number of grid options (finite thresholds plus "never commit").
fn options(grid: &[f64]) -> usize {
    grid.len() + 1
}

fn route_stats(
    traces: &[RunTrace],
    modules: &[Vec<String>],
    grid: &[f64],
    rule: Rule,
) -> Vec<RouteStats> {
    let module_of: HashMap<&str, usize> = modules
        .iter()
        .enumerate()
        .flat_map(|(m, nodes)| nodes.iter().map(move |n| (n.as_str(), m)))
        .collect();
    let g_count = options(grid);
    let mut table: BTreeMap<Vec<String>, RouteStats> = BTreeMap::new();

    for trace in traces {
        let route: Vec<String> = trace.steps.iter().map(|s| s.node.clone()).collect();
        let stats = table.entry(route.clone()).or_insert_with(|| RouteStats {
            route,
            runs: 0,
            stale: false,
            errors: vec![vec![0; g_count]; modules.len()],
            review: vec![vec![0; g_count]; modules.len()],
        });
        stats.runs += 1;

        for step in &trace.steps {
            let m = module_of[step.node.as_str()];
            // Escalated at threshold g when the confidence is below it; always for "never".
            let first = grid.partition_point(|&t| t <= step.confidence);
            for cell in &mut stats.review[m][first..] {
                *cell += 1;
            }
        }

        if let Some(first_error) = trace.steps.iter().position(|s| !s.correct) {
            let charged = match rule {
                Rule::FirstError => &trace.steps[first_error],
                Rule::Min => {
                    // Ties go to the first wrong decision if it is among the least
                    // confident, else to the earliest node; they are common when
                    // confidences saturate at 1.
                    let lowest = trace
                        .steps
                        .iter()
                        .map(|s| s.confidence)
                        .fold(f64::INFINITY, f64::min);
                    if trace.steps[first_error].confidence == lowest {
                        &trace.steps[first_error]
                    } else {
                        trace
                            .steps
                            .iter()
                            .find(|s| s.confidence == lowest)
                            .expect("validated: runs are not empty")
                    }
                }
            };
            let m = module_of[charged.node.as_str()];
            // Committed at threshold g when the confidence is at least it; never for "never".
            let commits = grid.partition_point(|&t| t <= charged.confidence);
            for cell in &mut stats.errors[m][..commits] {
                *cell += 1;
            }
        }
    }
    table.into_values().collect()
}

/// Issue a certificate from labelled traces of all traffic, on the default grid.
pub fn certify(
    graph: &AnpGraph,
    traces: &[RunTrace],
    rule: Rule,
    granularity: Granularity,
    delta: f64,
) -> Result<RiskCertificate, RiskError> {
    certify_on_grid(graph, traces, rule, granularity, delta, &GRID)
}

/// Issue a certificate on a given grid of finite thresholds (ascending, within
/// [0, 1]). The grid must be chosen before looking at the traces.
pub fn certify_on_grid(
    graph: &AnpGraph,
    traces: &[RunTrace],
    rule: Rule,
    granularity: Granularity,
    delta: f64,
    grid: &[f64],
) -> Result<RiskCertificate, RiskError> {
    let ascending = grid.windows(2).all(|w| w[0] < w[1]);
    if grid.is_empty() || !ascending || grid[0] < 0.0 || grid[grid.len() - 1] > 1.0 {
        return Err(RiskError::Other(
            "the threshold grid must be ascending and within [0, 1]".into(),
        ));
    }
    if !(delta > 0.0 && delta < 1.0) {
        return Err(RiskError::BadDelta);
    }
    let fingerprints = decision_fingerprints(graph);
    if fingerprints.is_empty() {
        return Err(RiskError::NoDecisions);
    }
    let decisions: BTreeSet<&str> = fingerprints.keys().map(String::as_str).collect();
    validate_traces(traces, &decisions)?;
    let modules = build_modules(traces, &decisions, granularity);
    let routes = route_stats(traces, &modules, grid, rule);
    Ok(RiskCertificate {
        version: 1,
        rule,
        grid: grid.to_vec(),
        parts: vec![Part {
            n: traces.len() as u64,
            delta,
            spent: 0.0,
            exponent: modules.len() as u32,
            scale: 1.0,
            edited: Vec::new(),
            routes,
        }],
        modules,
        fingerprints,
    })
}

impl Part {
    fn ln_alpha(&self, g_count: usize) -> f64 {
        self.delta.ln() - self.exponent as f64 * (g_count as f64).ln()
    }

    fn live(&self) -> impl Iterator<Item = &RouteStats> {
        self.routes.iter().filter(|r| !r.stale)
    }

    /// Charged errors of module `m` at grid option `g`, over the routes still valid.
    fn errors(&self, m: usize, g: usize) -> u64 {
        self.live().map(|r| r.errors[m][g]).sum()
    }

    fn upper(&self, count: u64, g_count: usize) -> f64 {
        self.scale * clopper_pearson_upper(count, self.n, self.ln_alpha(g_count))
    }
}

impl RiskCertificate {
    fn g_count(&self) -> usize {
        options(&self.grid)
    }

    /// Probability that every bound of this certificate holds.
    pub fn confidence(&self) -> f64 {
        1.0 - self.parts.iter().map(|p| p.delta + p.spent).sum::<f64>()
    }

    /// Certified upper bound on the error rate for one grid option per module.
    pub fn bound(&self, choice: &[usize]) -> f64 {
        let g_count = self.g_count();
        // A workflow that never commits cannot complete a wrong run: no estimate needed.
        if choice.iter().all(|&g| g + 1 == g_count) {
            return 0.0;
        }
        self.parts
            .iter()
            .map(|part| {
                let count = choice
                    .iter()
                    .enumerate()
                    .map(|(m, &g)| part.errors(m, g))
                    .sum();
                part.upper(count, g_count)
            })
            .sum()
    }

    /// Estimated escalation rate of module `m` at grid option `g`.
    fn review_rate(&self, m: usize, g: usize) -> f64 {
        self.parts
            .iter()
            .map(|part| {
                let visits: u64 = part.live().map(|r| r.review[m][g]).sum();
                part.scale * visits as f64 / part.n as f64
            })
            .sum()
    }

    pub fn threshold(&self, g: usize) -> Option<f64> {
        self.grid.get(g).copied()
    }

    /// Choose thresholds that minimise the estimated review rate subject to
    /// the certified bound staying within `epsilon`. Always succeeds: never
    /// committing anywhere has bound zero.
    pub fn optimize(&self, epsilon: f64) -> Result<Selection, RiskError> {
        if !(epsilon > 0.0 && epsilon < 1.0) {
            return Err(RiskError::BadEpsilon);
        }
        let choice = if self.parts.len() == 1 {
            self.optimize_exact(epsilon)
        } else {
            self.optimize_greedy(epsilon)
        };
        Ok(Selection {
            bound: self.bound(&choice),
            review: choice
                .iter()
                .enumerate()
                .map(|(m, &g)| self.review_rate(m, g))
                .sum(),
            choice,
        })
    }

    /// One part: multiple-choice knapsack over integer error counts.
    fn optimize_exact(&self, epsilon: f64) -> Vec<usize> {
        let part = &self.parts[0];
        let (m_count, g_count) = (self.modules.len(), self.g_count());
        let never = g_count - 1;
        let counts: Vec<Vec<u64>> = (0..m_count)
            .map(|m| (0..g_count).map(|g| part.errors(m, g)).collect())
            .collect();
        let most: u64 = counts.iter().map(|c| c[0]).sum();
        // Largest total count whose bound still fits the budget (the bound grows with it).
        let (mut lo, mut hi) = (0u64, most);
        if part.upper(0, g_count) > epsilon {
            return vec![never; m_count];
        }
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if part.upper(mid, g_count) <= epsilon {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let cap = lo as usize;

        let mut best = vec![0.0f64; cap + 1];
        let mut back = vec![vec![never; cap + 1]; m_count];
        for (m, module_counts) in counts.iter().enumerate() {
            let mut next = vec![f64::INFINITY; cap + 1];
            for (g, &count) in module_counts.iter().enumerate() {
                let cost = count as usize;
                if cost > cap {
                    continue;
                }
                let value = self.review_rate(m, g);
                for budget in cost..=cap {
                    let candidate = best[budget - cost] + value;
                    if candidate < next[budget] {
                        next[budget] = candidate;
                        back[m][budget] = g;
                    }
                }
            }
            best = next;
        }
        let mut choice = vec![never; m_count];
        let mut budget = cap;
        for m in (0..m_count).rev() {
            let g = back[m][budget];
            choice[m] = g;
            budget -= counts[m][g] as usize;
        }
        choice
    }

    /// Several parts: start from "never commit" and keep lowering the threshold
    /// that buys the largest review reduction per unit of bound.
    fn optimize_greedy(&self, epsilon: f64) -> Vec<usize> {
        let (m_count, g_count) = (self.modules.len(), self.g_count());
        let mut choice = vec![g_count - 1; m_count];
        let mut current = self.bound(&choice);
        loop {
            let mut best: Option<(f64, usize, usize, f64)> = None;
            for m in 0..m_count {
                let now = self.review_rate(m, choice[m]);
                for g in 0..choice[m] {
                    let gain = now - self.review_rate(m, g);
                    if gain <= 0.0 {
                        continue;
                    }
                    let mut trial = choice.clone();
                    trial[m] = g;
                    let bound = self.bound(&trial);
                    if bound > epsilon {
                        continue;
                    }
                    let score = gain / (bound - current).max(1e-12);
                    if best.is_none_or(|(s, ..)| score > s) {
                        best = Some((score, m, g, bound));
                    }
                }
            }
            match best {
                Some((_, m, g, bound)) => {
                    choice[m] = g;
                    current = bound;
                }
                None => return choice,
            }
        }
    }

    /// Decision nodes whose fingerprint differs from the one recorded with their evidence.
    pub fn stale_nodes(&self, graph: &AnpGraph) -> Vec<String> {
        let current = decision_fingerprints(graph);
        let mut stale: Vec<String> = current
            .iter()
            .filter(|(id, fp)| self.fingerprints.get(*id) != Some(*fp))
            .map(|(id, _)| id.clone())
            .collect();
        stale.extend(
            self.fingerprints
                .keys()
                .filter(|id| !current.contains_key(*id))
                .cloned(),
        );
        stale.sort();
        stale
    }

    /// Check the thresholds configured in `graph` against a target risk.
    ///
    /// A threshold between two grid points is evaluated at the lower one, and
    /// a module whose nodes differ is evaluated at its lowest threshold; both
    /// can only overstate the risk.
    pub fn check(&self, graph: &AnpGraph, epsilon: f64) -> RiskCheck {
        let configs: HashMap<&str, &serde_json::Value> = graph
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), &n.config))
            .collect();
        let never = self.grid.len();
        let choice: Vec<usize> = self
            .modules
            .iter()
            .map(|nodes| {
                nodes
                    .iter()
                    .filter_map(|id| configs.get(id.as_str()))
                    .map(|config| {
                        if config.get("never_commit").and_then(|v| v.as_bool()) == Some(true) {
                            return never;
                        }
                        let threshold = config
                            .get("threshold")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(DEFAULT_THRESHOLD);
                        self.grid
                            .partition_point(|&t| t <= threshold + 1e-12)
                            .saturating_sub(1)
                    })
                    .min()
                    .unwrap_or(never)
            })
            .collect();
        let bound = self.bound(&choice);
        let stale_nodes = self.stale_nodes(graph);
        RiskCheck {
            epsilon,
            bound,
            confidence: self.confidence(),
            pass: stale_nodes.is_empty() && bound <= epsilon,
            stale_nodes,
            thresholds: choice.iter().map(|&g| self.threshold(g)).collect(),
        }
    }

    /// Write the chosen thresholds into the decision nodes of `graph`.
    pub fn apply(&self, graph: &mut AnpGraph, selection: &Selection) {
        let chosen: HashMap<&str, Option<f64>> = self
            .modules
            .iter()
            .zip(&selection.choice)
            .flat_map(|(nodes, &g)| nodes.iter().map(move |n| (n.as_str(), self.threshold(g))))
            .collect();
        for node in &mut graph.nodes {
            let Some(threshold) = chosen.get(node.id.as_str()) else {
                continue;
            };
            if !node.config.is_object() {
                node.config = serde_json::json!({});
            }
            let config = node.config.as_object_mut().expect("just ensured");
            match threshold {
                Some(t) => {
                    config.insert("threshold".into(), serde_json::json!(t));
                    config.remove("never_commit");
                }
                None => {
                    config.insert("never_commit".into(), serde_json::json!(true));
                }
            }
        }
    }

    /// Recertify after `edited` nodes changed, reusing the evidence the edit left intact.
    ///
    /// A run that avoids every edited node behaves exactly as before, and
    /// whether a run reaches an edited node is decided by unedited nodes. The
    /// old counts of the untouched routes are therefore kept, and `fresh` —
    /// labelled runs of the edited workflow, drawn from the traffic that
    /// reaches an edited node — covers the rest. `delta` is the additional
    /// failure probability spent on the new evidence.
    pub fn recertify(
        &self,
        graph: &AnpGraph,
        fresh: &[RunTrace],
        edited: &[String],
        delta: f64,
    ) -> Result<RiskCertificate, RiskError> {
        if !(delta > 0.0 && delta < 1.0) {
            return Err(RiskError::BadDelta);
        }
        let fingerprints = decision_fingerprints(graph);
        let decisions: BTreeSet<&str> = fingerprints.keys().map(String::as_str).collect();
        for node in edited {
            if !decisions.contains(node.as_str()) {
                return Err(RiskError::UnknownNode(node.clone()));
            }
        }
        validate_traces(fresh, &decisions)?;
        let edited_set: HashSet<&str> = edited.iter().map(String::as_str).collect();
        for (index, trace) in fresh.iter().enumerate() {
            if !trace
                .steps
                .iter()
                .any(|s| edited_set.contains(s.node.as_str()))
            {
                return Err(RiskError::BadTrace {
                    index,
                    message: "run does not pass through an edited node".into(),
                });
            }
        }
        let base = self
            .parts
            .iter()
            .find(|p| p.edited.is_empty())
            .ok_or_else(|| RiskError::Other("certificate has no sample of all traffic".into()))?;

        // Nodes added by the edit get a module of their own.
        let mut modules = self.modules.clone();
        let known: HashSet<String> = modules.iter().flatten().cloned().collect();
        for node in fingerprints.keys().filter(|n| !known.contains(*n)) {
            modules.push(vec![node.clone()]);
        }
        let grow = modules.len() - self.modules.len();

        // Share of the traffic that can reach an edited node: runs whose recorded route
        // meets one, or meets a route an earlier edit already invalidated.
        let reaching: u64 = base
            .routes
            .iter()
            .filter(|r| r.stale || r.route.iter().any(|n| edited_set.contains(n.as_str())))
            .map(|r| r.runs)
            .sum();
        let share = clopper_pearson_upper(reaching, base.n, (delta / 2.0).ln());

        let mut parts: Vec<Part> = self.parts.clone();
        for part in &mut parts {
            for route in &mut part.routes {
                if route.route.iter().any(|n| edited_set.contains(n.as_str())) {
                    route.stale = true;
                }
                for _ in 0..grow {
                    route.errors.push(vec![0; options(&self.grid)]);
                    route.review.push(vec![0; options(&self.grid)]);
                }
            }
        }
        // The fresh evidence depends on the thresholds of the modules that can occur on a
        // route through the edit; this is a property of the graph, not of the sample.
        let reach = nodes_sharing_a_route(graph, &edited_set);
        let touched = modules
            .iter()
            .filter(|m| m.iter().any(|n| reach.contains(n)))
            .count();
        parts.push(Part {
            n: fresh.len() as u64,
            delta: delta / 2.0,
            spent: 0.0,
            exponent: touched.max(1) as u32,
            scale: share,
            edited: edited.to_vec(),
            routes: route_stats(fresh, &modules, &self.grid, self.rule),
        });
        // The share estimate spends the other half of delta. It is recorded on the base
        // part, whose own bound keeps the level it was certified at.
        if let Some(base) = parts.iter_mut().find(|p| p.edited.is_empty()) {
            base.spent += delta / 2.0;
        }

        let mut merged = self.fingerprints.clone();
        merged.retain(|id, _| fingerprints.contains_key(id));
        for (id, fp) in &fingerprints {
            if edited_set.contains(id.as_str()) || !merged.contains_key(id) {
                merged.insert(id.clone(), fp.clone());
            }
        }
        Ok(RiskCertificate {
            version: self.version,
            rule: self.rule,
            grid: self.grid.clone(),
            modules,
            fingerprints: merged,
            parts,
        })
    }
}

fn show_threshold(t: Option<f64>) -> String {
    t.map_or("never commit".to_string(), |t| format!("{:.3}", t))
}

impl RiskCertificate {
    /// Human-readable report of a selection.
    pub fn describe(&self, selection: &Selection) -> String {
        let mut out = String::new();
        out.push_str("  module                                   threshold     errors charged\n");
        for (m, nodes) in self.modules.iter().enumerate() {
            let g = selection.choice[m];
            let charged: u64 = self.parts.iter().map(|p| p.errors(m, g)).sum();
            let name = if nodes.len() == 1 {
                nodes[0].clone()
            } else {
                format!("{} (+{} more)", nodes[0], nodes.len() - 1)
            };
            out.push_str(&format!(
                "  {:<40} {:<13} {}\n",
                name,
                show_threshold(self.threshold(g)),
                charged
            ));
        }
        out.push_str(&format!(
            "  certified error rate <= {:.4} with probability >= {:.3}\n",
            selection.bound,
            self.confidence()
        ));
        out.push_str(&format!(
            "  estimated human review rate: {:.1}%",
            selection.review.min(1.0) * 100.0
        ));
        out
    }
}

impl fmt::Display for RiskCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.stale_nodes.is_empty() {
            return write!(
                f,
                "certificate is stale for: {} — recertify the traffic through them",
                self.stale_nodes.join(", ")
            );
        }
        write!(
            f,
            "certified error rate <= {:.4} (target {:.4}, probability >= {:.3})",
            self.bound, self.epsilon, self.confidence
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::spec::*;

    fn node(id: &str, node_type: &str, config: serde_json::Value) -> Node {
        Node {
            id: id.into(),
            node_type: node_type.into(),
            kind: NodeKind::Processor,
            config,
            inputs: vec![],
            outputs: vec![],
        }
    }

    fn edge(from: &str, port: &str, to: &str) -> Edge {
        Edge {
            id: format!("{}.{}->{}", from, port, to),
            from_node: from.into(),
            from_port: port.into(),
            to_node: to.into(),
            to_port: "default".into(),
        }
    }

    /// root routes to leaf_a or leaf_b; each leaf picks between two sinks.
    fn graph() -> AnpGraph {
        let decision = |q: &str| serde_json::json!({"question": q, "choices": ["a", "b"]});
        AnpGraph {
            version: GraphVersion::V0_1,
            metadata: GraphMetadata {
                name: "t".into(),
                description: String::new(),
            },
            nodes: vec![
                node("root", "decision", decision("which branch?")),
                node("fmt", "transform", serde_json::json!({})),
                node("leaf_a", "decision", decision("a?")),
                node("leaf_b", "decision", decision("b?")),
                node("sink", "console", serde_json::json!({})),
            ],
            edges: vec![
                edge("root", "a", "fmt"),
                edge("fmt", "default", "leaf_a"),
                edge("root", "b", "leaf_b"),
                edge("leaf_a", "a", "sink"),
                edge("leaf_b", "a", "sink"),
            ],
        }
    }

    fn run(steps: &[(&str, f64, bool)]) -> RunTrace {
        RunTrace {
            steps: steps
                .iter()
                .map(|&(node, confidence, correct)| Step {
                    node: node.into(),
                    confidence,
                    correct,
                })
                .collect(),
        }
    }

    /// 1000 runs: 2% confident errors at the root that leaf_a would have caught,
    /// 3% low-confidence errors at leaf_b, the rest correct.
    fn traces() -> Vec<RunTrace> {
        let mut out = Vec::new();
        for i in 0..1000 {
            out.push(match i % 100 {
                0 | 1 => run(&[("root", 0.99, false), ("leaf_a", 0.30, false)]),
                2..=4 => run(&[("root", 0.92, true), ("leaf_b", 0.55, false)]),
                5..=49 => run(&[("root", 0.95, true), ("leaf_a", 0.90, true)]),
                _ => run(&[("root", 0.97, true), ("leaf_b", 0.85, true)]),
            });
        }
        out
    }

    #[test]
    fn clopper_pearson_matches_reference_values() {
        // Reference values computed with scipy.stats.beta.isf(alpha, k + 1, n - k).
        let cases = [
            (0u64, 4000u64, 1e-17f64, 0.00973825968990836),
            (5, 4000, 1e-17, 0.013525215079909383),
            (50, 4000, 1e-3, 0.0189238531659666),
            (0, 100, 0.05, 0.029513049607039932),
        ];
        for (k, n, alpha, expected) in cases {
            let got = clopper_pearson_upper(k, n, alpha.ln());
            assert!(
                (got - expected).abs() < 1e-9,
                "{k}/{n}: {got} vs {expected}"
            );
        }
        assert_eq!(clopper_pearson_upper(10, 10, (0.05f64).ln()), 1.0);
    }

    #[test]
    fn min_rule_sees_downstream_rescue() {
        let g = graph();
        let fd = certify(&g, &traces(), Rule::FirstError, Granularity::Node, 0.1).unwrap();
        let min = certify(&g, &traces(), Rule::Min, Granularity::Node, 0.1).unwrap();
        // Modules are sorted by node id: leaf_a, leaf_b, root. Threshold 0.5 everywhere.
        let half = GRID.iter().position(|&t| t == 0.5).unwrap();
        let count =
            |c: &RiskCertificate| -> u64 { (0..3).map(|m| c.parts[0].errors(m, half)).sum() };
        // First-error charging: the 20 root errors commit (0.99) plus the 30 leaf_b errors.
        assert_eq!(count(&fd), 50);
        // Least-confident charging: the root errors are charged to leaf_a (0.30), which escalates.
        assert_eq!(count(&min), 30);
        assert!(min.bound(&[half; 3]) < fd.bound(&[half; 3]));
    }

    #[test]
    fn min_rule_breaks_ties_towards_the_first_error() {
        let g = graph();
        // Both nodes are fully confident; leaf_b is the one that is wrong.
        let tied = vec![run(&[("root", 1.0, true), ("leaf_b", 1.0, false)]); 10];
        let cert = certify(&g, &tied, Rule::Min, Granularity::Node, 0.1).unwrap();
        let top = GRID.len() - 1;
        let leaf_b = cert.modules.iter().position(|m| m[0] == "leaf_b").unwrap();
        let root = cert.modules.iter().position(|m| m[0] == "root").unwrap();
        assert_eq!(cert.parts[0].errors(leaf_b, top), 10);
        assert_eq!(cert.parts[0].errors(root, top), 0);
    }

    #[test]
    fn fine_tail_separates_saturated_confidences() {
        let g = graph();
        // A language-model-like node: every answer is near 1, the wrong ones slightly less so.
        let mut runs = Vec::new();
        for i in 0..2000 {
            runs.push(if i % 20 == 0 {
                run(&[("root", 0.9995, false), ("leaf_a", 0.99995, true)])
            } else {
                run(&[("root", 0.9999995, true), ("leaf_a", 0.9999995, true)])
            });
        }
        let coarse = certify(&g, &runs, Rule::Min, Granularity::Global, 0.1).unwrap();
        let grid: Vec<f64> = GRID.iter().chain(&FINE_TAIL).copied().collect();
        let fine = certify_on_grid(&g, &runs, Rule::Min, Granularity::Global, 0.1, &grid).unwrap();
        // 5% of the runs are wrong at 0.9995: the default grid cannot exclude them, the tail can.
        let coarse_pick = coarse.optimize(0.02).unwrap();
        assert_eq!(coarse_pick.choice, vec![GRID.len()]);
        let fine_pick = fine.optimize(0.02).unwrap();
        assert_eq!(fine.threshold(fine_pick.choice[0]), Some(0.9999));
        assert!(fine_pick.review < 0.06);
        assert!(
            certify_on_grid(&g, &runs, Rule::Min, Granularity::Global, 0.1, &[0.5, 0.2]).is_err()
        );
    }

    #[test]
    fn optimize_respects_the_budget_and_is_feasible() {
        let cert = certify(&graph(), &traces(), Rule::Min, Granularity::Node, 0.1).unwrap();
        for eps in [0.001, 0.02, 0.05, 0.2] {
            let sel = cert.optimize(eps).unwrap();
            assert!(sel.bound <= eps, "eps {eps}: bound {}", sel.bound);
        }
        // A tiny budget forces every module to never commit.
        let strict = cert.optimize(0.001).unwrap();
        assert!(strict.choice.iter().all(|&g| g == GRID.len()));
        assert_eq!(strict.bound, 0.0);
        // A generous budget lets every node commit freely.
        let loose = cert.optimize(0.2).unwrap();
        assert!(loose.review < 0.05);
        // More budget never costs more review.
        let mid = cert.optimize(0.05).unwrap();
        assert!(mid.review <= cert.optimize(0.02).unwrap().review + 1e-12);
    }

    #[test]
    fn exact_optimum_beats_every_shared_threshold() {
        let cert = certify(&graph(), &traces(), Rule::Min, Granularity::Node, 0.1).unwrap();
        let sel = cert.optimize(0.05).unwrap();
        for g in 0..=GRID.len() {
            let shared = [g; 3];
            if cert.bound(&shared) <= 0.05 {
                let review: f64 = (0..3).map(|m| cert.review_rate(m, g)).sum();
                assert!(sel.review <= review + 1e-12);
            }
        }
    }

    #[test]
    fn granularity_groups_nodes() {
        let g = graph();
        let level = certify(&g, &traces(), Rule::Min, Granularity::Level, 0.1).unwrap();
        assert_eq!(level.modules, vec![vec!["root"], vec!["leaf_a", "leaf_b"]]);
        let global = certify(&g, &traces(), Rule::Min, Granularity::Global, 0.1).unwrap();
        assert_eq!(global.modules.len(), 1);
        // Fewer modules: smaller union bound, so the same thresholds certify a lower risk.
        let node = certify(&g, &traces(), Rule::Min, Granularity::Node, 0.1).unwrap();
        let half = GRID.iter().position(|&t| t == 0.5).unwrap();
        assert!(global.bound(&[half]) < level.bound(&[half; 2]));
        assert!(level.bound(&[half; 2]) < node.bound(&[half; 3]));
    }

    #[test]
    fn check_reads_thresholds_from_the_graph() {
        let mut g = graph();
        let cert = certify(&g, &traces(), Rule::Min, Granularity::Level, 0.1).unwrap();
        let sel = cert.optimize(0.05).unwrap();
        cert.apply(&mut g, &sel);
        let check = cert.check(&g, 0.05);
        assert!(check.pass, "{check}");
        assert!((check.bound - sel.bound).abs() < 1e-12);
        // Thresholds are tunable: changing them never makes the certificate stale.
        assert!(check.stale_nodes.is_empty());

        // Lowering a threshold below what was certified fails the check.
        for n in &mut g.nodes {
            if n.node_type == "decision" {
                n.config["threshold"] = serde_json::json!(0.0);
                n.config.as_object_mut().unwrap().remove("never_commit");
            }
        }
        assert!(!cert.check(&g, 0.05).pass);
    }

    #[test]
    fn edits_make_only_the_touched_nodes_stale() {
        let g = graph();
        let cert = certify(&g, &traces(), Rule::Min, Granularity::Node, 0.1).unwrap();

        let mut model_swap = g.clone();
        model_swap.nodes[3].config["model"] = serde_json::json!("other");
        assert_eq!(cert.stale_nodes(&model_swap), vec!["leaf_b"]);

        // Inserting a decision below a port changes the routing of the node above it.
        let mut inserted = g.clone();
        inserted.nodes.push(node(
            "extra",
            "decision",
            serde_json::json!({"question": "x", "choices": ["a", "b"]}),
        ));
        inserted.edges.push(edge("leaf_b", "b", "extra"));
        assert_eq!(cert.stale_nodes(&inserted), vec!["extra", "leaf_b"]);

        // Editing a node that is not a decision, or a sink, leaves the certificate valid.
        let mut cosmetic = g.clone();
        cosmetic.nodes[4].config["prefix"] = serde_json::json!("> ");
        assert!(cert.stale_nodes(&cosmetic).is_empty());
    }

    #[test]
    fn recertify_reuses_untouched_routes() {
        let g = graph();
        let cert = certify(&g, &traces(), Rule::Min, Granularity::Node, 0.05).unwrap();

        let mut edited = g.clone();
        edited.nodes[3].config["model"] = serde_json::json!("better");
        assert!(!cert.check(&edited, 0.1).pass);

        // The new leaf_b is always right; label 200 runs of the traffic through it.
        let fresh: Vec<RunTrace> = (0..200)
            .map(|_| run(&[("root", 0.95, true), ("leaf_b", 0.9, true)]))
            .collect();
        let next = cert
            .recertify(&edited, &fresh, &["leaf_b".to_string()], 0.05)
            .unwrap();
        assert!(next.stale_nodes(&edited).is_empty());
        assert_eq!(next.parts.len(), 2);
        assert!((next.confidence() - 0.9).abs() < 1e-12);
        // The failure probability of the share estimate must not loosen the old bound.
        assert_eq!(next.parts[0].delta, cert.parts[0].delta);
        assert_eq!(
            next.parts[0].upper(3, options(&GRID)),
            cert.parts[0].upper(3, options(&GRID))
        );
        // A second edit spends its own failure probability on top.
        let again = next
            .recertify(&edited, &fresh, &["leaf_b".to_string()], 0.02)
            .unwrap();
        assert!((again.confidence() - 0.88).abs() < 1e-12);
        assert_eq!(again.parts[0].delta, cert.parts[0].delta);

        // Routes through leaf_b are dropped from the old evidence, the others are kept.
        let old = &next.parts[0];
        assert!(old
            .routes
            .iter()
            .all(|r| r.stale == r.route.contains(&"leaf_b".to_string())));
        // Routes through leaf_b can meet root and leaf_b but never leaf_a.
        assert_eq!(next.parts[1].exponent, 2);
        // The fresh part is weighted by the traffic share of leaf_b (about 53%).
        let share = next.parts[1].scale;
        assert!(share > 0.53 && share < 0.60, "share {share}");

        let half = GRID.iter().position(|&t| t == 0.5).unwrap();
        // leaf_b's 30 errors are gone; only the rescued root errors remain, and leaf_a escalates them.
        let total: u64 = (0..3).map(|m| next.parts[0].errors(m, half)).sum();
        assert_eq!(total, 0);
        assert!(next.optimize(0.05).unwrap().bound <= 0.05);

        // Fresh runs must come from the edited traffic.
        let wrong = vec![run(&[("root", 0.9, true), ("leaf_a", 0.9, true)])];
        assert!(cert
            .recertify(&edited, &wrong, &["leaf_b".to_string()], 0.05)
            .is_err());
    }

    /// The example under examples/certify holds real model outputs (CLINC150) and
    /// reference numbers computed by the independent Python implementation.
    #[test]
    fn matches_the_reference_implementation_on_real_traces() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/certify");
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
        let graph = crate::graph::loader::load_from_str(&read("workflow.anp.json")).unwrap();
        let traces: Vec<RunTrace> = read("traces.jsonl")
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let reference: serde_json::Value = serde_json::from_str(&read("reference.json")).unwrap();
        assert_eq!(traces.len() as u64, reference["n"].as_u64().unwrap());
        assert_eq!(
            GRID.len() as u64 + 1,
            reference["grid_options"].as_u64().unwrap()
        );
        let at_07 = GRID.iter().position(|&t| t == 0.7).unwrap();

        for (label, granularity) in [
            ("level", Granularity::Level),
            ("node", Granularity::Node),
            ("global", Granularity::Global),
        ] {
            let cert = certify(&graph, &traces, Rule::Min, granularity, 0.1).unwrap();
            let expect = &reference[label];
            let shared = vec![at_07; cert.modules.len()];
            let errors: u64 = (0..cert.modules.len())
                .map(|m| cert.parts[0].errors(m, at_07))
                .sum();
            assert_eq!(errors, expect["errors_at_0.7"].as_u64().unwrap(), "{label}");
            let bound = expect["bound_at_0.7"].as_f64().unwrap();
            assert!((cert.bound(&shared) - bound).abs() < 1e-9, "{label}");

            let best = cert.optimize(0.05).unwrap();
            let review = expect["optimal_review_at_0.05"].as_f64().unwrap();
            assert!(
                (best.review - review).abs() < 1e-9,
                "{label}: {}",
                best.review
            );
            assert!(best.bound <= 0.05);
        }

        let fd = certify(&graph, &traces, Rule::FirstError, Granularity::Level, 0.1).unwrap();
        let errors: u64 = (0..fd.modules.len())
            .map(|m| fd.parts[0].errors(m, at_07))
            .sum();
        assert_eq!(
            errors,
            reference["first_error_level_errors_at_0.7"]
                .as_u64()
                .unwrap()
        );

        // Edit one leaf model: only that node goes stale, and 300 fresh runs restore the check.
        let edited =
            crate::graph::loader::load_from_str(&read("workflow_edited.anp.json")).unwrap();
        let cert = certify(&graph, &traces, Rule::Min, Granularity::Level, 0.1).unwrap();
        assert_eq!(cert.stale_nodes(&edited), vec!["intent_banking"]);
        let fresh: Vec<RunTrace> = read("fresh_intent_banking.jsonl")
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let next = cert
            .recertify(&edited, &fresh, &["intent_banking".to_string()], 0.05)
            .unwrap();
        let mut tuned = edited.clone();
        let selection = next.optimize(0.05).unwrap();
        next.apply(&mut tuned, &selection);
        assert!(next.check(&tuned, 0.05).pass);
        assert!(selection.review < 0.6);
    }

    #[test]
    fn rejects_bad_input() {
        let g = graph();
        assert!(matches!(
            certify(&g, &[], Rule::Min, Granularity::Node, 0.1),
            Err(RiskError::NoTraces)
        ));
        let unknown = vec![run(&[("nope", 0.5, true)])];
        assert!(certify(&g, &unknown, Rule::Min, Granularity::Node, 0.1).is_err());
        let range = vec![run(&[("root", 1.5, true)])];
        assert!(certify(&g, &range, Rule::Min, Granularity::Node, 0.1).is_err());
        assert!(certify(&g, &traces(), Rule::Min, Granularity::Node, 0.0).is_err());
        let cert = certify(&g, &traces(), Rule::Min, Granularity::Node, 0.1).unwrap();
        assert!(cert.optimize(0.0).is_err());
    }

    #[test]
    fn certificate_round_trips_through_json() {
        let cert = certify(&graph(), &traces(), Rule::Min, Granularity::Level, 0.1).unwrap();
        let json = serde_json::to_string(&cert).unwrap();
        let back: RiskCertificate = serde_json::from_str(&json).unwrap();
        let sel = cert.optimize(0.05).unwrap();
        assert_eq!(back.optimize(0.05).unwrap().choice, sel.choice);
        assert!((back.bound(&sel.choice) - sel.bound).abs() < 1e-15);
    }
}
