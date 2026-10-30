#!/usr/bin/env python3
"""Build reproducible, data-backed artwork research figures for OpenWorldGen.

No values are synthesized here: every mark comes from the result bundles, the
Philadelphia Fed vintage backtest, Census BDS evaluation-only data, or the
reserve-liquidity engineering diagnostic named in SOURCE_MAPPING.md.
"""
from __future__ import annotations

import csv
import json
import math
import shutil
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.colors import LinearSegmentedColormap
from matplotlib.ticker import FuncFormatter
import numpy as np
from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "outputs" / "artwork-economic-research-dashboard-2026-10-21"
INDIVIDUAL = OUT / "individual"

# Reusable Vercel-inspired research-terminal visual system.
BG, PANEL, TEXT, SECONDARY, MINOR, GRID, BORDER = (
    "#000000", "#080B10", "#E5E7EB", "#9CA3AF", "#697586", "#20262F", "#313A46"
)
BLUE, TEAL, GREEN, VIOLET, ORANGE, RED, NEUTRAL = (
    "#70A5E8", "#6BC5AA", "#80B889", "#C4A0E8", "#E6A36B", "#D97979", "#AAB4C3"
)
BRANCH_COLORS = {
    "baseline": NEUTRAL, "combined": BLUE, "essential-tax-cut": TEAL,
    "monetary-tightening": ORANGE, "public-investment": GREEN,
    "targeted-transfers": VIOLET,
}
BRANCH_LABELS = {
    "baseline": "BASELINE", "combined": "COMBINED", "essential-tax-cut": "ESSENTIAL TAX CUT",
    "monetary-tightening": "MONETARY TIGHTENING", "public-investment": "PUBLIC INVESTMENT",
    "targeted-transfers": "TARGETED TRANSFERS",
}
FONT = "DejaVu Sans Mono"


def load_csv(path: Path):
    with path.open(newline="") as f:
        return list(csv.DictReader(f))


def f(row, key, default=np.nan):
    value = row.get(key, "")
    try:
        return float(value) if value not in (None, "") else default
    except (TypeError, ValueError):
        return default


def setup():
    plt.rcParams.update({
        "font.family": FONT, "font.size": 8.2, "axes.titleweight": "regular",
        "figure.facecolor": BG, "savefig.facecolor": BG, "axes.facecolor": PANEL,
        "text.color": TEXT, "axes.labelcolor": SECONDARY, "xtick.color": SECONDARY,
        "ytick.color": SECONDARY, "axes.edgecolor": BORDER, "axes.linewidth": 0.55,
        "legend.frameon": False, "svg.fonttype": "none", "savefig.bbox": "tight",
        "savefig.pad_inches": 0.04,
    })


def fig_axes(nrows=1, ncols=1, *, figsize=(10.8, 6.1), **kwargs):
    fig, axes = plt.subplots(nrows, ncols, figsize=figsize, **kwargs)
    axes = np.atleast_1d(axes).ravel()
    for ax in axes:
        ax.set_facecolor(PANEL)
        ax.grid(True, color=GRID, linewidth=0.42, alpha=0.8, zorder=0)
        ax.set_axisbelow(True)
        ax.tick_params(length=2.5, width=0.55, pad=3)
        ax.spines[["top", "right"]].set_visible(False)
        ax.spines[["left", "bottom"]].set_color(BORDER)
        ax.spines[["left", "bottom"]].set_linewidth(0.55)
    return fig, axes


def header(ax, title, subtitle, xlab="SIMULATION MONTH", ylab=None):
    ax.set_title(title, loc="left", fontsize=10.0, color=TEXT, pad=14)
    ax.text(0.0, 1.012, subtitle, transform=ax.transAxes, fontsize=6.7, color=MINOR,
            va="bottom", ha="left")
    if xlab:
        ax.set_xlabel(xlab, fontsize=7.4, labelpad=6)
    if ylab:
        ax.set_ylabel(ylab, fontsize=7.4, labelpad=6)


PLOTS = []
# Composite selection only. Individual exports remain the full 19-panel evidence set.
DASHBOARD_SLUGS = [
    "01_real_output_policy_paths",
    "02_inflation_policy_response",
    "03_labor_output_strain",
    "04_cpi_component_mechanism",
    "05_consumption_decile_matrix",
    "06_distributional_dynamics",
    "07_firm_credit_channel",
    "10_firm_churn_event_stream",
    "11_policy_objective_frontier",
    "12_policy_regret_matrix",
    "14_bds_job_flows",
    "16_bds_sector_job_creation_heatmap",
    "17_vintage_backtest_prediction_fit",
    "18_vintage_backtest_error_diagnostics",
    "19_reserve_liquidity_diagnostic",
]
def finish(fig, slug, title, source, kind):
    png = INDIVIDUAL / f"{slug}.png"
    svg = INDIVIDUAL / f"{slug}.svg"
    fig.savefig(png, dpi=240)
    fig.savefig(svg)
    plt.close(fig)
    PLOTS.append({"slug": slug, "title": title, "source": source, "kind": kind, "png": png})


def fmt_millions(x, _):
    return f"{x / 1_000_000:.1f}M"


def legend(ax, *, ncol=2, loc="upper right"):
    ax.legend(loc=loc, ncol=ncol, fontsize=6.1, labelcolor=TEXT, handlelength=1.55,
              columnspacing=0.8, borderaxespad=0.45)


def branch_rows(rows):
    out = {}
    for r in rows:
        out.setdefault(r["branch_id"], []).append(r)
    return {k: sorted(v, key=lambda z: f(z, "month")) for k, v in out.items()}


def generate():
    setup()
    if OUT.exists():
        for p in INDIVIDUAL.glob("*"):
            if p.is_file(): p.unlink()
    INDIVIDUAL.mkdir(parents=True, exist_ok=True)

    sim = load_csv(ROOT / "outputs/development-smoke-v2-2026-07-24/metrics.csv")
    groups = branch_rows(sim)
    baseline = groups["baseline"]
    comparison = json.loads((ROOT / "outputs/development-smoke-v2-2026-07-24/comparison.json").read_text())
    mechanisms = json.loads((ROOT / "outputs/development-smoke-v2-2026-07-24/mechanisms.json").read_text())
    backtest = load_csv(ROOT / "research/us-economy-2007-2023/artifacts/backtest_philadelphia_2007_2023.csv")
    bds = load_csv(ROOT / "research/us-economy-2007-2023/data/processed/census-bds/evaluation-only/bds2023_economy_2007_2023.csv")
    sector = load_csv(ROOT / "research/us-economy-2007-2023/data/processed/census-bds/evaluation-only/bds2023_sector_2007_2023.csv")
    reserve = load_csv(ROOT / "research/us-economy-2007-2023/artifacts/synthetic_reserve_liquidity_12m_20261018/observations.csv")

    # 01 — Conditional simulation dynamics.
    fig, (ax,) = fig_axes(figsize=(10.8, 5.9))
    for branch, rs in groups.items():
        ax.plot([f(r, "month") for r in rs], [f(r, "real_gdp") / 1000 for r in rs],
                color=BRANCH_COLORS[branch], lw=1.25, label=BRANCH_LABELS[branch])
    header(ax, "REAL OUTPUT TRAJECTORIES", "CONDITIONAL DEVELOPMENT SIMULATION • SIX POLICY BRANCHES • 2020–2029", ylab="REAL GDP (THOUSANDS)")
    legend(ax, ncol=3)
    finish(fig, "01_real_output_policy_paths", "Real output trajectories", "metrics.csv (six simulation branches)", "multi-series line")

    # 02 — Inflation / policy-rate response.
    fig, (ax,) = fig_axes(figsize=(10.8, 5.9))
    for branch, rs in groups.items():
        ax.plot([f(r, "month") for r in rs], [100 * f(r, "inflation_year_over_year") for r in rs],
                color=BRANCH_COLORS[branch], lw=1.05, label=BRANCH_LABELS[branch])
    ax.plot([f(r, "month") for r in baseline], [100 * f(r, "policy_rate") for r in baseline], color=TEXT,
            lw=0.9, ls="--", label="BASELINE POLICY RATE")
    header(ax, "INFLATION RESPONSE & POLICY RATE", "ANNUAL INFLATION; BASELINE POLICY RATE OVERLAID • PERCENT", ylab="PERCENT")
    ax.axhline(0, color=MINOR, lw=0.65); legend(ax, ncol=2)
    finish(fig, "02_inflation_policy_response", "Inflation response and policy rate", "metrics.csv (six simulation branches)", "multi-series line")

    # 03 — Labor / output strain.
    fig, axes = fig_axes(1, 2, figsize=(10.8, 5.2))
    for branch, rs in groups.items():
        x = [f(r, "month") for r in rs]
        axes[0].plot(x, [100 * f(r, "output_gap") for r in rs], color=BRANCH_COLORS[branch], lw=1.0)
        axes[1].plot(x, [100 * f(r, "unemployment") for r in rs], color=BRANCH_COLORS[branch], lw=1.0, label=BRANCH_LABELS[branch])
    header(axes[0], "OUTPUT GAP", "SIX BRANCHES", ylab="PERCENT")
    header(axes[1], "UNEMPLOYMENT", "SIX BRANCHES", ylab="PERCENT")
    axes[0].axhline(0, color=MINOR, lw=0.65); legend(axes[1], ncol=2)
    fig.suptitle("LABOR-MARKET / OUTPUT STRAIN", x=0.09, ha="left", y=1.01, fontsize=10.0, color=TEXT)
    finish(fig, "03_labor_output_strain", "Labor-market and output strain", "metrics.csv (six simulation branches)", "paired multi-series line")

    # 04 — CPI mechanism decomposition.
    fig, (ax,) = fig_axes(figsize=(10.8, 5.9))
    component = [("cpi", "ALL ITEMS", NEUTRAL), ("cpi_energy", "ENERGY", ORANGE), ("cpi_food", "FOOD", GREEN),
                 ("cpi_manufacturing", "MANUFACTURING", BLUE), ("cpi_services", "SERVICES", VIOLET)]
    for key, label, color in component:
        vals = np.array([f(r, key) for r in baseline]); vals = 100 * vals / vals[0]
        ax.plot([f(r, "month") for r in baseline], vals, color=color, lw=1.35, label=label)
    header(ax, "CPI COMPONENT MECHANISM", "BASELINE BRANCH • INDEXED TO MONTH 0 = 100", ylab="INDEX")
    legend(ax, ncol=3)
    finish(fig, "04_cpi_component_mechanism", "CPI component mechanism", "metrics.csv (baseline simulation branch)", "indexed component line")

    # 05 — Consumption distribution (real simulation deciles).
    fig, (ax,) = fig_axes(figsize=(10.8, 5.9))
    matrix = np.array([[f(r, f"consumption_decile_{d}") for r in baseline] for d in range(1, 11)])
    cmap = LinearSegmentedColormap.from_list("terminal", [PANEL, "#173755", BLUE, "#B7D7F5"])
    im = ax.imshow(matrix, aspect="auto", cmap=cmap, interpolation="nearest")
    ax.set_yticks(range(10), [f"D{d:02d}" for d in range(1, 11)])
    ax.set_xticks(np.arange(0, 120, 12), [str(int(x)) for x in np.arange(0, 120, 12)])
    header(ax, "CONSUMPTION DISTRIBUTION", "BASELINE BRANCH • DECILE-SPECIFIC MONTHLY CONSUMPTION", ylab="INCOME DECILE")
    cbar = fig.colorbar(im, ax=ax, pad=0.02); cbar.outline.set_edgecolor(BORDER); cbar.ax.tick_params(colors=SECONDARY, labelsize=6.4); cbar.set_label("CONSUMPTION", color=SECONDARY, fontsize=6.7)
    finish(fig, "05_consumption_decile_matrix", "Consumption distribution matrix", "metrics.csv (baseline simulation branch)", "heatmap")

    # 06 — Inequality dynamics.
    fig, axes = fig_axes(1, 2, figsize=(10.8, 5.2))
    for branch, rs in groups.items():
        x = [f(r, "month") for r in rs]
        axes[0].plot(x, [f(r, "gini") for r in rs], color=BRANCH_COLORS[branch], lw=1.05)
        axes[1].plot(x, [100 * f(r, "poverty_share") for r in rs], color=BRANCH_COLORS[branch], lw=1.05, label=BRANCH_LABELS[branch])
    header(axes[0], "GINI COEFFICIENT", "SIX POLICY BRANCHES", ylab="GINI")
    header(axes[1], "POVERTY SHARE", "SIX POLICY BRANCHES", ylab="PERCENT")
    legend(axes[1], ncol=2)
    fig.suptitle("DISTRIBUTIONAL DYNAMICS", x=0.09, ha="left", y=1.01, fontsize=10.0, color=TEXT)
    finish(fig, "06_distributional_dynamics", "Distributional dynamics", "metrics.csv (six simulation branches)", "paired multi-series line")

    # 07 — Credit / active firms.
    fig, axes = fig_axes(1, 2, figsize=(10.8, 5.2))
    for branch, rs in groups.items():
        x = [f(r, "month") for r in rs]
        axes[0].plot(x, [f(r, "credit") / 1000 for r in rs], color=BRANCH_COLORS[branch], lw=1.05)
        axes[1].plot(x, [f(r, "active_firms") for r in rs], color=BRANCH_COLORS[branch], lw=1.05, label=BRANCH_LABELS[branch])
    header(axes[0], "CREDIT STOCK", "SIX POLICY BRANCHES", ylab="CREDIT (THOUSANDS)")
    header(axes[1], "ACTIVE FIRMS", "SIX POLICY BRANCHES", ylab="FIRM COUNT")
    legend(axes[1], ncol=2)
    fig.suptitle("FIRM / CREDIT CHANNEL", x=0.09, ha="left", y=1.01, fontsize=10.0, color=TEXT)
    finish(fig, "07_firm_credit_channel", "Firm and credit channel", "metrics.csv (six simulation branches)", "paired multi-series line")

    # 08 — Public finance dynamics.
    fig, axes = fig_axes(1, 2, figsize=(10.8, 5.2))
    for branch, rs in groups.items():
        x = [f(r, "month") for r in rs]
        axes[0].plot(x, [f(r, "fiscal_balance") / 1000 for r in rs], color=BRANCH_COLORS[branch], lw=1.05)
        axes[1].plot(x, [f(r, "government_debt") / 1000 for r in rs], color=BRANCH_COLORS[branch], lw=1.05, label=BRANCH_LABELS[branch])
    header(axes[0], "FISCAL BALANCE", "SIX POLICY BRANCHES", ylab="THOUSANDS")
    axes[0].axhline(0, color=MINOR, lw=0.65)
    header(axes[1], "GOVERNMENT DEBT", "SIX POLICY BRANCHES", ylab="THOUSANDS")
    legend(axes[1], ncol=2)
    fig.suptitle("PUBLIC-FINANCE DYNAMICS", x=0.09, ha="left", y=1.01, fontsize=10.0, color=TEXT)
    finish(fig, "08_public_finance_dynamics", "Public-finance dynamics", "metrics.csv (six simulation branches)", "paired multi-series line")

    # 09 — Sector balance sheet positions.
    fig, (ax,) = fig_axes(figsize=(10.8, 5.9))
    nfa = [("household_nfa_minor_units", "HOUSEHOLDS", BLUE), ("firm_nfa_minor_units", "FIRMS", TEAL),
           ("bank_nfa_minor_units", "BANKS", ORANGE), ("government_nfa_minor_units", "GOVERNMENT", VIOLET),
           ("central_bank_nfa_minor_units", "CENTRAL BANK", RED), ("rest_of_world_nfa_minor_units", "REST OF WORLD", GREEN)]
    for key, label, color in nfa:
        ax.plot([f(r, "month") for r in baseline], [f(r, key) / 1e6 for r in baseline], color=color, lw=1.2, label=label)
    header(ax, "INSTITUTIONAL NET FINANCIAL ASSETS", "BASELINE BRANCH • LEDGER-DERIVED SECTOR POSITIONS", ylab="MINOR UNITS (MILLIONS)")
    ax.axhline(0, color=MINOR, lw=0.65); legend(ax, ncol=3)
    finish(fig, "09_sector_balance_sheet_positions", "Sector balance-sheet positions", "metrics.csv (baseline simulation branch)", "multi-series line")

    # 10 — Firm churn.
    fig, (ax,) = fig_axes(figsize=(10.8, 5.9))
    x = np.array([f(r, "month") for r in baseline])
    entries = np.array([f(r, "firm_entries") for r in baseline]); defaults = np.array([f(r, "firm_defaults") for r in baseline])
    ax.step(x, entries, color=GREEN, where="mid", lw=1.1, label="FIRM ENTRIES")
    ax.step(x, -defaults, color=RED, where="mid", lw=1.1, label="FIRM DEFAULTS (NEGATIVE)")
    ax.fill_between(x, 0, entries, step="mid", color=GREEN, alpha=0.12)
    ax.fill_between(x, 0, -defaults, step="mid", color=RED, alpha=0.12)
    header(ax, "FIRM ENTRY / DEFAULT EVENT STREAM", "BASELINE BRANCH • MONTHLY EVENT COUNTS", ylab="EVENT COUNT")
    ax.axhline(0, color=MINOR, lw=0.65); legend(ax)
    finish(fig, "10_firm_churn_event_stream", "Firm entry and default event stream", "metrics.csv (baseline simulation branch)", "signed step line")

    # 11 — Pareto policy objective surface.
    fig, (ax,) = fig_axes(figsize=(10.8, 5.9))
    scores = comparison["scores"]
    for s in scores:
        branch = s["branch_id"]
        ax.scatter(s["output_loss"], s["inflation_loss"], s=35 + s["fiscal_cost"] / 85,
                   color=BRANCH_COLORS[branch], edgecolors=TEXT if s["pareto_efficient"] else BORDER,
                   linewidths=0.75, alpha=0.92, zorder=3)
        ax.annotate(BRANCH_LABELS[branch].replace(" ", "\n"), (s["output_loss"], s["inflation_loss"]),
                    xytext=(5, 5), textcoords="offset points", fontsize=5.8, color=TEXT)
    header(ax, "POLICY OBJECTIVE FRONTIER", "EVALUATION MONTHS 24–60 • BUBBLE AREA = FISCAL COST • OUTLINED = PARETO-EFFICIENT", xlab="OUTPUT LOSS", ylab="INFLATION LOSS")
    finish(fig, "11_policy_objective_frontier", "Policy objective frontier", "comparison.json (policy objective scores)", "bubble scatter")

    # 12 — Regret matrix.
    fig, (ax,) = fig_axes(figsize=(10.8, 5.9))
    dims = ["inflation", "output", "unemployment", "distribution", "defaults", "fiscal"]
    mat = np.array([[s["normalized_regrets"][d] for d in dims] for s in scores])
    cmap2 = LinearSegmentedColormap.from_list("regret", [PANEL, "#243448", BLUE, ORANGE])
    im = ax.imshow(mat, aspect="auto", vmin=0, vmax=1, cmap=cmap2)
    ax.set_yticks(range(len(scores)), [BRANCH_LABELS[s["branch_id"]] for s in scores])
    ax.set_xticks(range(len(dims)), [d.upper() for d in dims], rotation=28, ha="right")
    for i in range(mat.shape[0]):
        for j in range(mat.shape[1]): ax.text(j, i, f"{mat[i,j]:.2f}", ha="center", va="center", fontsize=6.3, color=TEXT)
    header(ax, "NORMALIZED REGRET MATRIX", "EVALUATION MONTHS 24–60 • 0 = BEST BRANCH ON OBJECTIVE; 1 = WORST", xlab="OBJECTIVE", ylab="POLICY BRANCH")
    cbar = fig.colorbar(im, ax=ax, pad=0.02); cbar.outline.set_edgecolor(BORDER); cbar.ax.tick_params(colors=SECONDARY, labelsize=6.4)
    finish(fig, "12_policy_regret_matrix", "Policy normalized regret matrix", "comparison.json (normalized regret diagnostics)", "annotated heatmap")

    # 13 — Accounting precision.
    fig, (ax,) = fig_axes(figsize=(10.8, 5.9))
    for branch, rs in groups.items():
        vals = np.maximum(np.array([abs(f(r, "accounting_identity_relative_error")) for r in rs]), 1e-16)
        ax.plot([f(r, "month") for r in rs], vals, color=BRANCH_COLORS[branch], lw=1.0, label=BRANCH_LABELS[branch])
    ax.set_yscale("log")
    header(ax, "ACCOUNTING-IDENTITY NUMERICAL ERROR", "SIX BRANCHES • RELATIVE ERROR ON LOG SCALE • RESULT BUNDLE DIAGNOSTIC", ylab="ABSOLUTE RELATIVE ERROR")
    legend(ax, ncol=2)
    finish(fig, "13_accounting_precision", "Accounting identity numerical error", "metrics.csv (six simulation branches)", "log-scale line")

    # 14 — Census BDS job flows.
    fig, (ax,) = fig_axes(figsize=(10.8, 5.9))
    years = [int(f(r, "year")) for r in bds]
    ax.plot(years, [f(r, "job_creation") / 1e6 for r in bds], color=GREEN, lw=1.45, marker="o", ms=2.7, label="JOB CREATION")
    ax.plot(years, [f(r, "job_destruction") / 1e6 for r in bds], color=RED, lw=1.45, marker="s", ms=2.5, label="JOB DESTRUCTION")
    ax.bar(years, [f(r, "net_job_creation") / 1e6 for r in bds], width=0.56, color=[GREEN if f(r, "net_job_creation") >= 0 else RED for r in bds], alpha=0.35, label="NET JOB CREATION")
    header(ax, "US BUSINESS DYNAMICS: JOB FLOWS", "CENSUS BDS • EVALUATION-ONLY SERIES • 2007–2023", xlab="YEAR", ylab="JOBS (MILLIONS)")
    ax.axhline(0, color=MINOR, lw=0.65); legend(ax, ncol=3)
    finish(fig, "14_bds_job_flows", "Census BDS job flows", "bds2023_economy_2007_2023.csv", "line plus signed bar")

    # 15 — Census business reallocation.
    fig, axes = fig_axes(1, 2, figsize=(10.8, 5.2))
    axes[0].plot(years, [f(r, "reallocation_rate") for r in bds], color=BLUE, lw=1.5, marker="o", ms=2.6)
    header(axes[0], "JOB REALLOCATION RATE", "CENSUS BDS • 2007–2023", xlab="YEAR", ylab="PERCENT")
    axes[1].plot(years, [f(r, "estabs_entry_rate") for r in bds], color=GREEN, lw=1.2, marker="o", ms=2.3, label="ENTRY RATE")
    axes[1].plot(years, [f(r, "estabs_exit_rate") for r in bds], color=ORANGE, lw=1.2, marker="s", ms=2.3, label="EXIT RATE")
    header(axes[1], "ESTABLISHMENT ENTRY / EXIT", "CENSUS BDS • 2007–2023", xlab="YEAR", ylab="PERCENT")
    legend(axes[1])
    fig.suptitle("US BUSINESS REALLOCATION", x=0.09, ha="left", y=1.01, fontsize=10.0, color=TEXT)
    finish(fig, "15_bds_reallocation", "Census BDS business reallocation", "bds2023_economy_2007_2023.csv", "paired time-series")

    # 16 — Sector job creation rate heatmap.
    sector_codes = sorted({r["sector"] for r in sector}, key=lambda code: int(code.split("-")[0]))
    smat = np.array([[f(next(r for r in sector if r["sector"] == code and int(f(r, "year")) == year), "net_job_creation_rate")
                      for year in years] for code in sector_codes])
    fig, (ax,) = fig_axes(figsize=(10.8, 5.9))
    cmap3 = LinearSegmentedColormap.from_list("sector", [RED, PANEL, BLUE])
    vmax = max(abs(np.nanmin(smat)), abs(np.nanmax(smat)))
    im = ax.imshow(smat, aspect="auto", cmap=cmap3, vmin=-vmax, vmax=vmax, interpolation="nearest")
    ax.set_yticks(range(len(sector_codes)), [f"NAICS {s}" for s in sector_codes])
    ax.set_xticks(range(0, len(years), 2), [str(years[i]) for i in range(0, len(years), 2)])
    header(ax, "SECTOR NET JOB CREATION RATE", "CENSUS BDS • SECTOR-YEAR EVALUATION-ONLY PANEL • 2007–2023", xlab="YEAR", ylab="SECTOR")
    cbar = fig.colorbar(im, ax=ax, pad=0.02); cbar.outline.set_edgecolor(BORDER); cbar.ax.tick_params(colors=SECONDARY, labelsize=6.4); cbar.set_label("PERCENT", color=SECONDARY, fontsize=6.7)
    finish(fig, "16_bds_sector_job_creation_heatmap", "Sector net job creation rates", "bds2023_sector_2007_2023.csv", "diverging heatmap")

    # 17 — Vintage backtest: point predictions against next-vintage observations.
    fig, axes = fig_axes(1, 3, figsize=(12.5, 4.7))
    series_colors = {"CPI": ORANGE, "RUC": TEAL, "ROUTPUT": BLUE}
    for ax, name in zip(axes, ["CPI", "RUC", "ROUTPUT"]):
        subset = [r for r in backtest if r["series"] == name]
        for model, marker, color in [("persistence", "o", NEUTRAL), ("expanding_ar1", "s", series_colors[name])]:
            rr = [r for r in subset if r["model"] == model]
            ax.scatter([f(r, "actual_next_vintage") for r in rr], [f(r, "prediction") for r in rr], s=10, marker=marker,
                       color=color, alpha=0.78, label=model.upper())
        vals = [f(r, "actual_next_vintage") for r in subset] + [f(r, "prediction") for r in subset]
        lo, hi = min(vals), max(vals); pad = (hi - lo) * 0.06
        ax.plot([lo-pad, hi+pad], [lo-pad, hi+pad], color=MINOR, lw=0.7, ls="--")
        ax.set_xlim(lo-pad, hi+pad); ax.set_ylim(lo-pad, hi+pad)
        header(ax, name, "VINTAGE BACKTEST", xlab="NEXT-VINTAGE ACTUAL", ylab="PREDICTION")
        legend(ax, ncol=1)
    fig.suptitle("VINTAGE BACKTEST: PREDICTION VS NEXT-VINTAGE OBSERVATION", x=0.07, ha="left", y=1.02, fontsize=10.0, color=TEXT)
    finish(fig, "17_vintage_backtest_prediction_fit", "Vintage backtest prediction fit", "backtest_philadelphia_2007_2023.csv", "faceted prediction scatter")

    # 18 — Backtest residual distribution and interval calibration.
    fig, axes = fig_axes(1, 2, figsize=(10.8, 5.2))
    for name, color in [("CPI", ORANGE), ("RUC", TEAL), ("ROUTPUT", BLUE)]:
        vals = [f(r, "error") for r in backtest if r["series"] == name and r["model"] == "expanding_ar1"]
        axes[0].hist(vals, bins=14, histtype="step", color=color, lw=1.25, label=name)
    header(axes[0], "AR(1) FORECAST ERROR DISTRIBUTION", "VINTAGE BACKTEST • ERROR = PREDICTION − NEXT-VINTAGE ACTUAL", xlab="ERROR", ylab="OBSERVATIONS")
    axes[0].axvline(0, color=MINOR, lw=0.65); legend(axes[0])
    calibrated = [r for r in backtest if r["model"] == "expanding_ar1" and not math.isnan(f(r, "interval_hit_80"))]
    for name, color in [("CPI", ORANGE), ("RUC", TEAL), ("ROUTPUT", BLUE)]:
        rr = [r for r in calibrated if r["series"] == name]
        xx = [f(r, "training_pairs") for r in rr]; yy = [f(r, "interval_hit_80") for r in rr]
        axes[1].scatter(xx, yy, s=12, color=color, alpha=0.72, label=name)
    header(axes[1], "80% INTERVAL HIT DIAGNOSTIC", "AR(1) ROWS WITH AVAILABLE INTERVALS • 1 = ACTUAL INSIDE INTERVAL", xlab="TRAINING PAIRS", ylab="INTERVAL HIT")
    axes[1].axhline(0.8, color=MINOR, lw=0.7, ls="--", label="NOMINAL 80%")
    axes[1].set_ylim(-0.08, 1.08); legend(axes[1], ncol=2)
    fig.suptitle("BACKTEST RESIDUAL / INTERVAL DIAGNOSTICS", x=0.09, ha="left", y=1.01, fontsize=10.0, color=TEXT)
    finish(fig, "18_vintage_backtest_error_diagnostics", "Vintage backtest error diagnostics", "backtest_philadelphia_2007_2023.csv", "histogram plus calibration scatter")

    # 19 — Engineering reserve diagnostic: 3 deterministic seeds / 2 banks.
    fig, axes = fig_axes(1, 2, figsize=(10.8, 5.2))
    seed_colors = {"20260724": BLUE, "20260725": TEAL, "20260726": VIOLET}
    for seed in sorted(seed_colors):
        rr = [r for r in reserve if r["seed"] == seed and r["bank"] == "BANK-000"]
        axes[0].plot([f(r, "month") for r in rr], [f(r, "liquidity_ratio") for r in rr], color=seed_colors[seed], lw=1.25, marker="o", ms=2.4, label=f"SEED {seed[-3:]}")
        axes[1].plot([f(r, "month") for r in rr], [f(r, "reserve_minor_units") / 1e6 for r in rr], color=seed_colors[seed], lw=1.25, marker="o", ms=2.4, label=f"SEED {seed[-3:]}")
    axes[0].axhline(0.1, color=ORANGE, lw=0.9, ls="--", label="LIQUIDITY FLOOR")
    header(axes[0], "BANK-000 LIQUIDITY RATIO", "THREE-SEED SYNTHETIC RESERVE DIAGNOSTIC • MONTHS 0–12", ylab="RATIO")
    header(axes[1], "BANK-000 RESERVE POSITION", "THREE-SEED SYNTHETIC RESERVE DIAGNOSTIC • MONTHS 0–12", ylab="MINOR UNITS (MILLIONS)")
    legend(axes[0], ncol=2); legend(axes[1], ncol=2)
    fig.suptitle("RESERVE / LIQUIDITY INVARIANT OBSERVATION", x=0.09, ha="left", y=1.01, fontsize=10.0, color=TEXT)
    finish(fig, "19_reserve_liquidity_diagnostic", "Reserve liquidity diagnostic", "synthetic_reserve_liquidity_12m_20261018/observations.csv", "paired deterministic-seed line")


def create_dashboard():
    """Compose 15 high-value evidence panels into a 3840×2160 artwork-ready wall."""
    canvas = Image.new("RGB", (3840, 2160), BG)
    draw = ImageDraw.Draw(canvas)
    try:
        title_font = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", 60)
        subtitle_font = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", 47)
        tiny_font = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", 13)
    except OSError:
        title_font = subtitle_font = tiny_font = ImageFont.load_default()
    # Three-line anchor block: it owns the upper-left field; the graph wall starts beside it.
    title_x = 34
    draw.text((title_x, 44), "OPENWORLDGEN", fill=TEXT, font=title_font)
    draw.text((title_x, 121), "ECONOMIC RESEARCH", fill=TEXT, font=subtitle_font)
    draw.text((title_x, 181), "WALL", fill=TEXT, font=subtitle_font)
    draw.line((title_x, 257, 585, 257), fill=BORDER, width=1)
    draw.text((title_x, 276), "CONDITIONAL DEVELOPMENT SIMULATION", fill=SECONDARY, font=tiny_font)
    draw.text((title_x, 296), "VINTAGE BACKTEST • CENSUS BDS • ENGINEERING DIAGNOSTICS", fill=SECONDARY, font=tiny_font)
    draw.text((title_x, 330), "NOT A POINT FORECAST", fill=ORANGE, font=tiny_font)

    # Fifteen high-value panels with enlarged 4:3-ish footprints.  The upper row
    # advances left from the prior layout; the final row keeps a stepped start.
    gap, left, right = 13, 30, 30
    layouts = []
    top_x, top_y, top_h = 620, 30, 515
    top_w = (3840 - top_x - right - 2 * gap) // 3
    layouts += [(top_x + i * (top_w + gap), top_y, top_w, top_h) for i in range(3)]

    y = top_y + top_h + gap
    for offset in (30, 85):
        h = 515
        w = (3840 - offset - right - 3 * gap) // 4
        layouts += [(offset + i * (w + gap), y, w, h) for i in range(4)]
        y += h + gap

    bottom_offset, bottom_h = 320, 515
    bottom_w = (3840 - bottom_offset - right - 3 * gap) // 4
    layouts += [(bottom_offset + i * (bottom_w + gap), y, bottom_w, bottom_h) for i in range(4)]

    by_slug = {plot["slug"]: plot for plot in PLOTS}
    selected = []
    for slug in DASHBOARD_SLUGS:
        plot = by_slug.get(slug)
        if plot is None:
            plot = {"slug": slug, "png": INDIVIDUAL / f"{slug}.png"}
        if not plot["png"].is_file():
            raise FileNotFoundError(f"Missing source panel: {plot['png']}")
        selected.append(plot)

    if len(selected) != len(layouts):
        raise RuntimeError(f"Dashboard selection/layout mismatch: {len(selected)} plots, {len(layouts)} slots")
    for plot, (x, y, w, h) in zip(selected, layouts):
        with Image.open(plot["png"]) as src:
            src = src.convert("RGB")
            ratio = min(w/src.width, h/src.height)
            new = src.resize((max(1, int(src.width*ratio)), max(1, int(src.height*ratio))), Image.Resampling.LANCZOS)
            px, py = x+(w-new.width)//2, y+(h-new.height)//2
            canvas.paste(new, (px, py))
        draw.rectangle((x, y, x+w-1, y+h-1), outline=BORDER, width=1)

    dashboard = OUT / "openworldgen_economic_research_wall_3840x2160.png"
    canvas.save(dashboard, optimize=True)
    return dashboard


if __name__ == "__main__":
    generate()
    create_dashboard()
    print(f"Generated {len(PLOTS)} figures and 3840×2160 dashboard at {OUT}")
