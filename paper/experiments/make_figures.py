"""Turn results/*.json into the figures and LaTeX tables of the paper."""
import json
import os

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

HERE = os.path.dirname(__file__)
RES = os.path.join(HERE, "..", "results")
FIG = os.path.join(HERE, "..", "figures")
TAB = os.path.join(HERE, "..", "tables")
os.makedirs(FIG, exist_ok=True)
os.makedirs(TAB, exist_ok=True)

plt.rcParams.update({"font.size": 8, "axes.spines.top": False, "axes.spines.right": False,
                     "legend.frameon": False, "figure.dpi": 150, "pdf.fonttype": 42})
TITLE = {"clinc": "CLINC150 (12 decisions)", "massive": "MASSIVE (21 decisions)",
         "clinc_llm": "LLM-3 (12 decisions)", "clinc_llm_inscope": "LLM-2 (11 decisions)",
         "clinc_llm_fs": "LLM-3-FS (12 decisions)", "clinc_llm_fs_inscope": "LLM-2-FS (11 decisions)",
         "clinc_llm_claude": "LLM-3-C (12 decisions)", "clinc_llm_claude_inscope": "LLM-2-C (11 decisions)",
         "kg_webqsp": "WebQSP (7 decisions)", "kg_cwq": "CWQ (9 decisions)"}
KIND = {"leaf": "leaf edit", "middle": "middle edit", "root": "root edit", "upgrade": "leaf, newer LLM",
        "classifier": "leaf, trained classifier", "answer": "answer step, newer LLM",
        "late": "last relation, newer LLM"}
STYLE = {  # label, colour, linestyle, marker
    "global-e2e": ("Shared threshold, end-to-end", "#7f7f7f", "--", "s"),
    "e2e-pareto": ("End-to-end Pareto testing", "#000000", "-", "o"),
    "e2e-uniform": ("End-to-end, uniform", "#bcbd22", ":", "v"),
    "fd-node": ("First-error, per node", "#d62728", "-.", "x"),
    "min-node": ("Least-confident, per node", "#1f77b4", "-", "^"),
    "min-level": ("Least-confident, per level", "#2ca02c", "-", "D"),
    "min-global": ("Least-confident, shared", "#9467bd", ":", "P"),
}


def load(name):
    """Results of an experiment, merged with its language-model counterpart if present."""
    path = os.path.join(RES, f"{name}.json")
    if not os.path.exists(path):
        return None
    res = json.load(open(path))
    extra = os.path.join(RES, f"{name}_llm.json")
    if os.path.exists(extra):
        res["data"].update(json.load(open(extra))["data"])
    return res


def pct(x, digits=1):
    return f"{100 * x:.{digits}f}"


def table_main(res):
    eps_list = sorted({key.split("|")[0] for d in res["data"].values() for key in d["results"]}, key=float)
    rows = [("fixed-0.7", "Fixed threshold 0.7", "none"),
            ("per-node-selective", "Per-node selective risk", "none"),
            ("plug-in", "Plug-in (no confidence bound)", "none"),
            ("global-e2e", "Shared threshold, end-to-end LTT", "fixed"),
            ("e2e-pareto", "End-to-end Pareto testing", "fixed"),
            ("e2e-uniform", "End-to-end, uniform over grid", "uniform"),
            ("tf-node", "Teacher-forced, per node", "uniform"),
            ("fd-node", "First-error, per node", "uniform"),
            ("fd-level", "First-error, per level", "uniform"),
            ("min-node", "Least-confident, per node", "uniform"),
            ("min-level", "Least-confident, per level", "uniform"),
            ("min-global", "Least-confident, shared", "uniform"),
            ("anchored-level", "Anchored, per level", "uniform")]
    for name, data in res["data"].items():
        agent = name.startswith("kg_")   # modules of an agent are the roles of its decisions
        lines = [r"\begin{tabular}{llrrrrrrrrr}", r"\toprule",
                 " & & " + " & ".join(rf"\multicolumn{{3}}{{c}}{{$\varepsilon={100 * float(e):.0f}\%$}}"
                                      for e in eps_list) + r" \\",
                 r"\cmidrule(lr){3-5}\cmidrule(lr){6-8}\cmidrule(lr){9-11}",
                 r"Method & Guarantee & viol. & risk & review & viol. & risk & review & viol. & risk & review \\",
                 r"\midrule"]
        for key, label, kind in rows:
            if f"{eps_list[0]}|{key}" not in data["results"]:
                continue
            label = label.replace("per level", "per role") if agent else label
            cells = []
            for e in eps_list:
                r = data["results"][f"{e}|{key}"]
                viol = pct(r["violation"], 0)
                if kind != "none" and r["violation"] > res["delta"]:
                    viol = r"\textbf{" + viol + "}"
                cells += [viol, pct(r["risk_mean"]), pct(r["review_mean"])]
            lines.append(f"{label} & {kind} & " + " & ".join(cells) + r" \\")
            if key in ("plug-in", "e2e-uniform", "fd-level"):
                lines.append(r"\midrule")
        lines += [r"\bottomrule", r"\end{tabular}"]
        open(os.path.join(TAB, f"main_{name}.tex"), "w").write("\n".join(lines) + "\n")


def fig_nsweep(res):
    fig, axes = plt.subplots(1, 2, figsize=(6.6, 2.9), sharey=True)
    for ax, (name, data) in zip(axes, res["data"].items()):
        sizes = sorted(data, key=int)
        for m, (label, colour, ls, marker) in STYLE.items():
            if m in data[sizes[0]]:
                ax.plot([int(s) for s in sizes], [100 * data[s][m]["review_mean"] for s in sizes],
                        color=colour, ls=ls, marker=marker, ms=3.5, lw=1.1, label=label)
        ax.set_xscale("log")
        ax.set_xticks([int(s) for s in sizes])
        ax.set_xticklabels(sizes)
        ax.minorticks_off()
        ax.set_xlabel("calibration runs $n$")
        ax.set_title(TITLE[name], fontsize=8)
    axes[0].set_ylabel("human review (%)")
    fig.legend(*axes[0].get_legend_handles_labels(), fontsize=6.5, loc="lower center", ncol=4)
    fig.tight_layout(rect=(0, 0.13, 1, 1))
    fig.savefig(os.path.join(FIG, "nsweep.pdf"))


def fig_posthoc(res):
    fig, axes = plt.subplots(1, 2, figsize=(6.6, 2.3), sharey=True)
    for ax, (name, data) in zip(axes, res["data"].items()):
        eps = [100 * e for e in data["eps"]]
        for m in ["global-e2e", "e2e-pareto", "min-node", "min-level"]:
            label, colour, ls, marker = STYLE[m]
            ax.plot(eps, [100 * r for r in data[m]["review"]], color=colour, ls=ls, marker=marker, ms=3.5,
                    lw=1.1, label=f"{label} ({pct(data[m]['any_violation'])}%)")
        ax.set_xlabel(r"target risk $\varepsilon$ (%)")
        ax.set_title(TITLE[name], fontsize=8)
        ax.set_ylim(top=ax.get_ylim()[1] * 1.3)
        ax.legend(fontsize=6.5, title="method (draws with any violation)", title_fontsize=6.5,
                  loc="upper right")
    axes[0].set_ylabel("human review (%)")
    fig.tight_layout()
    fig.savefig(os.path.join(FIG, "posthoc.pdf"))


def fig_edits(res, out="edits.pdf"):
    names = list(res["data"])
    fig, axes = plt.subplots(len(names), 3, figsize=(6.8, 1.7 * len(names)), sharey=True, squeeze=False)
    legend_done = False
    lines = {"scratch: e2e-pareto": ("From scratch: Pareto testing", "#000000", "-", "o"),
             "scratch: min-level": ("From scratch: least-confident, level", "#2ca02c", "--", "D"),
             "local: min-level": ("Edit-local (ours)", "#d62728", "-", "^")}
    if out != "edits.pdf":   # the modules of an agent are roles
        lines = {m: (label.replace(", level", ", role"), *rest) for m, (label, *rest) in lines.items()}
    for i, name in enumerate(names):
        kinds = list(res["data"][name]["edits"])
        for j in range(3):
            ax = axes[i][j]
            kind = kinds[j] if j < len(kinds) else None
            info = res["data"][name]["edits"].get(kind)
            if info is None:
                ax.axis("off")
                if not legend_done:
                    ax.legend(*axes[0][0].get_legend_handles_labels(), fontsize=6.5, loc="center")
                    legend_done = True
                continue
            sizes = sorted(info["sizes"], key=int)
            for m, (label, colour, ls, marker) in lines.items():
                ax.plot([int(s) for s in sizes], [100 * info["sizes"][s][m]["review_mean"] for s in sizes],
                        color=colour, ls=ls, marker=marker, ms=3.5, lw=1.1, label=label)
            ref = info["sizes"][sizes[-1]]["rerun: min-level"]["review_mean"]
            ax.axhline(100 * ref, color="#7f7f7f", lw=0.8, ls=":", label="Re-run all stored inputs")
            ax.set_xscale("log")
            ax.set_xticks([int(s) for s in sizes])
            ax.set_xticklabels(sizes, fontsize=6.5)
            ax.minorticks_off()
            ax.set_title(f"{TITLE[name].split(' ')[0]}: {KIND[kind]} ({pct(info['traffic_share'], 0)}%)",
                         fontsize=7.5)
            if i == len(names) - 1:
                ax.set_xlabel("fresh labelled runs $m$")
            if j == 0:
                ax.set_ylabel("human review (%)")
    fig.tight_layout()
    fig.savefig(os.path.join(FIG, out))
    if out != "edits.pdf":
        return

    rows = []
    for name in names:
        for kind, info in res["data"][name]["edits"].items():
            rows.append(f"{TITLE[name].split(' ')[0]} & {KIND[kind]} & {pct(info['traffic_share'], 0)} & "
                        f"{len(info['tf_changed'])} & {len(info['fd_changed'])} ({len(info['fd_predicted'])}) & "
                        f"{len(info['min_changed'])} \\\\")
    open(os.path.join(TAB, "footprints.tex"), "w").write(
        "\\begin{tabular}{llrrrr}\n\\toprule\n"
        "Workflow & Edited node & Traffic (\\%) & TF & FE & LC \\\\\n\\midrule\n" + "\n".join(rows)
        + "\n\\bottomrule\n\\end{tabular}\n")


def fig_synth(res):
    data = res["data"]
    ks = sorted(data, key=int)
    ns = sorted(data[ks[0]], key=int)
    fig, axes = plt.subplots(1, len(ns), figsize=(6.8, 2.8), sharey=True)
    for ax, n in zip(axes, ns):
        for m in ["global-e2e", "e2e-pareto", "fd-node", "min-node", "min-global"]:
            label, colour, ls, marker = STYLE[m]
            ax.plot([int(k) for k in ks], [100 * data[k][n][m]["review_mean"] for k in ks], color=colour, ls=ls,
                    marker=marker, ms=3.5, lw=1.1, label=label)
        ax.set_xscale("log", base=2)
        ax.set_xticks([int(k) for k in ks])
        ax.set_xticklabels(ks)
        ax.set_xlabel("decisions per run $K$")
        ax.set_title(f"$n={int(n):,}$".replace(",", "{,}"), fontsize=8)
    axes[0].set_ylabel("human review (%)")
    fig.legend(*axes[0].get_legend_handles_labels(), fontsize=6.5, loc="lower center", ncol=3)
    fig.tight_layout(rect=(0, 0.14, 1, 1))
    fig.savefig(os.path.join(FIG, "synth.pdf"))


def table_ablation(res):
    groups = [("Confidence bound", "bound", [("one pooled binomial (default)", "pooled"),
                                             ("one band per module", "separate")]),
              ("Threshold search", "search", [("knapsack only", "knapsack only"),
                                              ("knapsack and refinement (default)", "with refinement")]),
              ("Grid size $G$", "grid", [("13", "coarse (13)"), ("26 (default)", "default (26)"),
                                         ("103", "fine (103)")]),
              ("Confidence $1-\\delta$", "delta", [("0.99", "0.01"), ("0.95", "0.05"), ("0.90 (default)", "0.1"),
                                                   ("0.80", "0.2")])]
    names = [n for n in ("clinc", "massive") if n in res["data"]]
    lines = ["\\begin{tabular}{ll" + "rr" * len(names) + "}", "\\toprule",
             " & & " + " & ".join(f"\\multicolumn{{2}}{{c}}{{{TITLE[n].split(' ')[0]}}}" for n in names) + " \\\\",
             "".join(f"\\cmidrule(lr){{{3 + 2 * i}-{4 + 2 * i}}}" for i in range(len(names))),
             "Design choice & Setting & " + " & ".join("per node & per level" for _ in names) + " \\\\",
             "\\midrule"]
    worst = 0.0
    for gi, (title, key, rows) in enumerate(groups):
        for ri, (label, setting) in enumerate(rows):
            cells = []
            for n in names:
                for modules in ("node", "level"):
                    r = res["data"][n][f"{key}|{modules}|{setting}"]
                    cells.append(pct(r["review_mean"]))
                    worst = max(worst, r["violation"])
            lines.append(f"{title if ri == 0 else ''} & {label} & " + " & ".join(cells) + " \\\\")
        if gi < len(groups) - 1:
            lines.append("\\midrule")
    lines += ["\\bottomrule", "\\end{tabular}"]
    open(os.path.join(TAB, "ablation.tex"), "w").write("\n".join(lines) + "\n")
    return worst


def numbers():
    """numbers.tex: every figure quoted in the prose, as a LaTeX macro."""
    out = {}
    ds = {"clinc": "C", "massive": "M", "clinc_llm": "T", "clinc_llm_inscope": "L", "clinc_llm_fs": "G",
          "clinc_llm_fs_inscope": "F", "clinc_llm_claude": "H", "clinc_llm_claude_inscope": "K"}
    meth = {"fixed-0.7": "Fixed", "per-node-selective": "PerNode", "plug-in": "Plug", "global-e2e": "Shared",
            "e2e-pareto": "Pareto", "e2e-uniform": "Unif", "tf-node": "TFnode", "fd-node": "FEnode",
            "fd-level": "FElevel", "min-node": "LCnode", "min-level": "LClevel", "min-global": "LCshared",
            "anchored-node": "AnchNode", "anchored-level": "Anch"}
    eps_name = {"0.02": "Two", "0.05": "Five", "0.1": "Ten"}
    main = load("main")
    if main:
        out["Trials"] = str(main["trials"])
        ratios = []
        for name, d in main["data"].items():
            tag = ds[name]
            out["FreeErr" + {"C": "Clinc", "M": "Massive", "L": "Llm", "T": "LlmThree", "F": "LlmFs",
                             "G": "LlmThreeFs", "H": "LlmThreeClaude", "K": "LlmClaude"}[tag]] = pct(
                d["free_run_error"])
            for e, en in eps_name.items():
                out[f"OracleNode{tag}{en}"] = pct(d["oracle"][e if e != "0.1" else "0.1"]["node"])
                out[f"OracleShared{tag}{en}"] = pct(d["oracle"][e if e != "0.1" else "0.1"]["shared"])
                for m, mn in meth.items():
                    r = d["results"][f"{e}|{m}"]
                    out[f"Rev{tag}{mn}{en}"] = pct(r["review_mean"])
                    out[f"Risk{tag}{mn}{en}"] = pct(r["risk_mean"])
                    out[f"Viol{tag}{mn}{en}"] = pct(r["violation"], 0)
            r = d["results"]["0.05|global-e2e"]
            if r["risk_mean"] > 0:  # undefined when nothing is committed
                ratio = r["fd_mass_mean"] / r["risk_mean"]
                if tag in "CM":
                    ratios.append(ratio)
                out[f"FdRatio{tag}"] = f"{ratio:.1f}"
                out[f"FdMass{tag}"] = pct(r["fd_mass_mean"])
                out[f"RescuedShare{tag}"] = pct(1 - r["risk_mean"] / r["fd_mass_mean"], 0)
        out["FdOverstateMin"], out["FdOverstateMax"] = f"{min(ratios):.1f}", f"{max(ratios):.1f}"
        se = lambda keep: f"{100 * max(r['review_sd'] / r['trials'] ** 0.5 for n in ('clinc', 'massive') for k, r in main['data'][n]['results'].items() if keep(k)):.1f}"
        out["MaxSeReview"] = se(lambda k: True)
        out["MaxSeReviewFive"] = se(lambda k: k.startswith("0.05|"))
        certified = [m for m in meth if m not in ("fixed-0.7", "per-node-selective", "plug-in")]
        worst = lambda names: pct(max(main["data"][n]["results"][f"{e}|{m}"]["violation"]
                                      for n in names for e in eps_name for m in certified),
                                  1 if names[0].startswith("clinc_llm") else 0)
        out["MaxViolCertified"] = worst(["clinc", "massive"])
        llm = [n for n in main["data"] if n.startswith("clinc_llm")]
        if llm:
            out["MaxViolCertifiedLlm"] = worst(llm)
            uniform = [m for m in certified if m not in ("global-e2e", "e2e-pareto")]
            out["MaxViolUniformLlm"] = pct(max(main["data"][n]["results"][f"{e}|{m}"]["violation"]
                                               for n in llm for e in eps_name for m in uniform))
    post = load("posthoc")
    if post:
        for name, d in post["data"].items():
            for m, mn in [("min-level", "LClevel"), ("min-node", "LCnode"), ("global-e2e", "Shared"),
                          ("e2e-pareto", "Pareto")]:
                out[f"AnyViol{ds[name]}{mn}"] = pct(d[m]["any_violation"])
        out["PosthocTrials"] = str(post["trials"])
    edits = load("edits")
    if edits:
        size_name = {"100": "Hundred", "250": "TwoFifty", "500": "FiveHundred", "1000": "Thousand"}
        mm = {"scratch: e2e-pareto": "Scratch", "scratch: min-level": "ScratchLC", "local: min-level": "Local",
              "local: min-node": "LocalNode", "rerun: e2e-pareto": "RerunPareto", "rerun: min-level": "Rerun"}
        worst = 0.0
        for name, d in edits["data"].items():
            for kind, info in d["edits"].items():
                out[f"EditShare{ds[name]}{kind.capitalize()}"] = pct(info["traffic_share"], 0)
                for size, sn in size_name.items():
                    for m, mn in mm.items():
                        r = info["sizes"][size][m]
                        out[f"Edit{ds[name]}{kind.capitalize()}{mn}{sn}"] = pct(r["review_mean"])
                        if mn.startswith("Local"):
                            worst = max(worst, r["violation"])
        out["EditMaxViolLocal"] = pct(worst, 0)
        out["EditM"] = "100"
        for name, key in [("clinc", "EditLocalClinc"), ("massive", "EditLocalMassive")]:
            leaf = edits["data"][name]["edits"]["leaf"]["sizes"]["100"]["local: min-level"]
            out[key] = pct(leaf["review_mean"], 0)
        out["EditTrials"] = str(edits["trials"])
    stats = load("llmstats")
    if stats:
        thousands = lambda x: f"{x:,}".replace(",", "{,}")
        for name, d in stats["data"].items():
            tag = ds[name]
            out[f"LlmSaturated{tag}"] = pct(d["saturated_share"], 0)
            out[f"LlmRootAcc{tag}"] = pct(d["root_accuracy"])
            out[f"LlmRootWrongSat{tag}"] = pct(d["root_wrong_saturated"])
            out[f"LlmDefaultRisk{tag}"] = pct(d["at_default"]["risk"])
            out[f"LlmDefaultReview{tag}"] = pct(d["at_default"]["review"])
            out[f"LlmTopRisk{tag}"] = pct(d["at_top"]["risk"])
            out[f"LlmTopReview{tag}"] = pct(d["at_top"]["review"])
            if "root_only" in d:
                out[f"LlmRootOnlyRisk{tag}"] = pct(d["root_only"]["risk"])
                out[f"LlmRootOnlyReview{tag}"] = pct(d["root_only"]["review"])
            out[f"LlmSize{tag}"] = thousands(d["N"])
        d = stats["data"]["clinc_llm"]
        out["LlmModel"], out["LlmUpgrade"] = d["model"], d["upgrade"]
        out["LlmRequests"], out["LlmUpgradeRequests"] = thousands(d["model_requests"]), thousands(d["upgrade_requests"])
        out["LlmUnparsed"] = str(d["model_unparsed"])
        if "clinc_llm_fs" in stats["data"]:
            out["LlmFsRequests"] = thousands(stats["data"]["clinc_llm_fs"]["model_requests"])
            out["LlmFsUnparsed"] = str(stats["data"]["clinc_llm_fs"]["model_unparsed"])
    ab = load("ablation")
    if ab:
        out["AblationTrials"] = str(ab["trials"])
        out["AblationMaxViol"] = pct(max(r["violation"] for d in ab["data"].values() for r in d.values()))
        for name, d in ab["data"].items():
            for modules, mn in [("node", "Node"), ("level", "Level")]:
                for key, kn in [("bound|{}|pooled", "Pooled"), ("bound|{}|separate", "Separate"),
                                ("search|{}|knapsack only", "Knapsack"), ("search|{}|with refinement", "Refined"),
                                ("grid|{}|coarse (13)", "GridCoarse"), ("grid|{}|default (26)", "GridDefault"),
                                ("grid|{}|fine (103)", "GridFine"), ("delta|{}|0.01", "DeltaStrict"),
                                ("delta|{}|0.2", "DeltaLoose")]:
                    out[f"Ab{ds[name]}{mn}{kn}"] = pct(d[key.format(modules)]["review_mean"])
    ns = load("nsweep")
    if ns:
        out["NsweepTrials"] = str(ns["trials"])
        for name, d in ns["data"].items():
            sizes = sorted(d, key=int)
            for size, sn in [(sizes[0], "Small"), (sizes[-1], "Large")]:
                for m, mn in [("global-e2e", "Shared"), ("e2e-pareto", "Pareto"), ("min-node", "LCnode"),
                              ("min-level", "LClevel"), ("min-global", "LCshared")]:
                    out[f"N{ds[name]}{sn}{mn}"] = pct(d[size][m]["review_mean"])
    sy = load("synth")
    if sy:
        out["SynthTrials"] = str(sy["trials"])
        for k, kn in [("2", "Two"), ("32", "ThirtyTwo")]:
            for n, nn in [("1000", "Small"), ("4000", "Mid"), ("16000", "Large")]:
                for m, mn in [("min-node", "LCnode"), ("e2e-pareto", "Pareto"), ("min-global", "LCshared"),
                              ("fd-node", "FEnode")]:
                    out[f"Synth{kn}{nn}{mn}"] = pct(sy["data"][k][n][m]["review_mean"])
        out["SynthFEmin"] = pct(min(v["fd-node"]["review_mean"] for d in sy["data"].values() for v in d.values()), 0)
    agent = load("main_agent")
    if agent:
        tags = {"kg_webqsp": "W", "kg_cwq": "Q"}
        eps_agent = {"0.05": "Five", "0.1": "Ten", "0.2": "Twenty"}
        thousands = lambda x: f"{x:,}".replace(",", "{,}")
        viol = []
        for name, d in agent["data"].items():
            tag = tags[name]
            out[f"AgentSize{tag}"] = thousands(d["N"])
            out[f"AgentNodes{tag}"] = str(d["K"])
            out[f"AgentBudget{tag}"] = str(d["budget"])
            out[f"AgentErr{tag}"] = pct(d["free_run_error"])
            out[f"AgentSuccess{tag}"] = pct(1 - d["free_run_error"])
            out[f"AgentDecisions{tag}"] = f"{d['decisions_mean']:.1f}"
            out[f"AgentDecisionsMax{tag}"] = str(d["decisions_max"])
            out[f"AgentRoutes{tag}"] = str(d["routes"])
            out[f"AgentSaturated{tag}"] = pct(d["saturated_share"], 0)
            out[f"AgentDefaultRisk{tag}"] = pct(d["at_default"]["risk"])
            out[f"AgentDefaultReview{tag}"] = pct(d["at_default"]["review"])
            out[f"AgentTopRisk{tag}"] = pct(d["at_top"]["risk"])
            out[f"AgentTopReview{tag}"] = pct(d["at_top"]["review"])
            if "goes_on" in d:
                out[f"AgentGoesOn{tag}"] = pct(d["goes_on"], 0)
                out[f"AgentLeastLater{tag}"] = pct(d["least_confident_later"], 0)
                out[f"AgentConfidentSound{tag}"] = pct(d["confident_sound"], 0)
                out[f"AgentConfidentAfter{tag}"] = pct(d["confident_after"], 0)
            for e, en in eps_agent.items():
                out[f"OracleShared{tag}{en}"] = pct(d["oracle"][e]["shared"])
                out[f"OracleNode{tag}{en}"] = pct(d["oracle"][e]["node"])
                for m, mn in meth.items():
                    r = d["results"].get(f"{e}|{m}")
                    if r is None:
                        continue
                    out[f"Rev{tag}{mn}{en}"] = pct(r["review_mean"])
                    out[f"Risk{tag}{mn}{en}"] = pct(r["risk_mean"])
                    out[f"Viol{tag}{mn}{en}"] = pct(r["violation"], 0)
                    if m not in ("fixed-0.7", "plug-in"):
                        viol.append(r["violation"])
                r = d["results"][f"{e}|global-e2e"]
                if r["risk_mean"] > 0:
                    out[f"FdRatio{tag}{en}"] = f"{r['fd_mass_mean'] / r['risk_mean']:.1f}"
                    out[f"FdMass{tag}{en}"] = pct(r["fd_mass_mean"])
                    out[f"RescuedShare{tag}{en}"] = pct(1 - r["risk_mean"] / r["fd_mass_mean"], 0)
        out["AgentModel"] = next(iter(agent["data"].values()))["model"]
        out["AgentSkipped"] = str(sum(d["skipped"] for d in agent["data"].values()))
        out["AgentTrials"] = str(agent["trials"])
        out["MaxViolCertifiedAgent"] = pct(max(viol), 1)
        for model, count in agent.get("requests", {}).items():
            out["AgentRequests" + ("Base" if model == out["AgentModel"] else "Upgrade")] = thousands(count)
    post = load("posthoc_agent")
    if post:
        for name, d in post["data"].items():
            for m, mn in [("min-level", "LClevel"), ("min-node", "LCnode"), ("global-e2e", "Shared"),
                          ("e2e-pareto", "Pareto")]:
                out[f"AnyViol{tags[name]}{mn}"] = pct(d[m]["any_violation"])
    edits = load("edits_agent")
    if edits:
        worst = 0.0
        for name, d in edits["data"].items():
            out["AgentEditEps"] = pct(d["eps"], 0)
            for kind, info in d["edits"].items():
                key = tags[name] + kind.capitalize()
                out[f"EditShare{key}"] = pct(info["traffic_share"], 0)
                out[f"EditErrBefore{key}"] = pct(info["error_before"])
                out[f"EditErrAfter{key}"] = pct(info["error_after"])
                for size, sn in size_name.items():
                    for m, mn in mm.items():
                        r = info["sizes"][size][m]
                        out[f"Edit{key}{mn}{sn}"] = pct(r["review_mean"])
                        if mn.startswith("Local"):
                            worst = max(worst, r["violation"])
        out["EditMaxViolLocalAgent"] = pct(worst, 0)
    cov = load("coverage")
    if cov:
        thousands = lambda x: f"{x:,}".replace(",", "{,}")
        cert, recert, single = cov["certificate"], cov["recertified"], cov["single"]
        out["CoverageDraws"] = thousands(cov["draws"])
        out["CoverageCertConfigs"] = str(len(cert))
        out["CoverageRecertConfigs"] = str(len(recert))
        out["CoverageAssignMin"] = thousands(min(x["assignments"] for x in cert.values()))
        out["CoverageAssignMax"] = thousands(max(x["assignments"] for x in cert.values()))
        out["CoverageCertViolations"] = str(sum(x["violations"] for x in cert.values()))
        out["CoverageCertDraws"] = thousands(sum(x["draws"] for x in cert.values()))
        out["CoverageCertWorst"] = pct(max(x["violation_rate"] for x in cert.values()))
        out["CoverageRecertViolations"] = str(sum(x["violations"] for x in recert.values()))
        out["CoverageRecertDraws"] = thousands(sum(x["draws"] for x in recert.values()))
        out["CoverageSingleDraws"] = thousands(next(iter(single.values()))["draws"])
        out["CoverageSingleMeanLow"] = pct(min(x["mean_violation_rate"] for x in single.values()))
        out["CoverageSingleMeanHigh"] = pct(max(x["mean_violation_rate"] for x in single.values()))
        out["CoverageSingleLargest"] = pct(max(x["largest_violation_rate"] for x in single.values()))
    with open(os.path.join(HERE, "..", "numbers.tex"), "w") as f:
        f.write("% Generated by experiments/make_figures.py from results/*.json. Do not edit.\n")
        for key in sorted(out):
            f.write(f"\\newcommand{{\\{key}}}{{{out[key]}}}\n")
    print("numbers:", len(out), "macros")


if __name__ == "__main__":
    numbers()

    for name, fn in [("main", table_main), ("nsweep", fig_nsweep), ("posthoc", fig_posthoc),
                     ("edits", fig_edits), ("synth", fig_synth), ("ablation", table_ablation),
                     ("main_agent", table_main), ("edits_agent", lambda r: fig_edits(r, "edits_agent.pdf"))]:
        res = load(name)
        if res is None:
            print("missing", name)
            continue
        fn(res)
        print("done", name)
