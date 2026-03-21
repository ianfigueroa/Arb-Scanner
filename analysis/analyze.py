"""
Arbitrage opportunity analytics dashboard.

Usage:
    python analysis/analyze.py [arb_opportunities.db]
"""

import sys
import sqlite3
import os
from pathlib import Path

import pandas as pd
import matplotlib.pyplot as plt

try:
    from rich.console import Console
    from rich.table import Table
    from rich.panel import Panel
except ImportError:
    class Console:
        def print(self, *args, **kwargs) -> None:
            try:
                print(*args, **kwargs)
            except UnicodeEncodeError:
                text = kwargs.get("sep", " ").join(str(arg) for arg in args)
                end = kwargs.get("end", "\n")
                stream = kwargs.get("file", sys.stdout)
                stream.buffer.write((text + end).encode(stream.encoding or "utf-8", errors="replace"))
                stream.flush()

    class Table:
        def __init__(self, *args, **kwargs) -> None:
            self.rows: list[tuple[str, ...]] = []

        def add_column(self, *args, **kwargs) -> None:
            pass

        def add_row(self, *args) -> None:
            self.rows.append(tuple(str(arg) for arg in args))

        def __str__(self) -> str:
            return "\n".join(" | ".join(row) for row in self.rows)

    class Panel:
        def __init__(self, renderable: str, *args, **kwargs) -> None:
            self.renderable = renderable

        def __str__(self) -> str:
            return self.renderable

console = Console()

OUTPUT_DIR = Path(__file__).parent / "output"


def main(db_path: str) -> None:
    OUTPUT_DIR.mkdir(exist_ok=True)
    opps, snaps = load_data(db_path)
    print_session_overview(opps, snaps)
    print_profitability_summary(opps)
    print_top_paths(opps)
    print_chain_breakdown(opps)
    out = str(OUTPUT_DIR)
    plot_roi_distribution(opps, out)
    plot_opportunities_timeline(opps, out)
    plot_path_frequency(opps, out)
    plot_gas_vs_profit(opps, out)
    plot_weth_price(snaps, out)
    console.print(f"\n[bold green]Charts saved to {out}/[/bold green]")


def load_data(db_path: str) -> tuple[pd.DataFrame, pd.DataFrame]:
    if not os.path.exists(db_path):
        console.print(f"[bold red]Error:[/bold red] database not found at {db_path}")
        sys.exit(1)

    con = sqlite3.connect(db_path)

    opps = pd.read_sql_query(
        "SELECT * FROM opportunities ORDER BY timestamp ASC", con
    )
    snaps = pd.read_sql_query(
        "SELECT * FROM price_snapshots ORDER BY timestamp ASC", con
    )
    con.close()

    if opps.empty and snaps.empty:
        console.print("[bold red]Error:[/bold red] database is empty — run the bot first to collect data.")
        sys.exit(1)

    if not opps.empty:
        opps["datetime"] = pd.to_datetime(opps["timestamp"], unit="s", utc=True)

    if not snaps.empty:
        snaps["datetime"] = pd.to_datetime(snaps["timestamp"], unit="s", utc=True)

    return opps, snaps


def count_tracked_blocks(snaps: pd.DataFrame) -> int:
    if snaps.empty or "block" not in snaps.columns:
        return 0
    return int(snaps["block"].nunique())


def net_profit_eth(opps: pd.DataFrame) -> pd.Series:
    if opps.empty or "net_wei" not in opps.columns:
        return pd.Series(dtype="float64")
    return opps["net_wei"] / 1e18


def session_overview_stats(opps: pd.DataFrame, snaps: pd.DataFrame) -> dict[str, int | str]:
    if not snaps.empty:
        t_min = snaps["datetime"].min().strftime("%Y-%m-%d %H:%M UTC")
        t_max = snaps["datetime"].max().strftime("%Y-%m-%d %H:%M UTC")
        time_range = f"{t_min} -> {t_max}"
        chains = int(snaps["chain"].nunique()) if "chain" in snaps.columns else 0
        blocks = count_tracked_blocks(snaps)
    elif not opps.empty:
        t_min = opps["datetime"].min().strftime("%Y-%m-%d %H:%M UTC")
        t_max = opps["datetime"].max().strftime("%Y-%m-%d %H:%M UTC")
        time_range = f"{t_min} -> {t_max}"
        chains = int(opps["chain"].nunique()) if "chain" in opps.columns else 0
        blocks = 0
    else:
        time_range = "no data"
        chains = 0
        blocks = 0

    return {
        "total_opportunities": len(opps),
        "chains_active": chains,
        "time_range": time_range,
        "blocks_tracked": blocks,
    }


def print_session_overview(opps: pd.DataFrame, snaps: pd.DataFrame) -> None:
    overview = session_overview_stats(opps, snaps)

    text = (
        f"[bold]Total opportunities:[/bold] {overview['total_opportunities']}\n"
        f"[bold]Chains active:[/bold]       {overview['chains_active']}\n"
        f"[bold]Time range:[/bold]           {overview['time_range']}\n"
        f"[bold]Blocks tracked:[/bold]       {overview['blocks_tracked']}"
    )
    console.print(Panel(text, title="Session Overview", expand=False))


def print_profitability_summary(opps: pd.DataFrame) -> None:
    console.print("\n[bold]Profitability Summary[/bold]")
    if opps.empty:
        console.print("  No opportunities recorded.")
        return

    table = Table(show_header=True, header_style="bold cyan")
    table.add_column("Metric", style="dim")
    table.add_column("Value")

    roi = opps["roi_pct"]
    pct_profitable = (roi > 0).mean() * 100
    table.add_row("% profitable", f"{pct_profitable:.1f}%")
    table.add_row("Median ROI", f"{roi.median():.4f}%")
    table.add_row("Mean ROI",   f"{roi.mean():.4f}%")
    table.add_row("Max ROI",    f"{roi.max():.4f}%")

    tiers = [
        ("< 0%",    roi < 0),
        ("0–0.1%",  (roi >= 0) & (roi < 0.1)),
        ("0.1–0.5%",(roi >= 0.1) & (roi < 0.5)),
        ("0.5–1%",  (roi >= 0.5) & (roi < 1.0)),
        ("> 1%",    roi >= 1.0),
    ]
    for label, mask in tiers:
        table.add_row(f"Tier {label}", str(mask.sum()))

    console.print(table)


def print_top_paths(opps: pd.DataFrame) -> None:
    console.print("\n[bold]Top Paths by Count[/bold]")
    if opps.empty:
        console.print("  No opportunities recorded.")
        return

    grouped = (
        opps.groupby("path")["roi_pct"]
        .agg(count="count", avg_roi="mean", best_roi="max")
        .reset_index()
        .sort_values("count", ascending=False)
        .head(10)
    )

    table = Table(show_header=True, header_style="bold cyan")
    table.add_column("Path")
    table.add_column("Count",    justify="right")
    table.add_column("Avg ROI%", justify="right")
    table.add_column("Best ROI%",justify="right")

    for _, row in grouped.iterrows():
        table.add_row(
            row["path"],
            str(int(row["count"])),
            f"{row['avg_roi']:.4f}",
            f"{row['best_roi']:.4f}",
        )
    console.print(table)


def print_chain_breakdown(opps: pd.DataFrame) -> None:
    console.print("\n[bold]Chain Breakdown[/bold]")
    if opps.empty:
        console.print("  No opportunities recorded.")
        return

    grouped = (
        opps.groupby("chain")["roi_pct"]
        .agg(count="count", avg_roi="mean", best_roi="max", worst_roi="min")
        .reset_index()
        .sort_values("count", ascending=False)
    )

    table = Table(show_header=True, header_style="bold cyan")
    table.add_column("Chain")
    table.add_column("Count",     justify="right")
    table.add_column("Avg ROI%",  justify="right")
    table.add_column("Best ROI%", justify="right")
    table.add_column("Worst ROI%",justify="right")

    for _, row in grouped.iterrows():
        table.add_row(
            row["chain"],
            str(int(row["count"])),
            f"{row['avg_roi']:.4f}",
            f"{row['best_roi']:.4f}",
            f"{row['worst_roi']:.4f}",
        )
    console.print(table)


def plot_roi_distribution(opps: pd.DataFrame, out: str) -> None:
    fig, ax = plt.subplots(figsize=(10, 5))
    if not opps.empty:
        ax.hist(opps["roi_pct"], bins=50, color="steelblue", edgecolor="white", linewidth=0.5)
        ax.axvline(0, color="red", linestyle="--", linewidth=1.5, label="Break-even")
        ax.set_yscale("log")
        ax.legend()
    ax.set_title("ROI Distribution Across All Opportunities")
    ax.set_xlabel("ROI (%)")
    ax.set_ylabel("Count (log scale)")
    fig.tight_layout()
    path = os.path.join(out, "roi_distribution.png")
    fig.savefig(path, dpi=150)
    plt.close(fig)


def plot_opportunities_timeline(opps: pd.DataFrame, out: str) -> None:
    fig, ax = plt.subplots(figsize=(12, 5))
    if not opps.empty:
        for chain, group in opps.groupby("chain"):
            series = group.set_index("datetime")["roi_pct"].resample("1h").count()
            ax.plot(series.index, series.values, label=chain, linewidth=1.5)
        ax.legend()
    ax.set_title("Opportunities per Hour by Chain")
    ax.set_xlabel("Time (UTC)")
    ax.set_ylabel("Count")
    fig.autofmt_xdate()
    fig.tight_layout()
    path = os.path.join(out, "opportunities_timeline.png")
    fig.savefig(path, dpi=150)
    plt.close(fig)


def plot_path_frequency(opps: pd.DataFrame, out: str) -> None:
    fig, ax = plt.subplots(figsize=(10, 6))
    if not opps.empty:
        counts = opps["path"].value_counts().head(15)
        counts[::-1].plot(kind="barh", ax=ax, color="steelblue")
    ax.set_title("Top 15 Paths by Frequency")
    ax.set_xlabel("Count")
    ax.set_ylabel("Path")
    fig.tight_layout()
    path = os.path.join(out, "path_frequency.png")
    fig.savefig(path, dpi=150)
    plt.close(fig)


def plot_gas_vs_profit(opps: pd.DataFrame, out: str) -> None:
    fig, ax = plt.subplots(figsize=(10, 6))
    if not opps.empty:
        net_eth = net_profit_eth(opps)
        colors = opps["chain"].astype("category").cat.codes
        scatter = ax.scatter(
            opps["gas_cost_usd"],
            net_eth,
            c=colors,
            alpha=0.5,
            s=10,
            cmap="tab10",
        )
        ax.axhline(0, color="red", linestyle="--", linewidth=1.2, label="Break-even")
        chains = opps["chain"].astype("category")
        handles = [
            plt.Line2D([0], [0], marker="o", color="w",
                       markerfacecolor=scatter.cmap(scatter.norm(i)), markersize=8, label=c)
            for i, c in enumerate(chains.cat.categories)
        ]
        ax.legend(handles=handles)
    ax.set_title("Gas Cost vs Net Profit")
    ax.set_xlabel("Gas Cost (USD)")
    ax.set_ylabel("Net Profit (ETH)")
    fig.tight_layout()
    path = os.path.join(out, "gas_vs_profit.png")
    fig.savefig(path, dpi=150)
    plt.close(fig)


def plot_weth_price(snaps: pd.DataFrame, out: str) -> None:
    fig, ax = plt.subplots(figsize=(12, 5))
    if not snaps.empty and "weth_usd" in snaps.columns and "chain" in snaps.columns:
        for chain, group in snaps.groupby("chain"):
            series = group.set_index("datetime")["weth_usd"]
            ax.plot(series.index, series.values, label=chain, linewidth=1.2)
        ax.legend()
    ax.set_title("WETH/USD Price by Chain Over Time")
    ax.set_xlabel("Time (UTC)")
    ax.set_ylabel("WETH Price (USD)")
    fig.autofmt_xdate()
    fig.tight_layout()
    path = os.path.join(out, "weth_price.png")
    fig.savefig(path, dpi=150)
    plt.close(fig)


if __name__ == "__main__":
    db = sys.argv[1] if len(sys.argv) > 1 else "arb_opportunities.db"
    main(db)
