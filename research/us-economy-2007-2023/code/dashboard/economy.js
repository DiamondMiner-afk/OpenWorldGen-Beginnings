"use strict";

const moneyCompact = new Intl.NumberFormat("en-US", {
  notation: "compact", maximumFractionDigits: 2
});
const money = value => Number.isFinite(value) ? moneyCompact.format(value) : "—";
const colors = { simulated: "#43f3ff", observed: "#ff6acb", baseline: "#c8ff4a" };

function graph(id, lines, options) {
  const chart = get(id);
  if (chart.clientWidth === 0) return;
  chart.replaceChildren();
  const width = Math.max(260, chart.clientWidth);
  const compact = width < 500;
  const height = options.height || (compact ? 195 : 220);
  const { horizon, year, cursor: selected, observedThrough, events, format, empty } = options;
  chart.setAttribute("viewBox", "0 0 " + width + " " + height);
  const left = 48, right = width - 15, top = 16, bottom = height - 28;
  const x = index => left + (right - left) * index / horizon;
  const values = lines.flatMap(line => line.values.filter(Number.isFinite));
  const lower = Math.min(0, ...values);
  const upper = Math.max(0, ...values);
  const magnitude = (upper - lower) / 4 || options.emptyStep;
  const scale = 10 ** Math.floor(Math.log10(magnitude));
  const step = ([1, 2, 2.5, 5, 10].find(multiplier => multiplier * scale >= magnitude) || 10) * scale;
  const floor = Math.floor(lower / step);
  const ceiling = Math.max(floor + 1, Math.ceil(upper / step));
  const y = value => bottom - (bottom - top) * (value / step - floor) / (ceiling - floor);
  for (let index = floor; index <= ceiling; index++) {
    const level = y(index * step);
    chart.append(svg("line", { x1: left, x2: right, y1: level, y2: level, class: "chart-grid" }),
      svg("text", { x: left - 5, y: level + 3, "text-anchor": "end" }, format(index * step)));
  }
  const yearTicks = Math.max(1, Math.ceil(horizon / 12 / (compact ? 3 : 6)));
  const tickMonths = horizon <= 12 ? 3 : 12 * yearTicks;
  for (let index = 1; index <= horizon; index += tickMonths) {
    const date = new Date(Date.UTC(year, index - 1, 1));
    const label = horizon <= 12 ? month.format(date).split(" ")[0] : String(date.getUTCFullYear());
    chart.append(svg("text", { x: x(index), y: height - 8, "text-anchor": "middle" }, label));
  }
  const [observedYear, observedMonth] = observedThrough.split("-").map(Number);
  const cutoff = (observedYear - year) * 12 + observedMonth;
  if (cutoff > 0 && cutoff < horizon) {
    chart.append(svg("line", { x1: x(cutoff), x2: x(cutoff), y1: top, y2: bottom, class: "archive-cutoff" }));
    chart.append(svg("text", { x: x(cutoff) + 3, y: top - 4 }, "Archive ends"));
  }
  for (const event of events) {
    const position = x(event.month + 1);
    const marker = svg("line", { x1: position, x2: position, y1: top, y2: bottom, class: "event-mark" });
    marker.append(svg("title", {}, event.label + " · " + event.period));
    chart.append(marker);
  }
  if (selected > 0) chart.append(svg("line", {
    x1: x(selected), x2: x(selected), y1: top, y2: bottom, class: "chart-cursor"
  }));
  for (const line of [...lines].reverse()) {
    const points = line.values.flatMap((value, index) => Number.isFinite(value) ? [[x(index), y(value)]] : []);
    if (points.length > 1) chart.append(svg("polyline", {
      points: points.map(point => point.join(",")).join(" "), fill: "none", stroke: line.color,
      "stroke-width": line.dashed ? 1.3 : 1.9, "stroke-dasharray": line.dashed ? "4 5" : "none",
      "vector-effect": "non-scaling-stroke"
    }));
    if (Number.isFinite(line.values[selected])) {
      chart.append(svg("circle", { cx: x(selected), cy: y(line.values[selected]), r: 3, fill: line.color }));
    } else if (points.length === 1) {
      chart.append(svg("circle", { cx: points[0][0], cy: points[0][1], r: 2.6, fill: line.color }));
    }
  }
  if (!values.length) chart.append(svg("text", {
    x: (left + right) / 2, y: (top + bottom) / 2, "text-anchor": "middle"
  }, empty));
}

function chartOptions(run, cursor) {
  return {
    horizon: run.run.months,
    year: run.origin.year,
    cursor,
    observedThrough: run.origin.observed_through || "2023-12",
    events: run.events || []
  };
}

function quarterlyGrowth(frames) {
  return frames.map((_, index) => {
    const length = index + 1;
    if (length < 6 || length % 3 !== 0) return null;
    const output = (start, end) => frames.slice(start, end).reduce((sum, row) => sum + row.frame.metrics.real_gdp, 0);
    const past = output(length - 6, length - 3);
    const current = output(length - 3, length);
    return past > 0 && current > 0 ? (current / past) ** 4 - 1 : null;
  });
}

function renderEconomy(run, frames, cursor) {
  const current = frames[cursor - 1];
  const metrics = current?.frame.metrics;
  const actual = current?.comparison;
  set("gdp", money(metrics?.real_gdp));
  set("inflation", cursor > 12 ? percent(metrics.inflation_year_over_year) : "—");
  set("price-note", cursor > 12 ? "Simulated CPI · year over year" : "Requires 12 months of price history");
  set("macro-unemployment", metrics ? percent(metrics.unemployment) : "—");
  set("macro-unemployment-note", Number.isFinite(actual?.observed_unemployment)
    ? "Observed US " + percent(actual.observed_unemployment) : "Share of the simulated labor force");
  set("active-firms", metrics ? count.format(metrics.active_firms) : "—");
  set("firm-note", metrics ? metrics.firm_defaults + " exits · " + metrics.firm_entries + " entries" : "Simulated firms in operation");

  const path = (values, color, dashed = false) => ({ color, dashed, values: [null, ...values] });
  const observed = name => frames.map(row => row.comparison[name]);
  const series = name => frames.map(row => row.frame.metrics[name]);
  const options = chartOptions(run, cursor);
  const rate = { ...options, emptyStep: .05, format: v => (v * 100).toFixed(Math.abs(v * 100) < 1 && v !== 0 ? 1 : 0) + "%" };
  const amount = { ...options, emptyStep: 1000, format: money };
  graph("gdp-chart", [
    path(quarterlyGrowth(frames), colors.simulated),
    path(observed("observed_real_growth"), colors.observed),
  ], { ...rate, empty: "Quarterly values will appear here" });
  graph("cpi-chart", [
    path(frames.map((row, i) => i >= 12 ? row.frame.metrics.inflation_year_over_year : null), colors.simulated),
    path(observed("observed_inflation"), colors.observed),
  ], { ...rate, empty: "Advance one month to see prices" });
  const bankOpening = run.run.opening_finance;
  const showRatio = get("credit-view").value === "ratio";
  const legend = get("credit-legend");
  if (showRatio) {
    const initial = run.bank_opening ? run.origin.finance.opening_loan_to_deposit : null;
    const loanRatios = frames.map(row => {
      const banks = row.frame.banks || [];
      const total = field => banks.reduce((sum, bank) => sum + bank[field], 0);
      return total("deposits") > 0 ? total("loans") / total("deposits") : null;
    });
    set("credit-note", "Consumer and business loan balances as a share of deposits");
    legend.replaceChildren(element("span", "━ Simulated", "accent"), element("span", "━ Observed H.8", "observed"));
    graph("credit-chart", [
      { values: [initial, ...loanRatios], color: colors.simulated },
      path(observed("observed_bank_loan_to_deposit"), colors.observed),
      { values: [initial, ...frames.map(() => initial)], color: colors.baseline, dashed: true },
    ], { ...rate, empty: "Advance one month to see balances" });
    if (initial !== null) legend.append(element("span", "┄ Opening ratio", "baseline"));
  } else {
    set("credit-note", "Simulated month-end balances · OWG");
    legend.replaceChildren(element("span", "━ Private credit", "accent"), element("span", "┄ Bank reserves", "baseline"));
    graph("credit-chart", [
      path(series("credit"), colors.simulated),
      path(frames.map(row => row.frame.banks?.reduce((sum, bank) => sum + bank.reserves, 0)), colors.baseline, true),
    ], { ...amount, empty: "Advance one month to see balances" });
  }
  graph("public-chart", [
    path(series("fiscal_balance"), colors.simulated),
    path(series("imports"), colors.baseline, true),
  ], { ...amount, empty: "Advance one month to see flows" });

  function rows(id, values) {
    const body = get(id);
    body.replaceChildren();
    for (const [label, ...cells] of values) {
      const row = element("tr");
      const header = element("th", label);
      header.scope = "row";
      row.append(header, ...cells.map(value => element("td", value)));
      body.append(row);
    }
  }
  const openingSectors = run.run.opening_production?.sectors || [];
  const sectorRows = current?.frame.sectors || openingSectors.map(sector => ({
    sector: sector.sector, active_firms: sector.firms, workers: sector.workers
  }));
  rows("macro-sectors", sectorRows.map(sector => [
    sector.sector[0].toUpperCase() + sector.sector.slice(1),
    count.format(sector.active_firms), count.format(sector.workers), money(sector.sales), money(sector.household_spending)
  ]));
  renderComposition(run, frames, cursor);
  renderOutput(run, frames, cursor);
  const labels = { large: "Large domestic", other_domestic: "Other domestic" };
  const openingRows = bankOpening?.banks || [];
  const balances = current?.frame.banks || openingRows.map(bank => ({
    id: bank.bank_id, deposits: bank.deposits_minor / 100, reserves: bank.reserves_minor / 100,
    loans: (bank.household_loans_minor + bank.firm_loans_minor) / 100, equity: bank.equity_minor / 100
  }));
  rows("bank-balances", balances.map(bank => [
    labels[openingRows.find(row => row.bank_id === bank.id)?.cohort] || bank.id,
    money(bank.deposits), money(bank.reserves), money(bank.loans), money(bank.equity)
  ]));
  set("bank-source", bankOpening
    ? "Opening ratios from H.8 " + run.origin.finance.h8_month + " and Z.1 " + run.origin.finance.z1_quarter + " · OWG"
    : "Synthetic bank opening · OWG");
  function pairs(id, values) {
    get(id).replaceChildren(...values.flatMap(([label, value]) => [element("dt", label), element("dd", value)]));
  }
  pairs("household-summary", [
    ["Household spending", money(metrics?.consumption_total)],
    ["Average monthly wage", money(metrics?.average_wage)],
    ["Income inequality (Gini)", metrics ? metrics.gini.toFixed(3) : "—"]
  ]);
  pairs("public-summary", [
    ["Government debt", money(metrics?.government_debt)],
    ["Policy interest rate", metrics ? percent(metrics.policy_rate) : "—"],
    ["Ledger residual", metrics ? metrics.ledger_residual_minor_units + " minor units" : "—"]
  ]);
}

function renderComposition(run, frames, cursor) {
  const target = get("composition");
  target.replaceChildren();
  const payroll = get("composition-view").value === "payroll";
  const source = run.origin.production;
  const open = run.run.opening_production?.sectors;
  const categories = ["Energy", "Food", "Manufacturing", "Services"];
  const end = payroll ? cursor : cursor - cursor % 3;
  const frame = frames[end - 1];
  let actual, sim;
  if (payroll && frame) {
    actual = frame.comparison.observed_payroll_shares;
    sim = frame.frame.sectors.map(row => row.workers);
  } else if (payroll && open) {
    sim = open.map(row => row.workers);
  } else if (frame) {
    actual = frame.comparison.observed_consumption_shares;
    sim = categories.map((_, index) => frames.slice(end - 3, end).reduce(
      (sum, row) => sum + row.frame.sectors[index].household_spending, 0));
  }
  const base = payroll ? source?.payroll_shares : source?.consumption_shares;
  const total = sim?.reduce((sum, value) => sum + value, 0);
  const period = frame ? formatPeriod(frame.comparison.period) : "Opening";
  const provenance = source
    ? (payroll ? " · BLS private payroll mapping" : " · BEA nominal PCE mapping")
    : "";
  set("composition-note", (payroll ? period + " · share of employed agents" :
    frame ? "Quarter through " + period + " · share of household spending" : "First comparison after one complete quarter") +
    provenance);
  for (const [index, category] of categories.entries()) {
    const row = element("div", undefined, "composition-row");
    row.append(element("strong", category));
    for (const [name, value, kind] of [
      ["Simulated", total > 0 ? sim[index] / total : null, "accent"],
      ["Observed", actual?.[index], "observed"],
      ["Opening", base?.[index], "baseline"],
    ]) {
      const cell = element("div", undefined, "composition-cell " + kind);
      if (Number.isFinite(value)) {
        const bar = element("meter");
        bar.max = 1;
        bar.value = value;
        bar.setAttribute("aria-label", name + " " + category);
        cell.append(bar, element("span", percent(value)));
      } else cell.append(element("span", "—"));
      row.append(cell);
    }
    target.append(row);
  }
}

function renderOutput(run, frames, cursor) {
  const view = get("output-view").value;
  const legend = get("output-legend");
  const current = frames[cursor - 1];
  const options = chartOptions(run, cursor);
  const line = (values, color, dashed = false) => ({ values: [null, ...values], color, dashed });
  if (view === "output") {
    set("output-note", "Monthly gross output, imported inputs, and value added at opening prices · OWG");
    legend.replaceChildren(element("span", "━ Gross output", "accent"),
      element("span", "━ Value added", "observed"), element("span", "┄ Imported inputs", "baseline"));
    const flow = field => frames.map(row => row.frame.production_flows?.[field]);
    graph("output-chart", [
      line(flow("gross_output_fixed_prices"), colors.simulated),
      line(flow("value_added_fixed_prices"), colors.observed),
      line(flow("imported_inputs_fixed_prices"), colors.baseline, true),
    ], { ...options, emptyStep: 1000, format: money, empty: "Advance one month to see production" });
    const telemetry = current?.frame.production_flows;
    set("output-current", telemetry ? "Stock at month close " + money(telemetry.inventory_closing_nominal) +
      " OWG · unsold service slots removed " + money(telemetry.unused_service_slots_fixed_prices) + " OWG" : "—");
    return;
  }
  const sector = view === "food" ? 1 : 2;
  const target = run.origin.inventory?.[view]?.opening_months;
  const sim = frames.map(row => {
    const result = row.frame.sectors[sector];
    return result.sales > 0 ? result.inventory_value / result.sales : null;
  });
  set("output-note", "Month-end finished stocks divided by monthly sales · Census M3 comparison");
  legend.replaceChildren(element("span", "━ Simulated", "accent"),
    element("span", "━ Observed M3", "observed"), element("span", "┄ Opening ratio", "baseline"));
  graph("output-chart", [
    line(sim, colors.simulated),
    line(frames.map(row => row.comparison.observed_inventory_ratios?.[sector - 1]), colors.observed),
    line(frames.map(() => target), colors.baseline, true),
  ], { ...options, emptyStep: .2, format: v => v.toFixed(2), empty: "Advance one month to see inventories" });
  const point = sim[cursor - 1];
  set("output-current", Number.isFinite(point) ? "Simulated stock " + point.toFixed(2) +
    " months of sales · " + (run.inventory ? "dated adjustment trial" : "original stock rule") : "—");
}
