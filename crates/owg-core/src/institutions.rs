//! Legal identity and beneficial ownership, separate from tax treatment.
//!
//! These records describe claims and control. They do not by themselves move
//! cash, create taxable income, or establish legal compliance.

use crate::error::{OwgError, OwgResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const INSTITUTION_GRAPH_VERSION: &str = "owg.institutions.v1";
/// Opt-in public-office and appointment records. V1 retains its serialized
/// shape and ownership rules.
pub const INSTITUTION_GRAPH_AUTHORITY_VERSION: &str = "owg.institutions.v2";

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LegalForm {
    NaturalPerson,
    Corporation,
    LimitedLiabilityCompany,
    Partnership,
    Trust,
    Cooperative,
    GovernmentBody,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaxClassification {
    Unknown,
    Individual,
    Corporation,
    PassThrough,
    Disregarded,
    Exempt,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EconomicRole {
    Household,
    OperatingFirm,
    HoldingCompany,
    Conduit,
    PublicAuthority,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct InstitutionalEntity {
    pub id: String,
    pub legal_form: LegalForm,
    pub tax_classification: TaxClassification,
    pub economic_role: EconomicRole,
    /// A jurisdiction identifier, not a claim that one national law applies.
    pub jurisdiction: String,
    pub formed_month: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct OwnershipStake {
    pub owner_id: String,
    pub owned_id: String,
    /// A 100% direct interest is 10,000 basis points.
    pub basis_points: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct AuthorityAppointment {
    /// A natural-person household actor bound to the engine population.
    pub actor_id: String,
    /// A public-authority entity in the same jurisdiction as the actor.
    pub office_id: String,
    /// Inclusive start and exclusive end of this declared appointment.
    pub start_month: u32,
    pub end_month: u32,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct InstitutionGraph {
    pub schema_version: String,
    pub entities: Vec<InstitutionalEntity>,
    pub stakes: Vec<OwnershipStake>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authority_appointments: Vec<AuthorityAppointment>,
}

impl InstitutionGraph {
    pub fn synthetic_firm_ownership(
        household_ids: &[String],
        firm_ids: &[String],
    ) -> OwgResult<Self> {
        if household_ids.is_empty() || firm_ids.is_empty() {
            return Err(OwgError::InvalidState(
                "ownership graph requires households and firms".to_owned(),
            ));
        }
        let mut entities = Vec::with_capacity(household_ids.len() + firm_ids.len());
        entities.extend(household_ids.iter().map(|id| InstitutionalEntity {
            id: id.clone(),
            legal_form: LegalForm::NaturalPerson,
            tax_classification: TaxClassification::Individual,
            economic_role: EconomicRole::Household,
            jurisdiction: "US-UNSPECIFIED".to_owned(),
            formed_month: 0,
        }));
        entities.extend(firm_ids.iter().map(|id| InstitutionalEntity {
            id: id.clone(),
            legal_form: LegalForm::Corporation,
            tax_classification: TaxClassification::Unknown,
            economic_role: EconomicRole::OperatingFirm,
            jurisdiction: "US-UNSPECIFIED".to_owned(),
            formed_month: 0,
        }));
        let stakes = firm_ids
            .iter()
            .enumerate()
            .map(|(index, firm)| OwnershipStake {
                owner_id: household_ids[index % household_ids.len()].clone(),
                owned_id: firm.clone(),
                basis_points: 10_000,
            })
            .collect();
        let graph = Self {
            schema_version: INSTITUTION_GRAPH_VERSION.to_owned(),
            entities,
            stakes,
            authority_appointments: Vec::new(),
        };
        graph.validate()?;
        Ok(graph)
    }

    pub fn validate(&self) -> OwgResult<()> {
        let authority_aware = self.schema_version == INSTITUTION_GRAPH_AUTHORITY_VERSION;
        if (!authority_aware && self.schema_version != INSTITUTION_GRAPH_VERSION)
            || self.entities.is_empty()
            || (!authority_aware && !self.authority_appointments.is_empty())
        {
            return Err(OwgError::InvalidState(
                "unsupported or empty institution graph".to_owned(),
            ));
        }
        let mut entities = BTreeMap::new();
        for entity in &self.entities {
            if entity.id.trim().is_empty()
                || entity.jurisdiction.trim().is_empty()
                || entities.insert(entity.id.as_str(), entity).is_some()
                || (entity.legal_form == LegalForm::NaturalPerson
                    && (entity.economic_role != EconomicRole::Household
                        || entity.tax_classification != TaxClassification::Individual))
                || (entity.legal_form != LegalForm::NaturalPerson
                    && entity.economic_role == EconomicRole::Household)
                || (entity.legal_form == LegalForm::GovernmentBody
                    && (!authority_aware || entity.economic_role != EconomicRole::PublicAuthority))
                || (entity.economic_role == EconomicRole::PublicAuthority
                    && (!authority_aware
                        || entity.legal_form != LegalForm::GovernmentBody
                        || entity.tax_classification != TaxClassification::Exempt))
            {
                return Err(OwgError::InvalidState(format!(
                    "invalid or duplicate legal entity {}",
                    entity.id
                )));
            }
        }
        let mut sums: BTreeMap<&str, u32> = BTreeMap::new();
        let mut by_owned: BTreeMap<&str, Vec<&OwnershipStake>> = BTreeMap::new();
        let mut pairs = BTreeSet::new();
        for stake in &self.stakes {
            let owner_entity = entities.get(stake.owner_id.as_str());
            let target_entity = entities.get(stake.owned_id.as_str());
            if owner_entity.is_none()
                || target_entity.is_none()
                || target_entity.is_some_and(|entity| entity.legal_form == LegalForm::NaturalPerson)
                || target_entity
                    .is_some_and(|entity| entity.economic_role == EconomicRole::PublicAuthority)
                || owner_entity
                    .is_some_and(|entity| entity.economic_role == EconomicRole::PublicAuthority)
                || stake.owner_id == stake.owned_id
                || stake.basis_points == 0
                || stake.basis_points > 10_000
                || !pairs.insert((stake.owner_id.as_str(), stake.owned_id.as_str()))
            {
                return Err(OwgError::InvalidState(format!(
                    "invalid ownership stake {} -> {}",
                    stake.owner_id, stake.owned_id
                )));
            }
            *sums.entry(stake.owned_id.as_str()).or_default() += u32::from(stake.basis_points);
            by_owned
                .entry(stake.owned_id.as_str())
                .or_default()
                .push(stake);
        }
        for entity in &self.entities {
            let sum = sums.get(entity.id.as_str()).copied().unwrap_or(0);
            if (entity.legal_form == LegalForm::NaturalPerson
                || entity.economic_role == EconomicRole::PublicAuthority)
                && sum != 0
                || (entity.legal_form != LegalForm::NaturalPerson
                    && entity.economic_role != EconomicRole::PublicAuthority)
                    && sum != 10_000
            {
                return Err(OwgError::InvalidState(format!(
                    "ownership of {} totals {sum} basis points, expected {}",
                    entity.id,
                    if entity.legal_form == LegalForm::NaturalPerson
                        || entity.economic_role == EconomicRole::PublicAuthority
                    {
                        0
                    } else {
                        10_000
                    }
                )));
            }
        }
        // Acyclicity is a v1 restriction, not a claim that real companies never
        // cross-own each other. Later versions need a circular-ownership solver.
        let mut states = BTreeMap::new();
        for entity in &self.entities {
            Self::visit_ownership(&entity.id, &by_owned, &mut states)?;
        }
        let mut appointments = BTreeSet::new();
        for appointment in &self.authority_appointments {
            let actor = entities.get(appointment.actor_id.as_str());
            let office = entities.get(appointment.office_id.as_str());
            if appointment.start_month >= appointment.end_month
                || actor.is_none_or(|actor| actor.legal_form != LegalForm::NaturalPerson)
                || office.is_none_or(|office| office.economic_role != EconomicRole::PublicAuthority)
                || actor
                    .zip(office)
                    .is_none_or(|(actor, office)| actor.jurisdiction != office.jurisdiction)
                || actor.zip(office).is_none_or(|(actor, office)| {
                    actor.formed_month > appointment.start_month
                        || office.formed_month > appointment.start_month
                })
                || !appointments.insert((
                    appointment.actor_id.as_str(),
                    appointment.office_id.as_str(),
                    appointment.start_month,
                    appointment.end_month,
                ))
            {
                return Err(OwgError::InvalidState(format!(
                    "invalid public-office appointment {} -> {}",
                    appointment.actor_id, appointment.office_id
                )));
            }
        }
        Ok(())
    }

    /// Bind a proposed graph to the engine's actual agent IDs. Extra nodes
    /// may only be holding or conduit entities; they have no independent
    /// balance sheet or decision process in the present engine.
    pub fn validate_population(
        &self,
        household_ids: &[String],
        firm_ids: &[String],
    ) -> OwgResult<()> {
        self.validate()?;
        let households: BTreeSet<&str> = household_ids.iter().map(String::as_str).collect();
        let firms: BTreeSet<&str> = firm_ids.iter().map(String::as_str).collect();
        if households.len() != household_ids.len() || firms.len() != firm_ids.len() {
            return Err(OwgError::InvalidState(
                "duplicate household or firm agent ID".to_owned(),
            ));
        }
        let mut seen_households = BTreeSet::new();
        let mut seen_firms = BTreeSet::new();
        for entity in &self.entities {
            if entity.formed_month != 0 {
                return Err(OwgError::InvalidState(format!(
                    "initial institution {} is not formed at month zero",
                    entity.id
                )));
            }
            match entity.economic_role {
                EconomicRole::Household => {
                    if !households.contains(entity.id.as_str()) {
                        return Err(OwgError::InvalidState(format!(
                            "unbound household institution {}",
                            entity.id
                        )));
                    }
                    seen_households.insert(entity.id.as_str());
                }
                EconomicRole::OperatingFirm => {
                    if !firms.contains(entity.id.as_str()) {
                        return Err(OwgError::InvalidState(format!(
                            "unbound operating-firm institution {}",
                            entity.id
                        )));
                    }
                    seen_firms.insert(entity.id.as_str());
                }
                EconomicRole::HoldingCompany
                | EconomicRole::Conduit
                | EconomicRole::PublicAuthority => {}
            }
        }
        if seen_households != households || seen_firms != firms {
            return Err(OwgError::InvalidState(
                "institution graph omits household or firm agents".to_owned(),
            ));
        }
        Ok(())
    }

    fn visit_ownership<'a>(
        entity_id: &'a str,
        by_owned: &BTreeMap<&'a str, Vec<&'a OwnershipStake>>,
        states: &mut BTreeMap<&'a str, u8>,
    ) -> OwgResult<()> {
        match states.get(entity_id) {
            Some(2) => return Ok(()),
            Some(1) => {
                return Err(OwgError::InvalidState(format!(
                    "circular ownership involving {entity_id} is unsupported in v1"
                )));
            }
            _ => {}
        }
        states.insert(entity_id, 1);
        if let Some(stakes) = by_owned.get(entity_id) {
            for stake in stakes {
                Self::visit_ownership(&stake.owner_id, by_owned, states)?;
            }
        }
        states.insert(entity_id, 2);
        Ok(())
    }

    pub fn ultimate_owners(&self, entity_id: &str) -> OwgResult<BTreeMap<String, f64>> {
        self.validate()?;
        let entities: BTreeMap<&str, &InstitutionalEntity> = self
            .entities
            .iter()
            .map(|entity| (entity.id.as_str(), entity))
            .collect();
        let mut by_owned: BTreeMap<&str, Vec<&OwnershipStake>> = BTreeMap::new();
        for stake in &self.stakes {
            by_owned
                .entry(stake.owned_id.as_str())
                .or_default()
                .push(stake);
        }
        Self::ultimate_owners_unchecked(entity_id, &entities, &by_owned)
    }

    fn ultimate_owners_unchecked(
        entity_id: &str,
        entities: &BTreeMap<&str, &InstitutionalEntity>,
        by_owned: &BTreeMap<&str, Vec<&OwnershipStake>>,
    ) -> OwgResult<BTreeMap<String, f64>> {
        let entity = entities
            .get(entity_id)
            .ok_or_else(|| OwgError::InvalidState(format!("unknown institution {entity_id}")))?;
        if entity.legal_form == LegalForm::NaturalPerson {
            return Ok(BTreeMap::from([(entity_id.to_owned(), 1.0)]));
        }
        let mut result = BTreeMap::new();
        if let Some(stakes) = by_owned.get(entity_id) {
            for stake in stakes {
                let owners = Self::ultimate_owners_unchecked(&stake.owner_id, entities, by_owned)?;
                for (owner, share) in owners {
                    *result.entry(owner).or_insert(0.0) +=
                        share * f64::from(stake.basis_points) / 10_000.0;
                }
            }
        }
        Ok(result)
    }

    /// Initial-endowment change only; post-start acquisitions require a trade
    /// and matched cash/securities ledger postings, which are not yet present.
    pub fn set_initial_ownership(
        &mut self,
        owned_id: &str,
        replacement: Vec<OwnershipStake>,
    ) -> OwgResult<()> {
        if replacement.iter().any(|stake| stake.owned_id != owned_id) {
            return Err(OwgError::InvalidState(
                "replacement stakes target another entity".to_owned(),
            ));
        }
        let previous = self.stakes.clone();
        self.stakes.retain(|stake| stake.owned_id != owned_id);
        self.stakes.extend(replacement);
        if let Err(error) = self.validate() {
            self.stakes = previous;
            return Err(error);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph() -> InstitutionGraph {
        InstitutionGraph::synthetic_firm_ownership(
            &["HH-0".to_owned(), "HH-1".to_owned()],
            &["FIRM-0".to_owned()],
        )
        .unwrap()
    }

    #[test]
    fn public_office_appointments_are_opt_in_and_do_not_create_ownership() {
        let mut graph = graph();
        let legacy = serde_json::to_value(&graph).unwrap();
        assert!(legacy.get("authority_appointments").is_none());
        graph.schema_version = INSTITUTION_GRAPH_AUTHORITY_VERSION.to_owned();
        graph.entities[0].jurisdiction = "US".to_owned();
        graph.entities.push(InstitutionalEntity {
            id: "OFFICE-TAX".to_owned(),
            legal_form: LegalForm::GovernmentBody,
            tax_classification: TaxClassification::Exempt,
            economic_role: EconomicRole::PublicAuthority,
            jurisdiction: "US".to_owned(),
            formed_month: 0,
        });
        graph.authority_appointments.push(AuthorityAppointment {
            actor_id: "HH-0".to_owned(),
            office_id: "OFFICE-TAX".to_owned(),
            start_month: 0,
            end_month: 3,
        });
        graph
            .validate_population(
                &["HH-0".to_owned(), "HH-1".to_owned()],
                &["FIRM-0".to_owned()],
            )
            .unwrap();
        let restored: InstitutionGraph =
            serde_json::from_value(serde_json::to_value(&graph).unwrap()).unwrap();
        assert_eq!(graph, restored);

        let mut bad = graph.clone();
        bad.entities[0].jurisdiction = "CA".to_owned();
        assert!(bad.validate().is_err());
        bad = graph.clone();
        bad.stakes.push(OwnershipStake {
            owner_id: "HH-0".to_owned(),
            owned_id: "OFFICE-TAX".to_owned(),
            basis_points: 10_000,
        });
        assert!(bad.validate().is_err());
        bad = graph.clone();
        bad.authority_appointments[0].end_month = 0;
        assert!(bad.validate().is_err());
        bad = graph;
        bad.schema_version = INSTITUTION_GRAPH_VERSION.to_owned();
        assert!(bad.validate().is_err());
    }

    #[test]
    fn validates_and_resolves_nested_holding_ownership() {
        let mut graph = graph();
        graph.entities.push(InstitutionalEntity {
            id: "HOLD-0".to_owned(),
            legal_form: LegalForm::LimitedLiabilityCompany,
            tax_classification: TaxClassification::Disregarded,
            economic_role: EconomicRole::HoldingCompany,
            jurisdiction: "US-DE".to_owned(),
            formed_month: 0,
        });
        graph.stakes.push(OwnershipStake {
            owner_id: "HH-1".to_owned(),
            owned_id: "HOLD-0".to_owned(),
            basis_points: 10_000,
        });
        graph
            .set_initial_ownership(
                "FIRM-0",
                vec![
                    OwnershipStake {
                        owner_id: "HH-0".to_owned(),
                        owned_id: "FIRM-0".to_owned(),
                        basis_points: 2_500,
                    },
                    OwnershipStake {
                        owner_id: "HOLD-0".to_owned(),
                        owned_id: "FIRM-0".to_owned(),
                        basis_points: 7_500,
                    },
                ],
            )
            .unwrap();
        let owners = graph.ultimate_owners("FIRM-0").unwrap();
        assert_eq!(owners.get("HH-0"), Some(&0.25));
        assert_eq!(owners.get("HH-1"), Some(&0.75));
    }

    #[test]
    fn rejects_cycles_and_preserves_previous_valid_stakes() {
        let mut graph = graph();
        graph.entities.push(InstitutionalEntity {
            id: "HOLD-0".to_owned(),
            legal_form: LegalForm::Corporation,
            tax_classification: TaxClassification::Unknown,
            economic_role: EconomicRole::HoldingCompany,
            jurisdiction: "US-UNSPECIFIED".to_owned(),
            formed_month: 0,
        });
        graph.stakes.push(OwnershipStake {
            owner_id: "FIRM-0".to_owned(),
            owned_id: "HOLD-0".to_owned(),
            basis_points: 10_000,
        });
        graph.validate().unwrap();
        let old = graph.stakes.clone();
        assert!(
            graph
                .set_initial_ownership(
                    "FIRM-0",
                    vec![OwnershipStake {
                        owner_id: "HOLD-0".to_owned(),
                        owned_id: "FIRM-0".to_owned(),
                        basis_points: 10_000,
                    }],
                )
                .is_err()
        );
        assert_eq!(graph.stakes, old);
    }

    #[test]
    fn rejects_missing_shares_and_unknown_owner() {
        let mut graph = graph();
        graph.stakes[0].basis_points = 9_999;
        assert!(graph.validate().is_err());
        graph.stakes[0].basis_points = 10_000;
        graph.stakes[0].owner_id = "unknown".to_owned();
        assert!(graph.validate().is_err());
    }
}
