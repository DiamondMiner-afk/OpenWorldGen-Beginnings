# OpenWorldGen Python SDK

The SDK provides a typed, notebook-friendly control layer over the independent
Rust engine. It invokes the `owg` process without a shell, so the simulation
kernel remains language-neutral and all manifests are identical to CLI runs.

Python 3.13 is the supported interpreter for version 0.1.

```python
from pathlib import Path
from openworldgen import OpenWorldGen

owg = OpenWorldGen()
print(owg.capabilities()["implemented_systems"])
result = owg.compare(
    Path("../scenarios/v0.1/inflation_lab.yaml"),
    output=Path("../outputs/inflation_lab"),
    replications=8,
)
verification = owg.verify_bundle(result.path)
assert verification.valid
print(result.manifest["validation_status"])
print(result.comparison["pareto_frontier"])
print(result.mechanisms)
print(result.accounting)
print(result.visualization)  # bundled, checksum-covered plots.html
owg.plot(result.path, output=Path("../outputs/inflation-lab-shareable.html"))
```

Set `OWG_BIN` when `owg` is not installed on `PATH`.

`compare(..., replications=None)` uses the scenario's adaptive Monte Carlo
stopping rule and may be intentionally expensive. Pass an explicit replication
count only for a declared fixed design or an engineering smoke run.
