"""Export the CLINC workflow as an ANP graph with labelled calibration traces, so the
`anp certify` command can be tried (and is tested) on real model outputs.

Writes examples/certify/{workflow.anp.json, traces.jsonl, fresh_intent_banking.jsonl,
workflow_edited.anp.json, reference.json}.
"""
import json
import os

import numpy as np

import riskcert as rc
from run_experiments import CACHE, levels

OUT = os.path.join(os.path.dirname(__file__), "..", "..", "examples", "certify")
N_TRACES, N_FRESH, EDITED = 3000, 300, 3


def node_id(name):
    return name.replace("[", "_").replace("]", "")


def main():
    os.makedirs(OUT, exist_ok=True)
    d = np.load(os.path.join(CACHE, "clinc.npz"))
    names = [node_id(n) for n in d["names"]]
    domains = [n[len("intent_"):] for n in names[2:]]
    truth, nxt = d["truth"].astype(int), d["nxt"].astype(int)
    pop = rc.traces_from_tables(d["pred"].astype(int), d["conf"].astype(float), truth, nxt)
    pred1, conf1 = d["pred"].astype(int).copy(), d["conf"].astype(float).copy()
    pred1[:, EDITED], conf1[:, EDITED] = d["pred_alt"][:, EDITED], d["conf_alt"][:, EDITED]
    pop1 = rc.traces_from_tables(pred1, conf1, truth, nxt)

    def dump(tr, path):
        with open(path, "w") as f:
            for i in range(len(tr)):
                steps = []
                for j in range(tr.path_node.shape[1]):
                    v = tr.path_node[i, j]
                    if v < 0:
                        break
                    correct = not (tr.dev_pos[i] >= 0 and j >= tr.dev_pos[i])
                    steps.append({"node": names[v], "confidence": round(float(tr.path_conf[i, j]), 6),
                                  "correct": bool(correct)})
                f.write(json.dumps({"steps": steps}) + "\n")

    rng = np.random.default_rng(0)
    cal = pop.subset(rng.integers(0, len(pop), N_TRACES))
    hit = np.where(pop1.through([EDITED]))[0]
    fresh = pop1.subset(hit[rng.integers(0, len(hit), N_FRESH)])
    for tr in (cal, fresh):  # the files store six decimals; certify exactly what is stored
        tr.path_conf = np.round(tr.path_conf, 6)
    dump(cal, os.path.join(OUT, "traces.jsonl"))
    dump(fresh, os.path.join(OUT, "fresh_intent_banking.jsonl"))

    # -- the graph ---------------------------------------------------------
    def decision(nid, question, choices, model="intent-encoder-v1"):
        return {"id": nid, "node_type": "decision", "kind": "processor",
                "config": {"question": question, "choices": choices, "provider": "input",
                           "scores_field": f"scores.{nid}", "model": model},
                "inputs": [{"name": "default"}],
                "outputs": [{"name": c} for c in choices] + [{"name": "uncertain"}]}

    def sink(nid, prefix):
        return {"id": nid, "node_type": "console", "kind": "sink", "config": {"prefix": prefix},
                "inputs": [{"name": "default"}], "outputs": []}

    nodes = [{"id": "request", "node_type": "file_read", "kind": "source",
              "config": {"path": "examples/triage_ticket.txt"}, "inputs": [], "outputs": [{"name": "default"}]},
             decision("scope", "Can the assistant handle this request?", ["in_scope", "out_of_scope"]),
             decision("domain", "Which product area is the request about?", domains)]
    edges = []

    def connect(a, port, b):
        edges.append({"id": f"e{len(edges) + 1}", "from_node": a, "from_port": port, "to_node": b,
                      "to_port": "default"})

    connect("request", "default", "scope")
    connect("scope", "in_scope", "domain")
    nodes.append(sink("out_of_scope_reply", "[out of scope] "))
    connect("scope", "out_of_scope", "out_of_scope_reply")
    nodes.append(sink("human_review", "[human review] "))
    connect("scope", "uncertain", "human_review")
    connect("domain", "uncertain", "human_review")
    for dom in domains:
        nid = f"intent_{dom}"
        choices = [f"{dom}_{j:02d}" for j in range(15)]
        nodes.append(decision(nid, f"Which {dom} intent is this?", choices))
        nodes.append(sink(f"handle_{dom}", f"[{dom}] "))
        connect("domain", dom, nid)
        for c in choices:
            connect(nid, c, f"handle_{dom}")
        connect(nid, "uncertain", "human_review")
    graph = {"version": "0.1",
             "metadata": {"name": "Assistant triage (CLINC150)",
                          "description": "Three levels of decisions: scope, product area, intent. Scores come "
                                         "from an upstream classifier; uncertain requests go to human review. "
                                         "Used to demonstrate `anp certify`."},
             "nodes": nodes, "edges": edges}
    with open(os.path.join(OUT, "workflow.anp.json"), "w") as f:
        json.dump(graph, f, indent=2)
    for n in graph["nodes"]:
        if n["id"] == names[EDITED]:
            n["config"]["model"] = "intent-encoder-v2"
    with open(os.path.join(OUT, "workflow_edited.anp.json"), "w") as f:
        json.dump(graph, f, indent=2)

    # -- reference numbers for the Rust implementation ---------------------
    grid = rc.default_grid()
    lv = levels(pop)
    ref = {"n": N_TRACES, "delta": 0.1, "grid_options": len(grid)}
    for label, modules in [("level", lv), ("node", None), ("global", [list(range(pop.n_nodes))])]:
        cert = rc.certify(cal, 0.1, grid, modules, mode="min")
        m_count = len(cert.modules)
        g07 = int(np.where(np.isclose(grid, 0.7))[0][0])
        best = rc.optimize(cert, 0.05)
        ref[label] = {"bound_at_0.7": cert.bound([g07] * m_count),
                      "errors_at_0.7": int(cert.parts[0].counts[:, g07].sum()),
                      "optimal_review_at_0.05": float(sum(cert.review[m, g] for m, g in enumerate(best))),
                      "optimal_bound_at_0.05": cert.bound(best)}
    cert_fd = rc.certify(cal, 0.1, grid, lv, mode="fd")
    ref["first_error_level_errors_at_0.7"] = int(cert_fd.parts[0].counts[:, g07].sum())
    with open(os.path.join(OUT, "reference.json"), "w") as f:
        json.dump(ref, f, indent=1)
    print(json.dumps(ref, indent=1))


if __name__ == "__main__":
    main()
