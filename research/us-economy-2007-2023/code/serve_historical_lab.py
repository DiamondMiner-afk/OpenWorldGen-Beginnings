"""Serve the private localhost dashboard with demand-driven Rust engine steps."""

import argparse
import json
import secrets
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from tempfile import TemporaryDirectory

from historical_lab import BINARY, DEFAULT_END_YEAR, OUTPUT, digest, engine_command, prepare, save

ASSETS = Path(__file__).with_name("dashboard")
STATIC = {
    "/": ("index.html", "text/html; charset=utf-8"),
    "/app.js": ("app.js", "text/javascript; charset=utf-8"),
    "/app.css": ("app.css", "text/css; charset=utf-8"),
    "/economy.js": ("economy.js", "text/javascript; charset=utf-8"),
}
MAX_SESSIONS = 16
SESSION_SECONDS = 1800
MAX_END_YEAR = 2100
PRESETS = {
    "baseline": "Endogenous baseline",
    "covid": "COVID replay",
    "ai": "AI adoption scenario",
    "covid-ai": "COVID replay + AI scenario",
}


def scenario_with_events(template, year, end_year, preset):
    calendar = {**template["calendar"], "months": (end_year - year + 1) * 12}
    scenario = {**template, "calendar": calendar, "shocks": []}
    events = []

    def add_event(event_year, event_month, label, description, shocks):
        offset = (event_year - year) * 12 + event_month - 1
        if not 0 <= offset < calendar["months"]:
            return
        events.append({"period": f"{event_year}-{event_month:02d}", "month": offset, "label": label, "description": description})
        for shock in shocks:
            duration = min(shock["duration_months"], calendar["months"] - offset)
            decay = min(shock.get("decay_months", 0), calendar["months"] - offset - duration)
            scenario["shocks"].append({**shock, "start_month": offset, "duration_months": duration, "decay_months": decay})

    if preset in ("covid", "covid-ai"):
        add_event(2020, 4, "COVID replay", "April 2020 onset. Household confidence −18%, service capacity −25%, manufacturing capacity −12% for three months, then a nine-month recovery. Magnitudes are scenario assumptions.", [
            {"id": "covid-confidence", "type": "confidence", "multiplier": .82, "duration_months": 3, "decay_months": 9},
            {"id": "covid-services", "type": "supply_capacity", "sector": "services", "multiplier": .75, "duration_months": 3, "decay_months": 9},
            {"id": "covid-manufacturing", "type": "supply_capacity", "sector": "manufacturing", "multiplier": .88, "duration_months": 3, "decay_months": 9},
        ])
    if preset in ("ai", "covid-ai"):
        add_event(2026, 1, "AI adoption", "Hypothetical January 2026 onset. Service productivity +10% and manufacturing productivity +5% through the end of the run.", [
            {"id": "ai-services", "type": "productivity", "sector": "services", "multiplier": 1.10, "duration_months": calendar["months"]},
            {"id": "ai-manufacturing", "type": "productivity", "sector": "manufacturing", "multiplier": 1.05, "duration_months": calendar["months"]},
        ])
    return scenario, events



class Run:
    def __init__(self, binary, folder, origin, seed, end_year, preset, bank_opening, production_opening, inventory):
        self.lock = threading.Lock()
        self.touched = time.monotonic()
        self.year = origin["year"]
        for name, expected in origin["files"].items():
            if digest(folder / name) != expected:
                raise ValueError(f"changed prepared input {name}; restart the lab")
        template = json.loads((folder / "live-scenario.json").read_text())
        scenario, self.events = scenario_with_events(template, self.year, end_year, preset)
        self.preset = preset
        self.bank_opening = bank_opening
        self.production_opening = production_opening
        self.inventory = inventory
        self.horizon = scenario["calendar"]["months"]
        self.observed = json.loads((folder / "observed.json").read_text())
        self.month = 0
        with TemporaryDirectory(prefix="owg-live-") as tmp:
            scenario_path = Path(tmp) / "scenario.json"
            save(scenario_path, scenario)
            self.process = subprocess.Popen(
                engine_command(binary, folder, seed, step_input=True, scenario_path=scenario_path, finance=bank_opening,
                               production=production_opening, inventory=inventory),
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                text=True, encoding="utf-8",
            )
            try:
                self.header = self.read_frame()
                if (
                    self.header["months"] != self.horizon
                    or self.header["source_manifest_sha256"] != origin["source_manifest_sha256"]
                    or self.header["labor_sha256"] != digest(folder / "labor.json")
                    or (bank_opening and (
                        self.header["finance_sha256"] != origin["files"]["finance.json"]
                        or self.header["opening_finance"]["source_sha256"] != origin["finance"]["source_sha256"]
                    ))
                    or (production_opening and (
                        self.header["production_sha256"] != origin["files"]["inventory.json" if inventory else "production.json"]
                        or self.header["opening_production"]["source_sha256"] != origin["production"]["source_sha256"]
                    ))
                    or (inventory and self.header["opening_production"]["inventory"]["source_sha256"]
                        != origin["inventory"]["source_sha256"])
                ):
                    raise ValueError("engine run inputs differ from the prepared origin")
            except BaseException:
                self.close()
                raise

    def read_frame(self):
        line = self.process.stdout.readline()
        if not line:
            detail = self.process.stderr.read().strip()
            raise RuntimeError(detail or "engine closed before producing a frame")
        return json.loads(line)

    def step(self):
        with self.lock:
            if self.month >= self.horizon:
                raise ValueError("run is already complete")
            self.touched = time.monotonic()
            self.process.stdin.write("step\n")
            self.process.stdin.flush()
            frame = self.read_frame()
            period = f"{self.year + self.month // 12}-{1 + self.month % 12:02d}"
            actual = self.observed[self.month] if self.month < len(self.observed) else {"period": period}
            metrics = frame["metrics"]
            if f"{metrics['year']}-{metrics['month_of_year']:02d}" != actual["period"]:
                raise ValueError("engine frame calendar differs from the archive")
            self.month += 1
            return {"frame": frame, "comparison": actual, "done": self.month == self.horizon}

    def close(self):
        with self.lock:
            if self.process.poll() is None:
                self.process.terminate()
                try:
                    self.process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait()
            for pipe in (self.process.stdin, self.process.stdout, self.process.stderr):
                pipe.close()


def read_evidence(folder):
    try:
        evidence = json.loads((folder / "summary.json").read_text())
        if (evidence["protocol_sha256"] == digest(folder / "protocol.json")
                and evidence["scores_sha256"] == digest(folder / "scores.csv")
                and ("production" not in evidence or
                     evidence["production"]["errors_sha256"] == digest(folder / "production-errors.json"))
                and ("inventory" not in evidence or
                     evidence["inventory"]["diagnostics_sha256"] == digest(folder / "inventory-diagnostics.json"))):
            return evidence
    except (FileNotFoundError, ValueError, KeyError, TypeError):
        pass
    return None


class LabServer(ThreadingHTTPServer):
    def __init__(self, port, binary, folder, protocol):
        super().__init__(("127.0.0.1", port), Handler)
        self.binary = binary
        self.folder = folder
        self.protocol = protocol
        self.evidence = read_evidence(folder)
        self.origins = {origin["year"]: origin for origin in protocol["origins"]}
        self.sessions = {}
        self.lock = threading.Lock()

    def start_run(self, year, seed, end_year, preset, bank_opening, production_opening, inventory):
        with self.lock:
            expired = [key for key, run in self.sessions.items() if time.monotonic() - run.touched > SESSION_SECONDS]
            for key in expired:
                self.sessions.pop(key).close()
            if len(self.sessions) >= MAX_SESSIONS:
                raise ValueError("too many open runs")
            origin = self.origins[year]
            run = Run(self.binary, self.folder / str(year), origin, seed, end_year, preset, bank_opening, production_opening, inventory)
            token = secrets.token_urlsafe(24)
            self.sessions[token] = run
            return {"token": token, "origin": origin, "run": run.header, "events": run.events, "preset": run.preset, "bank_opening": run.bank_opening, "production_opening": run.production_opening, "inventory": run.inventory}

    def remove(self, token):
        with self.lock:
            run = self.sessions.pop(token, None)
        if run:
            run.close()

    def server_close(self):
        super().server_close()
        with self.lock:
            for run in self.sessions.values():
                run.close()
            self.sessions.clear()


class Handler(BaseHTTPRequestHandler):
    def send_payload(self, status, mime, payload):
        self.send_response(status)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Content-Security-Policy", "default-src 'self'; style-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'")
        self.end_headers()
        self.wfile.write(payload)

    def send_json(self, payload, status=200):
        self.send_payload(status, "application/json", json.dumps(payload, allow_nan=False).encode())

    def do_GET(self):
        if self.path in STATIC:
            asset, mime = STATIC[self.path]
            self.send_payload(200, mime, (ASSETS / asset).read_bytes())
        elif self.path == "/api/config":
            self.send_json({**self.server.protocol, "evaluation": self.server.evidence,
                            "max_end_year": MAX_END_YEAR, "presets": PRESETS})
        else:
            self.send_json({"error": "Route not found"}, 404)

    def do_POST(self):
        port = self.server.server_port
        if self.headers.get("Host") != f"127.0.0.1:{port}" or self.headers.get("Origin") != f"http://127.0.0.1:{port}":
            self.send_json({"error": "Use the local dashboard URL"}, 403)
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= 2048:
                raise ValueError("invalid request length")
            request = json.loads(self.rfile.read(length))
            if not isinstance(request, dict):
                raise ValueError("expected an object")
            if self.path == "/api/start":
                year, seed = request["year"], request["seed"]
                if type(year) is not int or year not in self.server.origins or type(seed) is not int or seed not in self.server.protocol["seeds"]:
                    raise ValueError("unknown origin or seed")
                end_year = request.get("end_year", DEFAULT_END_YEAR)
                preset = request.get("preset", "baseline")
                if type(end_year) is not int or not year <= end_year <= MAX_END_YEAR or not isinstance(preset, str) or preset not in PRESETS:
                    raise ValueError("invalid end year or scenario")
                bank_opening = request.get("bank_opening", False)
                if type(bank_opening) is not bool or (bank_opening and "finance" not in self.server.origins[year]):
                    raise ValueError("dated bank inputs unavailable for this origin")
                production_opening = request.get("production_opening", False)
                if type(production_opening) is not bool or (production_opening and "production" not in self.server.origins[year]):
                    raise ValueError("dated production inputs unavailable for this origin")
                inventory = request.get("inventory", False)
                if type(inventory) is not bool or (inventory and (
                    not production_opening or "inventory" not in self.server.origins[year]
                )):
                    raise ValueError("dated inventory inputs unavailable for this configuration")
                self.send_json(self.server.start_run(year, seed, end_year, preset, bank_opening, production_opening, inventory))
                return
            token = request["token"]
            if not isinstance(token, str):
                raise ValueError("invalid run token")
            if self.path == "/api/stop":
                self.server.remove(token)
                self.send_json({"stopped": True})
                return
            if self.path != "/api/step":
                self.send_json({"error": "Route not found"}, 404)
                return
            with self.server.lock:
                run = self.server.sessions.get(token)
            if run is None:
                self.send_json({"error": "Run expired or completed. Start a new run."}, 404)
                return
            try:
                result = run.step()
            except (ValueError, OSError, RuntimeError):
                self.server.remove(token)
                raise
            if result["done"]:
                self.server.remove(token)
            self.send_json(result)
        except (ValueError, KeyError, TypeError) as error:
            self.send_json({"error": str(error)}, 400)
        except (OSError, RuntimeError) as error:
            self.send_json({"error": str(error)}, 500)

    def log_message(self, *_):
        pass


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=BINARY)
    parser.add_argument("--data", type=Path, default=OUTPUT)
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--worker-only", action="store_true")
    parser.add_argument("--finance", action="store_true")
    parser.add_argument("--production", action="store_true")
    parser.add_argument("--inventory", action="store_true")
    args = parser.parse_args()
    binary, data = args.binary.resolve(), args.data.resolve()
    protocol = prepare(binary, data, recruiting=not args.worker_only, finance=args.finance, production=args.production, inventory=args.inventory)
    with LabServer(args.port, binary, data, protocol) as server:
        print(f"OpenWorldGen live lab at http://127.0.0.1:{server.server_port}/", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass
