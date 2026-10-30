# Recorded firm law gate: bounded component

Status: **development simulation component**, not historical legal validation or an engine-scheduled AI policy. See [[IMPLEMENTATION_STATUS]] and [[HISTORICAL_INPUT_GATE]].

## Precise claim

For the one supported recorded firm action, a direct cash transfer to a household with a positive direct ownership stake, the opt-in dispatcher checks a caller-supplied action-to-law binding against the mutable governance law effective at the **start of `Economy.month`**. A governed transfer without that binding fails closed. The existing accounting settlement then checks the firm actor, ownership, liquidity, bank reserves, and balance-sheet invariants. A failed action leaves both the economy and the recorded-policy cursor unchanged.

This is a scenario-defined permission bit. `LawRule.enabled` is not a real-world corporate-law determination and does not establish that a distribution is a dividend, lawful under any jurisdiction, or tax-qualified.

## Contract and timing

- `FirmActionLawBinding` names its own `owg.firm-action-law-binding.v1` version, the governance contract version, law ID, and domain. The AI action cannot pick the binding; the caller supplies it.
- The bound law must exist in that domain. At a pre-month boundary, `GovernanceState.next_month` must equal `Economy.month`; any mismatch rejects the transfer. Accepted amendments due at this boundary are evaluated in the same effective-month/proposal-ID order as governance advancement, without mutating the state. Proposals submitted during the current month cannot take effect at this boundary.
- The outcome records the binding version, governance version, law ID, domain, and effective month used for a successful transfer. No-op actions remain possible even when the law is disabled.
- `dispatch_recorded_firm_action` rejects a governed direct-owner transfer with no binding. An economy with no governance can still use the old synthetic transfer path, which carries no law authorization in the outcome.
- The opt-in `SimulationEngine::run_recorded_firm` calls this component at the pre-month boundary, outside the ordinary scheduler and checkpoint path. `Economy::step_month` then advances governance **before** its scheduler runs; an eventual scheduled in-engine decision phase needs its own already-advanced legal-state check. Do not drop this pre-month query into that phase unchanged.

## Adversarial checks

The focused tests cover an enabled month, a law disabled by a month-0 amendment effective in month 1, and a month-1 re-enactment effective in month 2. They also reject an ungated governed transfer, wrong law domain, mismatched binding/governance version, and incorrect governance phase, while keeping the tape cursor and economy unchanged. Existing tests still check wrong owner, insufficient funds, wrong actor, changed observation, accounting closure, and unsupported actions.

## Missing gates

The opt-in run hashes the caller-supplied binding, but the mapping is not declared in the scenario or bound to the ordinary checkpoint/result-bundle contract. A caller able to create a new binding and run could point at another enabled law; this API must not be treated as an authoritative legal screen until the mapping itself is scenario-bound and replay-checked. The v1 firm observation does not tell the actor the applicable law, so a recorded AI choice cannot be claimed to have considered it. The model also lacks corporate board authority, insolvency-distribution rules, taxes, enforcement, jurisdictional conflict, and deliberate illegal conduct/detection. These are separate mechanisms, not implicit guarantees of this component.

## Next falsification increment

Add a versioned scenario action-law mapping with explicit jurisdiction and effective dates; bind it to checkpoints/results; schedule a strategic decision after governance advancement; then test tampered mappings, law changes, wrong jurisdiction, replay equivalence, and the legal/illegal-but-attempted distinction. Preserve the existing rule-policy baseline when evaluating any local LLM.
