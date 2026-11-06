"use strict";

const get = id => document.getElementById(id);
const set = (id, text) => { get(id).textContent = text; };
const count = new Intl.NumberFormat("en-US");
const month = new Intl.DateTimeFormat("en-US", { month: "short", year: "numeric", timeZone: "UTC" });
const percent = value => (100 * value).toFixed(1) + "%";
const formatPeriod = value => month.format(new Date(value + "-01T00:00:00Z"));
const statuses = ["Employed", "Jobless", "Outside"];
const frames = [];
let run = null;
let cursor = 0;
let busy = false;
let playing = false;
let timer;
const origins = new Map();

function status(message, error = false) {
  set("status", message);
  get("status").classList.toggle("error", error);
}

async function request(path, data) {
  const options = data === undefined ? {} : {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(data)
  };
  const response = await fetch("/api/" + path, options);
  const result = await response.json();
  if (!response.ok) throw new Error(result.error || "The local engine could not answer.");
  return result;
}

function controls() {
  const done = !run || frames.length >= run.run.months;
  get("step").disabled = busy || done;
  get("play").disabled = done || (busy && !playing);
  get("reset").disabled = busy;
  get("origin").disabled = busy;
  get("seed").disabled = busy;
  get("end-year").disabled = busy;
  get("preset").disabled = busy;
  const selectedOrigin = origins.get(Number(get("origin").value));
  get("finance").disabled = busy || !selectedOrigin?.finance;
  get("production").disabled = busy || !selectedOrigin?.production;
  get("cursor").disabled = frames.length === 0;
  get("cursor").max = frames.length;
  get("cursor").value = cursor;
  set("play", playing ? "Ⅱ Pause" : "▶ Play");
}

function stopPlayback() {
  playing = false;
  clearTimeout(timer);
  controls();
}

async function reset() {
  stopPlayback();
  busy = true;
  controls();
  status("Initializing a fresh Rust economy…");
  try {
    const year = Number(get("origin").value);
    const end = get("end-year");
    end.min = year;
    if (end.value && Number(end.value) < year) end.value = year;
    if (!end.reportValidity() || !end.value) {
      status("Choose a valid end year.", true);
      return;
    }
    if (run && frames.length < run.run.months) await request("stop", { token: run.token });
    const bank_opening = get("finance").value === "historical" && Boolean(origins.get(year)?.finance);
    const profile = get("production").value;
    const production_opening = profile !== "synthetic" && Boolean(origins.get(year)?.production);
    const inventory = profile === "inventory" && Boolean(origins.get(year)?.inventory);
    run = await request("start", {
      year, seed: Number(get("seed").value), end_year: Number(end.value), preset: get("preset").value || "baseline", bank_opening, production_opening, inventory
    });
    frames.length = 0;
    cursor = 0;
    status("Ready. Step one month or play through the run.");
    render();
  } catch (error) {
    run = null;
    status(error.message, true);
  } finally {
    busy = false;
    controls();
  }
}

async function advance() {
  if (busy || !run || frames.length >= run.run.months) return;
  busy = true;
  controls();
  try {
    const result = await request("step", { token: run.token });
    frames.push(result);
    cursor = frames.length;
    if (result.done) stopPlayback();
    status(result.done ? "Run complete. Scrub back to inspect any completed month." :
      playing ? "Running live · each update advances the engine." : "Paused between engine steps.");
    render();
  } catch (error) {
    stopPlayback();
    status(error.message, true);
  } finally {
    busy = false;
    controls();
  }
}

async function tick() {
  await advance();
  if (playing) timer = setTimeout(tick, Number(get("speed").value));
}

function element(tag, value, classes) {
  const node = document.createElement(tag);
  if (value !== undefined) node.textContent = value;
  if (classes) node.className = classes;
  return node;
}

function svg(tag, attributes, value) {
  const node = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const [name, attribute] of Object.entries(attributes)) node.setAttribute(name, attribute);
  if (value !== undefined) node.textContent = value;
  return node;
}

function drawChart() {
  if (!run) return;
  const baseline = get("baseline");
  set("baseline-legend", "┄ " + baseline.selectedOptions[0].textContent);
  const opening = run.origin.opening_unemployment;
  graph("history-chart", [
    { color: colors.simulated, values: [opening, ...frames.map(r => r.frame.metrics.unemployment)] },
    { color: colors.observed, values: [opening, ...frames.map(r => r.comparison.observed_unemployment)] },
    { color: colors.baseline, values: [opening, ...frames.map(r => r.comparison[baseline.value])], dashed: true },
  ], {
    ...chartOptions(run, cursor), height: 240, emptyStep: .02, format: v => (v * 100).toFixed(0) + "%"
  });
}

function drawMatrix(labor) {
  const matrix = get("matrix");
  matrix.replaceChildren();
  for (let i = 0; i < statuses.length; i++) {
    const row = element("tr");
    const header = element("th", statuses[i]);
    header.scope = "row";
    row.append(header);
    for (let j = 0; j < statuses.length; j++) {
      const value = labor?.transitions[i][j];
      row.append(element("td", value === undefined ? "—" : count.format(value),
        i !== j && value > 0 ? (value >= 20 ? "move strong" : "move") : ""));
    }
    matrix.append(row);
  }
  set("labor-totals", labor ? labor.population.map((value, index) =>
    statuses[index] + " " + count.format(value)).join("  ·  ") : "Advance one month to see changes.");
}

function drawSectors(sectors) {
  const container = get("sectors");
  if (!sectors) {
    container.textContent = "Advance one month to inspect firms.";
    return;
  }
  container.replaceChildren();
  const max = Math.max(1, ...sectors.map(sector => sector.workers));
  for (const sector of sectors) {
    const row = element("div", undefined, "sector");
    const label = sector.sector[0].toUpperCase() + sector.sector.slice(1);
    const bar = element("meter");
    bar.max = max;
    bar.value = sector.workers;
    bar.setAttribute("aria-label", label + " workers");
    row.title = count.format(sector.active_firms) + " active firms";
    row.append(element("span", label), bar, element("span", count.format(sector.workers)));
    container.append(row);
  }
}

function render() {
  if (!run) return;
  const current = frames[cursor - 1];
  const frame = current?.frame;
  const labor = frame?.labor;
  const origin = run.origin;
  const future = current && !Number.isFinite(current.comparison.observed_unemployment);
  set("period", current ? formatPeriod(current.comparison.period) + (future ? " · Projection" : "") :
    "Opening · " + formatPeriod(origin.opening_month));
  const note = get("scenario-note");
  const scenarios = run.events || [];
  note.replaceChildren();
  if (scenarios.length) {
    for (const event of scenarios) {
      const line = element("div");
      line.append(element("strong", event.label + " · " + event.period + " "), document.createTextNode(event.description));
      note.append(line);
    }
  } else {
    note.textContent = "Endogenous baseline · Simulated outcomes continue beyond the historical archive.";
  }
  set("progress", frames.length + " / " + run.run.months + " months computed");
  set("unemployment", frame ? percent(frame.metrics.unemployment) : "—");
  set("actual", future ? "—" : percent(current?.comparison.observed_unemployment ?? origin.opening_unemployment));
  set("participation", labor ? "Labor force participation " + percent((labor.population[0] + labor.population[1]) / labor.population.reduce((a, b) => a + b)) : "Observed opening rate seeds the simulation");
  const gap = frame && !future ? 100 * (frame.metrics.unemployment - current.comparison.observed_unemployment) : null;
  set("gap", future ? "Beyond the observation archive" : gap === null
    ? "BLS Current Population Survey · opening vintage" : "Simulation gap " + (gap > 0 ? "+" : "") + gap.toFixed(1) + " pp");
  set("matches", labor ? count.format(labor.matches) : "—");
  set("hiring", labor ? count.format(labor.applications) + " applications · " + count.format(labor.vacancies) + " vacancies" : "Simulated applications and vacancies");
  drawChart();
  renderEconomy(run, frames, cursor);
  drawMatrix(labor);
  const posting = labor?.recruiting;
  set("posting-stock", posting ? "Vacancy stock · " + posting.opening_stock + " carried + " + posting.new_postings +
    " posted − " + posting.withdrawn + " withdrawn − " + labor.matches + " filled = " + posting.closing_stock + " left open" : "");
  drawSectors(frame?.sectors);

  const audit = get("audit");
  const source = element("a", "BLS CPS via ALFRED");
  source.href = "https://alfred.stlouisfed.org/category?cid=33502";
  source.target = "_blank";
  source.rel = "noopener";
  audit.replaceChildren(
    element("dt", "Labor archive"), element("dd"),
    element("dt", "Information cutoff"), element("dd", origin.as_of),
    element("dt", "Fit window"), element("dd", origin.training_start + " → " + origin.opening_month),
    element("dt", "Scoring vintage"), element("dd", origin.scoring_vintage),
    element("dt", "Population"), element("dd", count.format(origin.population.households) + " households · " +
      count.format(origin.population.firms) + " firms · " + origin.population.banks + " banks"),
    element("dt", "Ledger residual"), element("dd", frame ? frame.metrics.ledger_residual_minor_units + " minor units" : "—")
  );
  audit.querySelector("dd").append(source);
  controls();
}

get("reset").addEventListener("click", reset);
get("step").addEventListener("click", advance);
get("play").addEventListener("click", () => {
  if (playing) {
    stopPlayback();
    status("Paused between engine steps.");
  } else {
    playing = true;
    controls();
    void tick();
  }
});
get("cursor").addEventListener("input", event => {
  cursor = Number(event.target.value);
  stopPlayback();
  render();
  status("Inspecting a completed state.");
});
get("baseline").addEventListener("change", drawChart);
window.addEventListener("resize", () => { if (run) render(); });
function showView(name) {
  for (const view of ["economy", "labor"]) {
    get(view + "-tab").setAttribute("aria-pressed", String(view === name));
    get(view + "-view").hidden = view !== name;
  }
  if (run) render();
}
get("economy-tab").addEventListener("click", () => showView("economy"));
get("labor-tab").addEventListener("click", () => showView("labor"));
function selectOpeningDefaults() {
  const origin = origins.get(Number(get("origin").value));
  get("finance").value = origin?.finance ? "historical" : "synthetic";
  get("production").value = origin?.production ? "historical" : "synthetic";
  get("production").querySelector('[value="inventory"]').disabled = !origin?.inventory;
}
get("origin").addEventListener("change", () => { selectOpeningDefaults(); void reset(); });
for (const control of ["credit-view", "composition-view", "output-view"]) {
  get(control).addEventListener("change", () => { if (run) renderEconomy(run, frames, cursor); });
}
get("seed").addEventListener("change", reset);

function drawEvidence(evaluation, seeds) {
  const evidence = get("evidence");
  if (!evaluation) {
    evidence.replaceChildren(element("p", "Backtest scores are unavailable for these exact run inputs."));
    return;
  }
  const scores = evaluation.mean_absolute_error_percentage_points;
  const models = [
    ["agent_engine", "Agent engine"], ["persistence", "Opening rate"],
    ["ar1", "AR(1)"], ["flows_only", "Flows only"]
  ];
  if (Number.isFinite(scores.worker_only_engine)) {
    models.splice(1, 0, ["worker_only_engine", "Worker-only engine"]);
    evidence.classList.add("with-control");
  }
  evidence.replaceChildren();
  for (const [key, label] of models) {
    const cell = element("div", undefined, "evidence-cell");
    const score = element("strong", scores[key].toFixed(2) + " pp", key === "agent_engine" ? "accent" : "");
    cell.append(element("span", label), score);
    evidence.append(cell);
  }
  const note = evaluation.matching.application_fill_share;
  evidence.append(element("p", evaluation.scored_months + " observed months · " + seeds.length +
    " seeds per opening · firms accepted " + (100 * note).toFixed(2) + "%" + " of simulated applications", "evidence-note"));
  const finance = evaluation.bank_opening;
  if (finance) {
    const panel = get("finance-evidence");
    panel.hidden = false;
    const credit = finance.loan_to_deposit_mae_pp;
    const labor = finance.unemployment_mae_pp;
    panel.replaceChildren(
      element("h2", "Dated bank opening check"),
      element("p", finance.matched_months + " matched months · unemployment MAE " +
        labor.bank_opening_unemployment.toFixed(2) + " pp dated vs " + labor.agent_engine.toFixed(2) + " pp synthetic."),
      element("p", "Loan-to-deposit MAE " + credit.bank_opening_loan_to_deposit.toFixed(2) +
        " pp dated vs " + credit.persistent_opening_loan_to_deposit.toFixed(2) + " pp for carrying the opening ratio forward.")
    );
  }
  const production = evaluation.production;
  if (production) {
    const panel = get("production-evidence");
    panel.hidden = false;
    const entries = [
      ["Payroll mix", production.payroll_share_error_pp],
      ["Household basket", production.consumption_share_error_pp],
      ["Real GDP growth", production.gdp_growth_error_pp],
    ];
    panel.replaceChildren(element("h2", "Dated sector opening check"), ...entries.map(([label, row]) =>
      element("p", label + " · " + row.periods + " periods · " + row.dated.toFixed(2) + " pp dated · " +
        row.synthetic.toFixed(2) + " pp synthetic · " + row.opening_persistence.toFixed(2) + " pp persistence")
    ));
  }
  const inventory = evaluation.inventory;
  if (inventory) {
    const panel = get("inventory-evidence");
    panel.hidden = false;
    const gdp = inventory.gdp_growth_mae_pp;
    const stocks = inventory.inventory_ratio_mae_months;
    panel.replaceChildren(
      element("h2", "Inventory adjustment trial"),
      element("p", "GDP growth MAE · " + gdp.periods + " quarters · " + gdp.candidate.toFixed(2) +
        " pp inventory trial · " + gdp.legacy_fixed_price.toFixed(2) + " pp valuation only · " +
        gdp.opening_persistence.toFixed(2) + " pp persistence"),
      element("p", "Stock-to-sales MAE · " + stocks.periods + " months · " + stocks.candidate.toFixed(2) +
        " months trial · " + stocks.legacy.toFixed(2) + " months previous model · " +
        stocks.opening_persistence.toFixed(2) + " months persistence")
    );
  }
}

async function init() {
  get("reset").disabled = true;
  get("step").disabled = true;
  get("play").disabled = true;
  try {
    const config = await request("config");
    drawEvidence(config.evaluation, config.seeds);
    get("end-year").value = config.default_end_year || 2028;
    get("end-year").max = config.max_end_year || 2100;
    const presets = config.presets || { baseline: "Endogenous baseline" };
    for (const [id, label] of Object.entries(presets)) get("preset").add(new Option(label, id));
    for (const origin of config.origins) {
      origins.set(origin.year, origin);
      get("origin").add(new Option(origin.year, origin.year));
    }
    for (const seed of config.seeds) get("seed").add(new Option(seed, seed));
    get("origin").value = "2020";
    selectOpeningDefaults();
    await reset();
  } catch (error) {
    status(error.message, true);
  }
}
void init();
