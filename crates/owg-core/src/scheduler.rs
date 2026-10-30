use crate::error::{OwgError, OwgResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const SCHEDULER_VERSION: &str = "openworldgen.scheduler.v1";

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SimulationPhase {
    Boundary,
    Policy,
    Entry,
    Labor,
    Fiscal,
    Pricing,
    Production,
    Settlement,
    GoodsMarket,
    External,
    Resolution,
    Learning,
    Reporting,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SimulationEventKind {
    ResetMonth,
    OriginateContractCredit,
    ServiceContractCredit,
    ResolveContractCredit,
    UpdatePolicy,
    RestartInactiveFirms,
    MatchLabor,
    ApplyTargetedTransfers,
    ApplyUnemploymentBenefits,
    UpdateFirmPrices,
    CaptureOpeningInventory,
    ProduceAndImport,
    PayWages,
    ServiceHouseholdDebt,
    GoodsMarket { week: u8 },
    SettleForeignDemand,
    SettleGovernmentDemand,
    ServiceFirmDebt,
    ResolveFirmDefaults,
    ResolveBanks,
    UpdateLearning,
    CollectMetrics,
    CloseMonth,
}

impl SimulationEventKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::OriginateContractCredit => "originate_contract_credit",
            Self::ServiceContractCredit => "service_contract_credit",
            Self::ResolveContractCredit => "resolve_contract_credit",
            Self::ResetMonth => "reset_month",
            Self::UpdatePolicy => "update_policy",
            Self::RestartInactiveFirms => "restart_inactive_firms",
            Self::MatchLabor => "match_labor",
            Self::ApplyTargetedTransfers => "apply_targeted_transfers",
            Self::ApplyUnemploymentBenefits => "apply_unemployment_benefits",
            Self::UpdateFirmPrices => "update_firm_prices",
            Self::CaptureOpeningInventory => "capture_opening_inventory",
            Self::ProduceAndImport => "produce_and_import",
            Self::PayWages => "pay_wages",
            Self::ServiceHouseholdDebt => "service_household_debt",
            Self::GoodsMarket { .. } => "goods_market",
            Self::SettleForeignDemand => "settle_foreign_demand",
            Self::SettleGovernmentDemand => "settle_government_demand",
            Self::ServiceFirmDebt => "service_firm_debt",
            Self::ResolveFirmDefaults => "resolve_firm_defaults",
            Self::ResolveBanks => "resolve_banks",
            Self::UpdateLearning => "update_learning",
            Self::CollectMetrics => "collect_metrics",
            Self::CloseMonth => "close_month",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ScheduledEvent {
    pub month: u32,
    pub within_month_order: u16,
    pub phase: SimulationPhase,
    pub priority: u16,
    pub actor_id: String,
    pub sequence: u32,
    pub kind: SimulationEventKind,
}

impl ScheduledEvent {
    #[must_use]
    pub fn identifier(&self) -> String {
        format!(
            "{:010}:{:05}:{:05}:{}:{:05}:{}",
            self.month,
            self.within_month_order,
            self.priority,
            self.actor_id,
            self.sequence,
            self.kind.label()
        )
    }

    fn order_key(&self) -> (u32, u16, u16, &str, u32) {
        (
            self.month,
            self.within_month_order,
            self.priority,
            self.actor_id.as_str(),
            self.sequence,
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MultiRateScheduler {
    weeks_per_month: u8,
}

impl MultiRateScheduler {
    pub fn new(weeks_per_month: u8) -> OwgResult<Self> {
        if weeks_per_month == 0 {
            return Err(OwgError::InvalidState(
                "scheduler requires at least one week per month".to_owned(),
            ));
        }
        Ok(Self { weeks_per_month })
    }

    #[must_use]
    pub const fn weeks_per_month(self) -> u8 {
        self.weeks_per_month
    }

    /// Preserve the legacy schedule exactly when no contract market is enabled.
    #[must_use]
    pub fn events_for_month_with_credit(
        self,
        month: u32,
        order: Option<crate::credit::CreditServiceOrder>,
    ) -> Vec<ScheduledEvent> {
        let mut events = self.events_for_month(month);
        if let Some(order) = order {
            let (service_order, service_priority) = match order {
                crate::credit::CreditServiceOrder::BeforeLegacyDebt => (80, 5),
                crate::credit::CreditServiceOrder::AfterLegacyDebt => (225, 0),
            };
            for (offset, (within_month_order, phase, priority, kind)) in [
                (
                    15,
                    SimulationPhase::Boundary,
                    0,
                    SimulationEventKind::OriginateContractCredit,
                ),
                (
                    service_order,
                    SimulationPhase::Settlement,
                    service_priority,
                    SimulationEventKind::ServiceContractCredit,
                ),
                (
                    230,
                    SimulationPhase::Resolution,
                    5,
                    SimulationEventKind::ResolveContractCredit,
                ),
            ]
            .into_iter()
            .enumerate()
            {
                events.push(ScheduledEvent {
                    month,
                    within_month_order,
                    phase,
                    priority,
                    actor_id: "SYSTEM".to_owned(),
                    sequence: u32::from(self.weeks_per_month) + 19 + offset as u32,
                    kind,
                });
            }
            events.sort_by(|left, right| left.order_key().cmp(&right.order_key()));
        }
        events
    }

    #[must_use]
    pub fn events_for_month(self, month: u32) -> Vec<ScheduledEvent> {
        let mut events = Vec::with_capacity(usize::from(self.weeks_per_month) + 19);
        let mut sequence = 0_u32;
        let mut push = |within_month_order, phase, priority, kind| {
            events.push(ScheduledEvent {
                month,
                within_month_order,
                phase,
                priority,
                actor_id: "SYSTEM".to_owned(),
                sequence,
                kind,
            });
            sequence += 1;
        };

        push(
            10,
            SimulationPhase::Boundary,
            0,
            SimulationEventKind::ResetMonth,
        );
        push(
            20,
            SimulationPhase::Policy,
            0,
            SimulationEventKind::UpdatePolicy,
        );
        push(
            30,
            SimulationPhase::Entry,
            0,
            SimulationEventKind::RestartInactiveFirms,
        );
        push(
            40,
            SimulationPhase::Labor,
            0,
            SimulationEventKind::MatchLabor,
        );
        push(
            50,
            SimulationPhase::Fiscal,
            10,
            SimulationEventKind::ApplyTargetedTransfers,
        );
        push(
            50,
            SimulationPhase::Fiscal,
            20,
            SimulationEventKind::ApplyUnemploymentBenefits,
        );
        push(
            60,
            SimulationPhase::Pricing,
            0,
            SimulationEventKind::UpdateFirmPrices,
        );
        push(
            70,
            SimulationPhase::Production,
            0,
            SimulationEventKind::CaptureOpeningInventory,
        );
        push(
            70,
            SimulationPhase::Production,
            10,
            SimulationEventKind::ProduceAndImport,
        );
        push(
            80,
            SimulationPhase::Settlement,
            0,
            SimulationEventKind::PayWages,
        );
        push(
            80,
            SimulationPhase::Settlement,
            10,
            SimulationEventKind::ServiceHouseholdDebt,
        );
        for week in 0..self.weeks_per_month {
            push(
                100 + u16::from(week),
                SimulationPhase::GoodsMarket,
                0,
                SimulationEventKind::GoodsMarket { week },
            );
        }
        push(
            200,
            SimulationPhase::External,
            0,
            SimulationEventKind::SettleForeignDemand,
        );
        push(
            210,
            SimulationPhase::Fiscal,
            0,
            SimulationEventKind::SettleGovernmentDemand,
        );
        push(
            220,
            SimulationPhase::Settlement,
            0,
            SimulationEventKind::ServiceFirmDebt,
        );
        push(
            230,
            SimulationPhase::Resolution,
            0,
            SimulationEventKind::ResolveFirmDefaults,
        );
        push(
            230,
            SimulationPhase::Resolution,
            10,
            SimulationEventKind::ResolveBanks,
        );
        push(
            240,
            SimulationPhase::Learning,
            0,
            SimulationEventKind::UpdateLearning,
        );
        push(
            250,
            SimulationPhase::Reporting,
            0,
            SimulationEventKind::CollectMetrics,
        );
        push(
            260,
            SimulationPhase::Boundary,
            0,
            SimulationEventKind::CloseMonth,
        );
        events
    }
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
pub struct ScheduleAudit {
    rolling_hash: [u8; 32],
    pub event_count: u64,
    pub events_by_kind: BTreeMap<String, u64>,
    pub events_by_month: BTreeMap<u32, u64>,
    pub first_event_id: Option<String>,
    pub last_event_id: Option<String>,
    last_event: Option<ScheduledEvent>,
}

impl ScheduleAudit {
    pub fn record(&mut self, event: &ScheduledEvent) -> OwgResult<()> {
        if self
            .last_event
            .as_ref()
            .is_some_and(|previous| previous.order_key() >= event.order_key())
        {
            return Err(OwgError::InvalidState(format!(
                "scheduled event order is not strictly increasing at {}",
                event.identifier()
            )));
        }
        let encoded = serde_json::to_vec(event)?;
        let mut rolling = Sha256::new();
        rolling.update(self.rolling_hash);
        rolling.update((encoded.len() as u64).to_le_bytes());
        rolling.update(encoded);
        self.rolling_hash = rolling.finalize().into();

        let identifier = event.identifier();
        self.event_count += 1;
        *self
            .events_by_kind
            .entry(event.kind.label().to_owned())
            .or_insert(0) += 1;
        *self.events_by_month.entry(event.month).or_insert(0) += 1;
        if self.first_event_id.is_none() {
            self.first_event_id = Some(identifier.clone());
        }
        self.last_event_id = Some(identifier);
        self.last_event = Some(event.clone());
        Ok(())
    }

    #[must_use]
    pub fn schedule_hash(&self) -> String {
        hex::encode(self.rolling_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::{MultiRateScheduler, SCHEDULER_VERSION, ScheduleAudit, SimulationEventKind};

    #[test]
    fn canonical_month_has_typed_weekly_and_monthly_events() {
        assert_eq!(SCHEDULER_VERSION, "openworldgen.scheduler.v1");
        let events = MultiRateScheduler::new(4).unwrap().events_for_month(7);
        assert_eq!(events.len(), 23);
        let weeks: Vec<u8> = events
            .iter()
            .filter_map(|event| match event.kind {
                SimulationEventKind::GoodsMarket { week } => Some(week),
                _ => None,
            })
            .collect();
        assert_eq!(weeks, vec![0, 1, 2, 3]);
        assert!(events.windows(2).all(|pair| {
            pair[0].within_month_order < pair[1].within_month_order
                || (pair[0].within_month_order == pair[1].within_month_order
                    && pair[0].priority < pair[1].priority)
        }));
    }

    #[test]
    fn schedule_audit_is_ordered_and_replayable() {
        let events = MultiRateScheduler::new(2).unwrap().events_for_month(0);
        let mut first = ScheduleAudit::default();
        let mut second = ScheduleAudit::default();
        for event in &events {
            first.record(event).unwrap();
            second.record(event).unwrap();
        }
        assert_eq!(first.schedule_hash(), second.schedule_hash());
        assert_eq!(first.event_count, 21);
        assert_eq!(first.events_by_kind["goods_market"], 2);
        assert!(first.record(&events[0]).is_err());
    }
}
