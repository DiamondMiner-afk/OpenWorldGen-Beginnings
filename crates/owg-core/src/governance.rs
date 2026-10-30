//! Scenario-configurable political influence, laws, and institutional gates.
//!
//! This is a deterministic process experiment, not a calibrated model of US
//! politics. Influence weights are dimensionless political efficacy, not money
//! or a permanent privilege for any actor class.

use crate::contracts::PolicyActionKind;
use crate::error::{OwgError, OwgResult};
use crate::institutions::{
    AuthorityAppointment, EconomicRole, INSTITUTION_GRAPH_AUTHORITY_VERSION, InstitutionGraph,
    LegalForm,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const GOVERNANCE_VERSION: &str = "owg.governance.v1";
/// Opt-in authority-aware process. V1 remains readable and keeps its original
/// meaning; V2 requires a declared public decision authority for every change.
pub const GOVERNANCE_AUTHORITY_VERSION: &str = "owg.governance.v2";
/// Opt-in scenario-bound public-office appointment and jurisdiction checks.
pub const GOVERNANCE_INSTITUTIONAL_VERSION: &str = "owg.governance.v3";

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct InfluenceGroup {
    pub id: String,
    pub label: String,
    /// Relative political efficacy. No group type has a hard-coded advantage.
    pub weight: f64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct InstitutionalGate {
    pub id: String,
    pub domain: String,
    pub passage_threshold: f64,
    /// Additional resistance when altering a law or the gate itself.
    pub amendment_surcharge: f64,
    /// An accepted change cannot take effect until the next month at earliest.
    pub review_delay_months: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct LawRule {
    pub id: String,
    pub domain: String,
    pub enabled: bool,
    /// Extra barrier to policy enactment or alteration of this rule.
    pub resistance: f64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct GroupStance {
    pub group_id: String,
    /// -1 opposes, +1 supports, 0 is neutral.
    pub value: f64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GovernanceChange {
    /// V2 envelope: a sponsoring group may petition, but this public office
    /// must hold the domain's decision authority when the proposal is heard.
    AuthorityDecision {
        authority_id: String,
        change: Box<GovernanceChange>,
    },
    /// V3 envelope. The actor must hold a current appointment to this public
    /// office in the scenario's institution graph, not merely name the gate.
    ActorDecision {
        actor_id: String,
        authority_id: String,
        change: Box<GovernanceChange>,
    },
    EconomicPolicy {
        law_id: String,
        duration_months: u32,
        action: PolicyActionKind,
    },
    AmendLaw {
        law_id: String,
        enabled: bool,
        resistance: f64,
    },
    AmendGate {
        gate_id: String,
        passage_threshold: f64,
        amendment_surcharge: f64,
        review_delay_months: u32,
    },
    ChangeInfluence {
        group_id: String,
        weight: f64,
    },
    /// Reassign one domain to another declared public office. The change is
    /// screened by the *current* gate and authority, then takes effect later.
    TransferAuthority {
        gate_id: String,
        new_authority_id: String,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct GovernanceProposal {
    pub id: String,
    pub submitted_month: u32,
    pub domain: String,
    pub sponsor_id: String,
    pub stances: Vec<GroupStance>,
    pub change: GovernanceChange,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct GovernanceSpec {
    pub schema_version: String,
    pub groups: Vec<InfluenceGroup>,
    pub gates: Vec<InstitutionalGate>,
    pub laws: Vec<LawRule>,
    pub proposals: Vec<GovernanceProposal>,
}

impl GovernanceSpec {
    pub fn validate(&self, horizon_months: u32) -> OwgResult<()> {
        if (self.schema_version != GOVERNANCE_VERSION
            && self.schema_version != GOVERNANCE_AUTHORITY_VERSION
            && self.schema_version != GOVERNANCE_INSTITUTIONAL_VERSION)
            || self.groups.is_empty()
            || self.gates.is_empty()
        {
            return Err(invalid("unsupported or empty governance specification"));
        }
        let mut group_ids = BTreeSet::new();
        for group in &self.groups {
            if group.id.trim().is_empty()
                || !group.weight.is_finite()
                || group.weight < 0.0
                || !group_ids.insert(group.id.as_str())
            {
                return Err(invalid("invalid or duplicate influence group"));
            }
        }
        let mut gate_ids = BTreeSet::new();
        let mut domains = BTreeSet::new();
        for gate in &self.gates {
            if gate.id.trim().is_empty()
                || gate.domain.trim().is_empty()
                || !unit_interval(gate.passage_threshold)
                || !unit_interval(gate.amendment_surcharge)
                || !gate_ids.insert(gate.id.as_str())
                || !domains.insert(gate.domain.as_str())
            {
                return Err(invalid("invalid or duplicate institutional gate/domain"));
            }
        }
        let mut laws = BTreeMap::new();
        for law in &self.laws {
            if law.id.trim().is_empty()
                || !domains.contains(law.domain.as_str())
                || !unit_interval(law.resistance)
                || laws.insert(law.id.as_str(), law.domain.as_str()).is_some()
            {
                return Err(invalid("invalid or duplicate law rule"));
            }
        }
        let mut proposal_ids = BTreeSet::new();
        for proposal in &self.proposals {
            if proposal.id.trim().is_empty()
                || proposal.submitted_month >= horizon_months
                || !proposal_ids.insert(proposal.id.as_str())
                || !domains.contains(proposal.domain.as_str())
                || !group_ids.contains(proposal.sponsor_id.as_str())
            {
                return Err(invalid("invalid governance proposal identity or date"));
            }
            let mut stance_ids = BTreeSet::new();
            let mut sponsor_supports = false;
            for stance in &proposal.stances {
                if !group_ids.contains(stance.group_id.as_str())
                    || !unit_symmetric(stance.value)
                    || !stance_ids.insert(stance.group_id.as_str())
                {
                    return Err(invalid("invalid or duplicate governance stance"));
                }
                if stance.group_id == proposal.sponsor_id && stance.value > 0.0 {
                    sponsor_supports = true;
                }
            }
            if !sponsor_supports {
                return Err(invalid("proposal sponsor must explicitly support it"));
            }
            let change = if self.schema_version == GOVERNANCE_INSTITUTIONAL_VERSION {
                match &proposal.change {
                    GovernanceChange::ActorDecision {
                        actor_id,
                        authority_id,
                        change,
                    } if !actor_id.trim().is_empty()
                        && gate_ids.contains(authority_id.as_str())
                        && !matches!(
                            change.as_ref(),
                            GovernanceChange::AuthorityDecision { .. }
                                | GovernanceChange::ActorDecision { .. }
                        ) =>
                    {
                        change.as_ref()
                    }
                    _ => {
                        return Err(invalid(
                            "v3 proposal requires one actor and declared public office",
                        ));
                    }
                }
            } else if self.schema_version == GOVERNANCE_AUTHORITY_VERSION {
                match &proposal.change {
                    GovernanceChange::AuthorityDecision {
                        authority_id,
                        change,
                    } if !authority_id.trim().is_empty()
                        && gate_ids.contains(authority_id.as_str())
                        && !matches!(
                            change.as_ref(),
                            GovernanceChange::AuthorityDecision { .. }
                                | GovernanceChange::ActorDecision { .. }
                        ) =>
                    {
                        change.as_ref()
                    }
                    _ => {
                        return Err(invalid(
                            "v2 proposal requires one declared public authority",
                        ));
                    }
                }
            } else {
                if matches!(
                    &proposal.change,
                    GovernanceChange::AuthorityDecision { .. }
                        | GovernanceChange::ActorDecision { .. }
                        | GovernanceChange::TransferAuthority { .. }
                ) {
                    return Err(invalid("authority decisions require governance v2"));
                }
                &proposal.change
            };
            match change {
                GovernanceChange::AuthorityDecision { .. }
                | GovernanceChange::ActorDecision { .. } => {
                    return Err(invalid("nested authority decision is not permitted"));
                }
                GovernanceChange::EconomicPolicy {
                    law_id,
                    duration_months,
                    action,
                } => {
                    if laws.get(law_id.as_str()) != Some(&proposal.domain.as_str())
                        || *duration_months == 0
                        || !valid_policy_action(action)
                    {
                        return Err(invalid("invalid governed economic policy"));
                    }
                }
                GovernanceChange::AmendLaw {
                    law_id, resistance, ..
                } => {
                    if laws.get(law_id.as_str()) != Some(&proposal.domain.as_str())
                        || !unit_interval(*resistance)
                    {
                        return Err(invalid("invalid law amendment"));
                    }
                }
                GovernanceChange::AmendGate {
                    gate_id,
                    passage_threshold,
                    amendment_surcharge,
                    ..
                } => {
                    let gate = self.gates.iter().find(|gate| gate.id == *gate_id);
                    if gate.is_none_or(|gate| gate.domain != proposal.domain)
                        || !unit_interval(*passage_threshold)
                        || !unit_interval(*amendment_surcharge)
                    {
                        return Err(invalid("invalid gate amendment"));
                    }
                }
                GovernanceChange::ChangeInfluence { group_id, weight } => {
                    if !group_ids.contains(group_id.as_str())
                        || !weight.is_finite()
                        || *weight < 0.0
                    {
                        return Err(invalid("invalid influence change"));
                    }
                }
                GovernanceChange::TransferAuthority {
                    gate_id,
                    new_authority_id,
                } => {
                    let gate = self.gates.iter().find(|gate| gate.id == *gate_id);
                    if gate.is_none_or(|gate| gate.domain != proposal.domain)
                        || !gate_ids.contains(new_authority_id.as_str())
                    {
                        return Err(invalid("invalid public authority transfer"));
                    }
                }
            }
        }
        Ok(())
    }

    /// V3 cross-contract validation: a gate names a public office in this
    /// region, and every proposed decision actor is a known natural person.
    /// Unappointed known actors remain valid proposals so denial is auditable.
    pub fn validate_with_institutions(
        &self,
        horizon_months: u32,
        graph: &InstitutionGraph,
        region_id: &str,
    ) -> OwgResult<()> {
        self.validate(horizon_months)?;
        if self.schema_version != GOVERNANCE_INSTITUTIONAL_VERSION {
            return Ok(());
        }
        graph.validate()?;
        if graph.schema_version != INSTITUTION_GRAPH_AUTHORITY_VERSION
            || region_id.trim().is_empty()
        {
            return Err(invalid(
                "v3 governance requires an authority-aware institution graph and region",
            ));
        }
        for gate in &self.gates {
            let office = graph.entities.iter().find(|entity| entity.id == gate.id);
            if office.is_none_or(|entity| {
                entity.economic_role != EconomicRole::PublicAuthority
                    || entity.jurisdiction != region_id
            }) {
                return Err(invalid(
                    "v3 gate must bind to a public office in the scenario region",
                ));
            }
        }
        for proposal in &self.proposals {
            let GovernanceChange::ActorDecision { actor_id, .. } = &proposal.change else {
                return Err(invalid("v3 proposal has no decision actor"));
            };
            if !graph.entities.iter().any(|entity| {
                entity.id == *actor_id
                    && entity.legal_form == LegalForm::NaturalPerson
                    && entity.jurisdiction == region_id
            }) {
                return Err(invalid(
                    "v3 decision actor must be a known natural person in the scenario region",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct GovernanceEvent {
    pub month: u32,
    pub proposal_id: String,
    /// V3 decision provenance. These fields identify declared scenario actors,
    /// not authenticated real-world officials or external model callers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_id: Option<String>,
    pub phase: String,
    pub outcome: String,
    pub support: Option<f64>,
    pub required: Option<f64>,
    pub effective_month: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct EnactedPolicy {
    pub proposal_id: String,
    pub start_month: u32,
    pub end_month: u32,
    pub action: PolicyActionKind,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
struct PendingChange {
    proposal_id: String,
    effective_month: u32,
    /// Present only for v3, preserving the serialized v1/v2 state shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    domain: Option<String>,
    change: GovernanceChange,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
struct InstitutionalAuthorityRoster {
    jurisdiction: String,
    appointments: Vec<AuthorityAppointment>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct GovernanceState {
    pub next_month: u32,
    pub groups: Vec<InfluenceGroup>,
    pub gates: Vec<InstitutionalGate>,
    pub laws: Vec<LawRule>,
    pub policies: Vec<EnactedPolicy>,
    pub events: Vec<GovernanceEvent>,
    pending: Vec<PendingChange>,
    /// V2 only. The office IDs are declared by institutional gates. Empty in
    /// V1 to preserve its serialized state shape and checkpoint compatibility.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    authority_by_domain: BTreeMap<String, String>,
    /// V3 only. Reconstructed and compared with the scenario on resume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    institutional_authority_roster: Option<InstitutionalAuthorityRoster>,
}

impl GovernanceState {
    pub fn new(spec: &GovernanceSpec, horizon_months: u32) -> OwgResult<Self> {
        spec.validate(horizon_months)?;
        if spec.schema_version == GOVERNANCE_INSTITUTIONAL_VERSION {
            return Err(invalid("v3 governance requires institution graph binding"));
        }
        Ok(Self::new_validated(spec, None))
    }

    pub fn new_with_institutions(
        spec: &GovernanceSpec,
        horizon_months: u32,
        graph: &InstitutionGraph,
        region_id: &str,
    ) -> OwgResult<Self> {
        spec.validate_with_institutions(horizon_months, graph, region_id)?;
        let roster = (spec.schema_version == GOVERNANCE_INSTITUTIONAL_VERSION).then(|| {
            InstitutionalAuthorityRoster {
                jurisdiction: region_id.to_owned(),
                appointments: graph.authority_appointments.clone(),
            }
        });
        Ok(Self::new_validated(spec, roster))
    }

    fn new_validated(spec: &GovernanceSpec, roster: Option<InstitutionalAuthorityRoster>) -> Self {
        Self {
            next_month: 0,
            groups: spec.groups.clone(),
            gates: spec.gates.clone(),
            laws: spec.laws.clone(),
            policies: Vec::new(),
            events: Vec::new(),
            pending: Vec::new(),
            authority_by_domain: if spec.schema_version == GOVERNANCE_AUTHORITY_VERSION
                || spec.schema_version == GOVERNANCE_INSTITUTIONAL_VERSION
            {
                spec.gates
                    .iter()
                    .map(|gate| (gate.domain.clone(), gate.id.clone()))
                    .collect()
            } else {
                BTreeMap::new()
            },
            institutional_authority_roster: roster,
        }
    }

    pub fn advance(&mut self, spec: &GovernanceSpec, month: u32) -> OwgResult<()> {
        if self.next_month != month {
            return Err(invalid("governance month is not sequential"));
        }
        let expects_authority_map = spec.schema_version != GOVERNANCE_VERSION;
        let has_authority_map = !self.authority_by_domain.is_empty();
        if (spec.schema_version == GOVERNANCE_INSTITUTIONAL_VERSION)
            != self.institutional_authority_roster.is_some()
            || expects_authority_map != has_authority_map
        {
            return Err(invalid(
                "governance state and specification versions disagree",
            ));
        }
        let mut due = std::mem::take(&mut self.pending);
        due.sort_by(|left, right| {
            left.effective_month
                .cmp(&right.effective_month)
                .then(left.proposal_id.cmp(&right.proposal_id))
        });
        let mut remaining = Vec::new();
        for pending in due {
            if pending.effective_month == month {
                let (actor_id, authority_id) = match &pending.change {
                    GovernanceChange::ActorDecision {
                        actor_id,
                        authority_id,
                        ..
                    } => (Some(actor_id.clone()), Some(authority_id.clone())),
                    _ => (None, None),
                };
                self.apply(&pending)?;
                self.events.push(GovernanceEvent {
                    month,
                    proposal_id: pending.proposal_id,
                    domain: pending.domain,
                    actor_id,
                    authority_id,
                    phase: "effective".to_owned(),
                    outcome: "applied".to_owned(),
                    support: None,
                    required: None,
                    effective_month: Some(month),
                });
            } else {
                remaining.push(pending);
            }
        }
        self.pending = remaining;
        let mut submitted: Vec<&GovernanceProposal> = spec
            .proposals
            .iter()
            .filter(|proposal| proposal.submitted_month == month)
            .collect();
        submitted.sort_by(|left, right| left.id.cmp(&right.id));
        for proposal in submitted {
            self.evaluate(proposal, month, &spec.schema_version)?;
        }
        self.next_month += 1;
        Ok(())
    }

    fn evaluate(
        &mut self,
        proposal: &GovernanceProposal,
        month: u32,
        schema_version: &str,
    ) -> OwgResult<()> {
        let gate = self
            .gates
            .iter()
            .find(|gate| gate.domain == proposal.domain)
            .ok_or_else(|| invalid("proposal domain has no gate"))?;
        let (declared_authority, actor_id, change) = match (&proposal.change, schema_version) {
            (
                GovernanceChange::AuthorityDecision {
                    authority_id,
                    change,
                },
                GOVERNANCE_AUTHORITY_VERSION,
            ) => (Some(authority_id.as_str()), None, change.as_ref()),
            (
                GovernanceChange::ActorDecision {
                    actor_id,
                    authority_id,
                    change,
                },
                GOVERNANCE_INSTITUTIONAL_VERSION,
            ) => (
                Some(authority_id.as_str()),
                Some(actor_id.as_str()),
                change.as_ref(),
            ),
            (change, GOVERNANCE_VERSION) => (None, None, change),
            _ => {
                return Err(invalid(
                    "proposal authority format disagrees with governance version",
                ));
            }
        };
        let authority_matches = schema_version == GOVERNANCE_VERSION
            || self
                .authority_by_domain
                .get(proposal.domain.as_str())
                .is_some_and(|current| Some(current.as_str()) == declared_authority);
        let actor_authorized = if schema_version == GOVERNANCE_INSTITUTIONAL_VERSION {
            self.institutional_authority_roster
                .as_ref()
                .is_some_and(|roster| {
                    roster.appointments.iter().any(|appointment| {
                        Some(appointment.actor_id.as_str()) == actor_id
                            && Some(appointment.office_id.as_str()) == declared_authority
                            && appointment.start_month <= month
                            && month < appointment.end_month
                    })
                })
        } else {
            true
        };
        let (barrier, law_enabled) = match change {
            GovernanceChange::AuthorityDecision { .. } | GovernanceChange::ActorDecision { .. } => {
                return Err(invalid("nested authority decision is not permitted"));
            }
            GovernanceChange::EconomicPolicy { law_id, .. } => {
                let law = self
                    .laws
                    .iter()
                    .find(|law| law.id == *law_id)
                    .ok_or_else(|| invalid("proposal law does not exist"))?;
                (law.resistance, law.enabled)
            }
            GovernanceChange::AmendLaw { law_id, .. } => {
                let law = self
                    .laws
                    .iter()
                    .find(|law| law.id == *law_id)
                    .ok_or_else(|| invalid("amended law does not exist"))?;
                (
                    law.resistance + (1.0 - law.resistance) * gate.amendment_surcharge,
                    true,
                )
            }
            GovernanceChange::AmendGate { .. }
            | GovernanceChange::ChangeInfluence { .. }
            | GovernanceChange::TransferAuthority { .. } => (gate.amendment_surcharge, true),
        };
        let required = gate.passage_threshold + (1.0 - gate.passage_threshold) * barrier;
        let total_weight: f64 = self.groups.iter().map(|group| group.weight).sum();
        if !total_weight.is_finite() {
            return Err(invalid("political influence weights have no finite total"));
        }
        let stances: BTreeMap<&str, f64> = proposal
            .stances
            .iter()
            .map(|stance| (stance.group_id.as_str(), stance.value))
            .collect();
        let support = if total_weight == 0.0 {
            0.5
        } else {
            self.groups
                .iter()
                .map(|group| {
                    group.weight * (1.0 + stances.get(group.id.as_str()).copied().unwrap_or(0.0))
                        / 2.0
                })
                .sum::<f64>()
                / total_weight
        };
        let transfer_pending = matches!(change, GovernanceChange::TransferAuthority { .. })
            && self.pending.iter().any(|pending| {
                matches!(
                    substantive_change(&pending.change),
                    GovernanceChange::TransferAuthority { gate_id, .. } if gate_id == &gate.id
                )
            });
        let accepted = authority_matches
            && actor_authorized
            && !transfer_pending
            && law_enabled
            && support >= required;
        let effective_month = if accepted {
            Some(
                month
                    .checked_add(gate.review_delay_months)
                    .and_then(|date| date.checked_add(1))
                    .ok_or_else(|| invalid("governance effective month overflow"))?,
            )
        } else {
            None
        };
        if let Some(effective_month) = effective_month {
            self.pending.push(PendingChange {
                proposal_id: proposal.id.clone(),
                effective_month,
                domain: (schema_version == GOVERNANCE_INSTITUTIONAL_VERSION)
                    .then(|| proposal.domain.clone()),
                change: proposal.change.clone(),
            });
        }
        self.events.push(GovernanceEvent {
            month,
            proposal_id: proposal.id.clone(),
            domain: (schema_version == GOVERNANCE_INSTITUTIONAL_VERSION)
                .then(|| proposal.domain.clone()),
            actor_id: actor_id.map(str::to_owned),
            authority_id: if schema_version == GOVERNANCE_INSTITUTIONAL_VERSION {
                declared_authority.map(str::to_owned)
            } else {
                None
            },
            phase: "decision".to_owned(),
            outcome: if accepted {
                "approved"
            } else if !authority_matches {
                "unauthorized_authority"
            } else if !actor_authorized {
                "unauthorized_actor"
            } else if transfer_pending {
                "authority_transfer_pending"
            } else if !law_enabled {
                "law_disabled"
            } else {
                "rejected"
            }
            .to_owned(),
            support: Some(support),
            required: Some(required),
            effective_month,
        });
        Ok(())
    }

    fn apply(&mut self, pending: &PendingChange) -> OwgResult<()> {
        match substantive_change(&pending.change) {
            GovernanceChange::AuthorityDecision { .. } | GovernanceChange::ActorDecision { .. } => {
                return Err(invalid("nested authority decision is not permitted"));
            }
            GovernanceChange::EconomicPolicy {
                duration_months,
                action,
                ..
            } => {
                let end_month = pending
                    .effective_month
                    .checked_add(*duration_months)
                    .ok_or_else(|| invalid("governed policy end month overflow"))?;
                self.policies.push(EnactedPolicy {
                    proposal_id: pending.proposal_id.clone(),
                    start_month: pending.effective_month,
                    end_month,
                    action: action.clone(),
                });
            }
            GovernanceChange::AmendLaw {
                law_id,
                enabled,
                resistance,
            } => {
                let law = self
                    .laws
                    .iter_mut()
                    .find(|law| law.id == *law_id)
                    .ok_or_else(|| invalid("amended law disappeared"))?;
                law.enabled = *enabled;
                law.resistance = *resistance;
            }
            GovernanceChange::AmendGate {
                gate_id,
                passage_threshold,
                amendment_surcharge,
                review_delay_months,
            } => {
                let gate = self
                    .gates
                    .iter_mut()
                    .find(|gate| gate.id == *gate_id)
                    .ok_or_else(|| invalid("amended gate disappeared"))?;
                gate.passage_threshold = *passage_threshold;
                gate.amendment_surcharge = *amendment_surcharge;
                gate.review_delay_months = *review_delay_months;
            }
            GovernanceChange::ChangeInfluence { group_id, weight } => {
                let group = self
                    .groups
                    .iter_mut()
                    .find(|group| group.id == *group_id)
                    .ok_or_else(|| invalid("influence group disappeared"))?;
                group.weight = *weight;
            }
            GovernanceChange::TransferAuthority {
                gate_id,
                new_authority_id,
            } => {
                let domain = self
                    .gates
                    .iter()
                    .find(|gate| gate.id == *gate_id)
                    .ok_or_else(|| invalid("authority transfer gate disappeared"))?
                    .domain
                    .clone();
                let authority = self
                    .authority_by_domain
                    .get_mut(&domain)
                    .ok_or_else(|| invalid("authority transfer domain disappeared"))?;
                authority.clone_from(new_authority_id);
            }
        }
        Ok(())
    }

    /// Returns the V2 office currently competent to decide this domain.
    /// V1 has no separate authority concept and therefore returns `None`.
    pub fn decision_authority(&self, domain: &str) -> Option<&str> {
        self.authority_by_domain.get(domain).map(String::as_str)
    }

    /// The law in force for an action at the *start* of the next simulation
    /// month. Accepted amendments due this month are considered, in the same
    /// deterministic order as `advance`, without mutating the governance
    /// state. Proposals submitted this month cannot take effect yet.
    ///
    /// This is a scenario rule, not a finding about real-world legality.
    pub fn law_enabled_for_dispatch(
        &self,
        law_id: &str,
        domain: &str,
        month: u32,
    ) -> OwgResult<bool> {
        if self.next_month != month || law_id.trim().is_empty() || domain.trim().is_empty() {
            return Err(invalid("law dispatch query has invalid identity or month"));
        }
        let mut laws = self.laws.iter().filter(|law| law.id == law_id);
        let law = laws
            .next()
            .ok_or_else(|| invalid("dispatch law is absent from governance state"))?;
        if laws.next().is_some() {
            return Err(invalid("duplicate dispatch law in governance state"));
        }
        if law.domain != domain {
            return Err(invalid("dispatch law is absent from the declared domain"));
        }
        let mut enabled = law.enabled;
        let mut due: Vec<&PendingChange> = self
            .pending
            .iter()
            .filter(|pending| pending.effective_month <= month)
            .collect();
        if due.iter().any(|pending| pending.effective_month < month) {
            return Err(invalid("governance state contains an overdue law change"));
        }
        due.sort_by(|left, right| {
            left.effective_month
                .cmp(&right.effective_month)
                .then(left.proposal_id.cmp(&right.proposal_id))
        });
        for pending in due {
            if let GovernanceChange::AmendLaw {
                law_id: amended_id,
                enabled: amended_enabled,
                ..
            } = substantive_change(&pending.change)
                && amended_id == law_id
            {
                enabled = *amended_enabled;
            }
        }
        Ok(enabled)
    }

    /// Identify the governance contract represented by this state. This is a
    /// consistency aid for a caller-supplied action-law binding, not a proof
    /// that the state was replayed from a trusted scenario.
    #[must_use]
    pub fn dispatch_schema_version(&self) -> &'static str {
        if self.institutional_authority_roster.is_some() {
            GOVERNANCE_INSTITUTIONAL_VERSION
        } else if !self.authority_by_domain.is_empty() {
            GOVERNANCE_AUTHORITY_VERSION
        } else {
            GOVERNANCE_VERSION
        }
    }

    pub fn active_policies(&self, month: u32) -> impl Iterator<Item = &EnactedPolicy> {
        self.policies
            .iter()
            .filter(move |policy| policy.start_month <= month && month < policy.end_month)
    }
}

fn substantive_change(change: &GovernanceChange) -> &GovernanceChange {
    match change {
        GovernanceChange::AuthorityDecision { change, .. }
        | GovernanceChange::ActorDecision { change, .. } => change.as_ref(),
        _ => change,
    }
}

fn valid_policy_action(action: &PolicyActionKind) -> bool {
    match action {
        PolicyActionKind::TargetedTransfer {
            amount_per_household,
            bottom_share,
        } => {
            amount_per_household.minor_units() >= 0
                && (0.0..=1.0).contains(bottom_share)
                && *bottom_share > 0.0
        }
        PolicyActionKind::PublicInvestment {
            monthly_amount,
            energy_intensity_reduction,
        } => monthly_amount.minor_units() >= 0 && (0.0..=0.25).contains(energy_intensity_reduction),
        PolicyActionKind::PolicyRateDelta { .. } | PolicyActionKind::IndirectTaxDelta { .. } => {
            true
        }
    }
}

fn unit_interval(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn unit_symmetric(value: f64) -> bool {
    value.is_finite() && (-1.0..=1.0).contains(&value)
}

fn invalid(message: &str) -> OwgError {
    OwgError::InvalidState(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::institutions::{InstitutionalEntity, TaxClassification};

    fn specimen() -> GovernanceSpec {
        GovernanceSpec {
            schema_version: GOVERNANCE_VERSION.to_owned(),
            groups: vec![
                InfluenceGroup {
                    id: "companies".to_owned(),
                    label: "Companies".to_owned(),
                    weight: 9.0,
                },
                InfluenceGroup {
                    id: "unions".to_owned(),
                    label: "Unions".to_owned(),
                    weight: 1.0,
                },
            ],
            gates: vec![InstitutionalGate {
                id: "tax-gate".to_owned(),
                domain: "tax".to_owned(),
                passage_threshold: 0.7,
                amendment_surcharge: 0.1,
                review_delay_months: 0,
            }],
            laws: vec![LawRule {
                id: "tax-authority".to_owned(),
                domain: "tax".to_owned(),
                enabled: true,
                resistance: 0.0,
            }],
            proposals: vec![GovernanceProposal {
                id: "tax-policy".to_owned(),
                submitted_month: 0,
                domain: "tax".to_owned(),
                sponsor_id: "companies".to_owned(),
                stances: vec![
                    GroupStance {
                        group_id: "companies".to_owned(),
                        value: 1.0,
                    },
                    GroupStance {
                        group_id: "unions".to_owned(),
                        value: -1.0,
                    },
                ],
                change: GovernanceChange::EconomicPolicy {
                    law_id: "tax-authority".to_owned(),
                    duration_months: 2,
                    action: PolicyActionKind::IndirectTaxDelta {
                        sector: crate::contracts::Sector::Energy,
                        basis_points: 100,
                    },
                },
            }],
        }
    }

    #[test]
    fn influence_can_favor_either_companies_or_unions() {
        let spec = specimen();
        let mut state = GovernanceState::new(&spec, 4).unwrap();
        state.advance(&spec, 0).unwrap();
        assert_eq!(state.events[0].outcome, "approved");
        state.advance(&spec, 1).unwrap();
        assert_eq!(state.active_policies(1).count(), 1);

        let mut reversed = specimen();
        reversed.groups[0].weight = 1.0;
        reversed.groups[1].weight = 9.0;
        let mut reversed_state = GovernanceState::new(&reversed, 4).unwrap();
        reversed_state.advance(&reversed, 0).unwrap();
        assert_eq!(reversed_state.events[0].outcome, "rejected");
    }

    #[test]
    fn law_and_safeguard_changes_affect_later_proposals_only() {
        let mut spec = specimen();
        spec.groups[0].weight = 1.0;
        spec.groups[1].weight = 1.0;
        spec.gates[0].passage_threshold = 0.8;
        spec.laws[0].enabled = false;
        spec.proposals[0].id = "a-too-early".to_owned();
        spec.proposals.push(GovernanceProposal {
            id: "b-amend-law".to_owned(),
            submitted_month: 0,
            domain: "tax".to_owned(),
            sponsor_id: "unions".to_owned(),
            stances: vec![
                GroupStance {
                    group_id: "companies".to_owned(),
                    value: 1.0,
                },
                GroupStance {
                    group_id: "unions".to_owned(),
                    value: 1.0,
                },
            ],
            change: GovernanceChange::AmendLaw {
                law_id: "tax-authority".to_owned(),
                enabled: true,
                resistance: 0.0,
            },
        });
        spec.proposals.push(GovernanceProposal {
            id: "c-lower-gate".to_owned(),
            submitted_month: 0,
            domain: "tax".to_owned(),
            sponsor_id: "unions".to_owned(),
            stances: vec![
                GroupStance {
                    group_id: "companies".to_owned(),
                    value: 1.0,
                },
                GroupStance {
                    group_id: "unions".to_owned(),
                    value: 1.0,
                },
            ],
            change: GovernanceChange::AmendGate {
                gate_id: "tax-gate".to_owned(),
                passage_threshold: 0.4,
                amendment_surcharge: 0.0,
                review_delay_months: 0,
            },
        });
        let mut later = spec.proposals[0].clone();
        later.id = "d-after-change".to_owned();
        later.submitted_month = 1;
        spec.proposals.push(later);
        let mut state = GovernanceState::new(&spec, 4).unwrap();
        state.advance(&spec, 0).unwrap();
        assert_eq!(state.events[0].outcome, "law_disabled");
        assert!(!state.laws[0].enabled);
        assert!((state.gates[0].passage_threshold - 0.8).abs() < 1e-12);
        state.advance(&spec, 1).unwrap();
        assert!(state.laws[0].enabled);
        assert!((state.gates[0].passage_threshold - 0.4).abs() < 1e-12);
        assert_eq!(state.events.last().unwrap().outcome, "approved");
        state.advance(&spec, 2).unwrap();
        assert_eq!(state.active_policies(2).count(), 1);
    }

    #[test]
    fn dispatch_law_query_rejects_duplicate_and_overdue_state() {
        let mut spec = specimen();
        spec.proposals[0].change = GovernanceChange::AmendLaw {
            law_id: "tax-authority".to_owned(),
            enabled: false,
            resistance: 0.0,
        };
        let mut state = GovernanceState::new(&spec, 4).unwrap();
        assert!(
            state
                .law_enabled_for_dispatch("tax-authority", "tax", 0)
                .unwrap()
        );
        state.advance(&spec, 0).unwrap();
        assert!(
            !state
                .law_enabled_for_dispatch("tax-authority", "tax", 1)
                .unwrap()
        );
        let mut duplicate = state.clone();
        duplicate.laws.push(LawRule {
            id: "tax-authority".to_owned(),
            domain: "different-domain".to_owned(),
            enabled: true,
            resistance: 0.0,
        });
        assert!(
            duplicate
                .law_enabled_for_dispatch("tax-authority", "tax", 1)
                .is_err()
        );
        state.next_month = 2;
        assert!(
            state
                .law_enabled_for_dispatch("tax-authority", "tax", 2)
                .is_err()
        );
    }

    #[test]
    fn invalid_weights_and_duplicate_stances_are_rejected() {
        let mut spec = specimen();
        spec.groups[0].weight = f64::NAN;
        assert!(spec.validate(4).is_err());
        spec = specimen();
        let duplicate = spec.proposals[0].stances[0].clone();
        spec.proposals[0].stances.push(duplicate);
        assert!(spec.validate(4).is_err());
    }

    fn authority_spec() -> GovernanceSpec {
        let mut spec = specimen();
        spec.schema_version = GOVERNANCE_AUTHORITY_VERSION.to_owned();
        spec.gates.push(InstitutionalGate {
            id: "labor-gate".to_owned(),
            domain: "labor".to_owned(),
            passage_threshold: 0.7,
            amendment_surcharge: 0.1,
            review_delay_months: 0,
        });
        let economic_change = spec.proposals[0].change.clone();
        spec.proposals[0].change = GovernanceChange::AuthorityDecision {
            authority_id: "tax-gate".to_owned(),
            change: Box::new(economic_change),
        };
        spec
    }

    fn institutional_spec_and_graph() -> (GovernanceSpec, InstitutionGraph) {
        let mut spec = specimen();
        spec.schema_version = GOVERNANCE_INSTITUTIONAL_VERSION.to_owned();
        let action = spec.proposals[0].change.clone();
        spec.proposals[0].change = GovernanceChange::ActorDecision {
            actor_id: "HH-0".to_owned(),
            authority_id: "tax-gate".to_owned(),
            change: Box::new(action),
        };
        spec.proposals[0].stances[1].value = 1.0;
        let mut graph = InstitutionGraph::synthetic_firm_ownership(
            &["HH-0".to_owned(), "HH-1".to_owned()],
            &["FIRM-0".to_owned()],
        )
        .unwrap();
        graph.schema_version = INSTITUTION_GRAPH_AUTHORITY_VERSION.to_owned();
        graph.entities[0].jurisdiction = "US".to_owned();
        graph.entities[1].jurisdiction = "US".to_owned();
        graph.entities.push(InstitutionalEntity {
            id: "tax-gate".to_owned(),
            legal_form: LegalForm::GovernmentBody,
            tax_classification: TaxClassification::Exempt,
            economic_role: EconomicRole::PublicAuthority,
            jurisdiction: "US".to_owned(),
            formed_month: 0,
        });
        graph.authority_appointments.push(AuthorityAppointment {
            actor_id: "HH-0".to_owned(),
            office_id: "tax-gate".to_owned(),
            start_month: 0,
            end_month: 2,
        });
        (spec, graph)
    }

    #[test]
    fn v3_requires_graph_bound_actor_appointment_and_rejects_impersonation() {
        let (mut spec, graph) = institutional_spec_and_graph();
        assert!(GovernanceState::new(&spec, 4).is_err());
        let mut impersonation = spec.proposals[0].clone();
        impersonation.id = "b-impersonation".to_owned();
        if let GovernanceChange::ActorDecision { actor_id, .. } = &mut impersonation.change {
            *actor_id = "HH-1".to_owned();
        }
        let mut expired = spec.proposals[0].clone();
        expired.id = "c-expired".to_owned();
        expired.submitted_month = 2;
        spec.proposals.push(impersonation);
        spec.proposals.push(expired);
        let mut mismatched =
            GovernanceState::new_with_institutions(&spec, 4, &graph, "US").unwrap();
        assert!(mismatched.advance(&specimen(), 0).is_err());
        let mut state = GovernanceState::new_with_institutions(&spec, 4, &graph, "US").unwrap();
        state.advance(&spec, 0).unwrap();
        assert_eq!(state.events[0].outcome, "unauthorized_actor");
        assert_eq!(state.events[1].outcome, "approved");
        assert_eq!(state.events[0].support, Some(1.0));
        assert_eq!(state.events[0].domain.as_deref(), Some("tax"));
        assert_eq!(state.events[0].actor_id.as_deref(), Some("HH-1"));
        assert_eq!(state.events[0].authority_id.as_deref(), Some("tax-gate"));
        assert_eq!(state.events[1].actor_id.as_deref(), Some("HH-0"));
        state.advance(&spec, 1).unwrap();
        assert_eq!(state.active_policies(1).count(), 1);
        let effective = state
            .events
            .iter()
            .find(|event| event.phase == "effective")
            .unwrap();
        assert_eq!(effective.domain.as_deref(), Some("tax"));
        assert_eq!(effective.actor_id.as_deref(), Some("HH-0"));
        assert_eq!(effective.authority_id.as_deref(), Some("tax-gate"));
        state.advance(&spec, 2).unwrap();
        assert_eq!(state.events.last().unwrap().outcome, "unauthorized_actor");
        let restored: GovernanceState =
            serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        assert_eq!(restored, state);
    }

    #[test]
    fn v3_rejects_unknown_office_actor_and_cross_jurisdiction_office() {
        let (mut spec, mut graph) = institutional_spec_and_graph();
        assert!(spec.validate_with_institutions(4, &graph, "CA").is_err());
        graph.entities.last_mut().unwrap().jurisdiction = "CA".to_owned();
        assert!(spec.validate_with_institutions(4, &graph, "US").is_err());
        let (_, graph) = institutional_spec_and_graph();
        if let GovernanceChange::ActorDecision { actor_id, .. } = &mut spec.proposals[0].change {
            *actor_id = "HH-GHOST".to_owned();
        }
        assert!(spec.validate_with_institutions(4, &graph, "US").is_err());
        if let GovernanceChange::ActorDecision {
            actor_id,
            authority_id,
            ..
        } = &mut spec.proposals[0].change
        {
            *actor_id = "HH-0".to_owned();
            *authority_id = "ghost-office".to_owned();
        }
        assert!(spec.validate_with_institutions(4, &graph, "US").is_err());
    }

    #[test]
    fn v3_authority_transfer_changes_who_may_act_only_after_review() {
        let (mut spec, mut graph) = institutional_spec_and_graph();
        spec.gates[0].review_delay_months = 1;
        spec.gates.push(InstitutionalGate {
            id: "labor-office".to_owned(),
            domain: "labor".to_owned(),
            passage_threshold: 0.5,
            amendment_surcharge: 0.0,
            review_delay_months: 0,
        });
        graph.entities.push(InstitutionalEntity {
            id: "labor-office".to_owned(),
            legal_form: LegalForm::GovernmentBody,
            tax_classification: TaxClassification::Exempt,
            economic_role: EconomicRole::PublicAuthority,
            jurisdiction: "US".to_owned(),
            formed_month: 0,
        });
        graph.authority_appointments.push(AuthorityAppointment {
            actor_id: "HH-1".to_owned(),
            office_id: "labor-office".to_owned(),
            start_month: 0,
            end_month: 5,
        });
        let mut transfer = spec.proposals[0].clone();
        transfer.id = "a-transfer".to_owned();
        transfer.change = GovernanceChange::ActorDecision {
            actor_id: "HH-0".to_owned(),
            authority_id: "tax-gate".to_owned(),
            change: Box::new(GovernanceChange::TransferAuthority {
                gate_id: "tax-gate".to_owned(),
                new_authority_id: "labor-office".to_owned(),
            }),
        };
        let mut early = spec.proposals[0].clone();
        early.id = "b-too-early".to_owned();
        early.submitted_month = 1;
        if let GovernanceChange::ActorDecision {
            actor_id,
            authority_id,
            ..
        } = &mut early.change
        {
            *actor_id = "HH-1".to_owned();
            *authority_id = "labor-office".to_owned();
        }
        let mut later = early.clone();
        later.id = "c-after-review".to_owned();
        later.submitted_month = 2;
        spec.proposals = vec![transfer, early, later];
        let mut state = GovernanceState::new_with_institutions(&spec, 5, &graph, "US").unwrap();
        state.advance(&spec, 0).unwrap();
        assert_eq!(state.events[0].outcome, "approved");
        assert_eq!(state.events[0].effective_month, Some(2));
        state.advance(&spec, 1).unwrap();
        assert_eq!(state.events[1].outcome, "unauthorized_authority");
        state.advance(&spec, 2).unwrap();
        assert_eq!(state.decision_authority("tax"), Some("labor-office"));
        assert_eq!(state.events[2].phase, "effective");
        assert_eq!(state.events[3].outcome, "approved");
    }

    #[test]
    fn declared_authority_and_sponsor_are_independent_of_influence() {
        let mut spec = authority_spec();
        let mut state = GovernanceState::new(&spec, 3).unwrap();
        assert_eq!(state.decision_authority("tax"), Some("tax-gate"));
        state.advance(&spec, 0).unwrap();
        assert_eq!(state.events[0].outcome, "approved");

        // Influence can be reversed without changing who has legal authority.
        spec.groups[0].weight = 1.0;
        spec.groups[1].weight = 9.0;
        let mut reversed = GovernanceState::new(&spec, 3).unwrap();
        reversed.advance(&spec, 0).unwrap();
        assert_eq!(reversed.events[0].outcome, "rejected");
        assert_eq!(reversed.decision_authority("tax"), Some("tax-gate"));

        // Unanimous support cannot make a different office competent.
        spec.proposals[0].stances[1].value = 1.0;
        if let GovernanceChange::AuthorityDecision { authority_id, .. } =
            &mut spec.proposals[0].change
        {
            *authority_id = "labor-gate".to_owned();
        }
        let mut unauthorized = GovernanceState::new(&spec, 3).unwrap();
        unauthorized.advance(&spec, 0).unwrap();
        assert_eq!(unauthorized.events[0].outcome, "unauthorized_authority");
        assert_eq!(unauthorized.events[0].support, Some(1.0));
        unauthorized.advance(&spec, 1).unwrap();
        assert_eq!(unauthorized.active_policies(1).count(), 0);
    }

    #[test]
    fn authority_transfer_uses_old_gate_and_only_future_decisions_use_new_office() {
        let mut spec = authority_spec();
        spec.gates[0].review_delay_months = 1;
        let mut transfer = spec.proposals[0].clone();
        transfer.id = "a-transfer".to_owned();
        transfer.stances[1].value = 1.0;
        transfer.change = GovernanceChange::AuthorityDecision {
            authority_id: "tax-gate".to_owned(),
            change: Box::new(GovernanceChange::TransferAuthority {
                gate_id: "tax-gate".to_owned(),
                new_authority_id: "labor-gate".to_owned(),
            }),
        };
        let mut early = spec.proposals[0].clone();
        early.id = "b-new-office-too-early".to_owned();
        early.submitted_month = 1;
        early.stances[1].value = 1.0;
        if let GovernanceChange::AuthorityDecision { authority_id, .. } = &mut early.change {
            *authority_id = "labor-gate".to_owned();
        }
        let mut old_after = spec.proposals[0].clone();
        old_after.id = "c-old-office-after".to_owned();
        old_after.submitted_month = 2;
        old_after.stances[1].value = 1.0;
        let mut new_after = early.clone();
        new_after.id = "d-new-office-after".to_owned();
        new_after.submitted_month = 2;
        spec.proposals = vec![transfer, early, old_after, new_after];

        let mut state = GovernanceState::new(&spec, 5).unwrap();
        state.advance(&spec, 0).unwrap();
        assert_eq!(state.events[0].outcome, "approved");
        assert_eq!(state.events[0].effective_month, Some(2));
        assert_eq!(state.decision_authority("tax"), Some("tax-gate"));
        state.advance(&spec, 1).unwrap();
        assert_eq!(
            state.events.last().unwrap().outcome,
            "unauthorized_authority"
        );
        assert_eq!(state.decision_authority("tax"), Some("tax-gate"));
        state.advance(&spec, 2).unwrap();
        assert_eq!(state.decision_authority("tax"), Some("labor-gate"));
        assert_eq!(state.events[2].phase, "effective");
        assert_eq!(state.events[3].outcome, "unauthorized_authority");
        assert_eq!(state.events[4].outcome, "approved");
        assert_eq!(state.events[4].effective_month, Some(4));
        let restored: GovernanceState =
            serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        assert_eq!(restored, state);
    }

    #[test]
    fn authority_transfer_cannot_bypass_old_office_or_old_threshold() {
        let mut spec = authority_spec();
        spec.proposals[0].stances[1].value = 1.0;
        spec.proposals[0].change = GovernanceChange::AuthorityDecision {
            authority_id: "labor-gate".to_owned(),
            change: Box::new(GovernanceChange::TransferAuthority {
                gate_id: "tax-gate".to_owned(),
                new_authority_id: "labor-gate".to_owned(),
            }),
        };
        let mut unauthorized = GovernanceState::new(&spec, 3).unwrap();
        unauthorized.advance(&spec, 0).unwrap();
        assert_eq!(unauthorized.events[0].outcome, "unauthorized_authority");
        unauthorized.advance(&spec, 1).unwrap();
        assert_eq!(unauthorized.decision_authority("tax"), Some("tax-gate"));

        if let GovernanceChange::AuthorityDecision { authority_id, .. } =
            &mut spec.proposals[0].change
        {
            *authority_id = "tax-gate".to_owned();
        }
        spec.groups[0].weight = 1.0;
        spec.groups[1].weight = 1.0;
        spec.proposals[0].stances[1].value = -1.0;
        let mut under_threshold = GovernanceState::new(&spec, 3).unwrap();
        under_threshold.advance(&spec, 0).unwrap();
        assert_eq!(under_threshold.events[0].outcome, "rejected");
        assert_eq!(under_threshold.events[0].support, Some(0.5));
        under_threshold.advance(&spec, 1).unwrap();
        assert_eq!(under_threshold.decision_authority("tax"), Some("tax-gate"));
    }

    #[test]
    fn v2_rejects_missing_unknown_or_nested_authority_and_v1_shape_stays_legacy() {
        let mut spec = authority_spec();
        let wrapped = spec.proposals[0].change.clone();
        let GovernanceChange::AuthorityDecision { change, .. } = wrapped else {
            unreachable!()
        };
        spec.proposals[0].change = *change;
        assert!(spec.validate(3).is_err());
        spec.proposals[0].change = GovernanceChange::AuthorityDecision {
            authority_id: "ghost-office".to_owned(),
            change: Box::new(spec.proposals[0].change.clone()),
        };
        assert!(spec.validate(3).is_err());
        spec.proposals[0].change = GovernanceChange::AuthorityDecision {
            authority_id: "tax-gate".to_owned(),
            change: Box::new(spec.proposals[0].change.clone()),
        };
        assert!(spec.validate(3).is_err());

        let legacy = GovernanceState::new(&specimen(), 3).unwrap();
        assert_eq!(legacy.decision_authority("tax"), None);
        assert!(
            serde_json::to_value(&legacy)
                .unwrap()
                .get("authority_by_domain")
                .is_none()
        );
        let restored: GovernanceState =
            serde_json::from_value(serde_json::to_value(&legacy).unwrap()).unwrap();
        assert_eq!(restored, legacy);
        let mut legacy_after_decision = GovernanceState::new(&specimen(), 3).unwrap();
        legacy_after_decision.advance(&specimen(), 0).unwrap();
        let legacy_event = serde_json::to_value(&legacy_after_decision.events[0]).unwrap();
        assert!(legacy_event.get("domain").is_none());
        assert!(legacy_event.get("actor_id").is_none());
        assert!(legacy_event.get("authority_id").is_none());
        let authority_spec = authority_spec();
        let mut authority_state = GovernanceState::new(&authority_spec, 3).unwrap();
        authority_state.advance(&authority_spec, 0).unwrap();
        let authority_event = serde_json::to_value(&authority_state.events[0]).unwrap();
        assert!(authority_event.get("domain").is_none());
        assert!(authority_event.get("actor_id").is_none());
        assert!(authority_event.get("authority_id").is_none());
    }
}
