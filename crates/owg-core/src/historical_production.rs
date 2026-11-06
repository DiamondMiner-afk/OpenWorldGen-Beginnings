use crate::contracts::Sector;
use crate::economy::Economy;
use crate::error::{OwgError, OwgResult};
use crate::historical_households::HouseholdDemandPolicy;
use crate::money::Money;
use crate::rng::SeedBook;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionOpeningSpec {
    pub schema_version: String,
    pub source_sha256: String,
    /// Energy, food, manufacturing, services.
    pub consumption_shares: [f64; 4],
    pub payroll_shares: [f64; 4],
    #[serde(default)]
    pub inventory: Option<InventoryPolicy>,
    #[serde(default)]
    pub household_demand: Option<HouseholdDemandPolicy>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryPolicy {
    pub source_sha256: String,
    pub food: InventoryRule,
    pub manufacturing: InventoryRule,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryRule {
    pub opening_months: f64,
    pub target_months: f64,
    pub adjustment: f64,
}

impl InventoryPolicy {
    fn valid(&self) -> bool {
        self.source_sha256.len() == 64
            && self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            && [&self.food, &self.manufacturing].into_iter().all(|rule| {
                (0.0..=5.0).contains(&rule.opening_months)
                    && (0.0..=5.0).contains(&rule.target_months)
                    && (0.0..=1.0).contains(&rule.adjustment)
            })
    }

    pub(crate) fn rule(&self, sector: Sector) -> Option<&InventoryRule> {
        match sector {
            Sector::Food => Some(&self.food),
            Sector::Manufacturing => Some(&self.manufacturing),
            Sector::Energy | Sector::Services => None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ProductionOpeningReport {
    pub source_sha256: String,
    pub sectors: [OpeningSector; 4],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inventory: Option<InventoryPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub household_demand: Option<HouseholdDemandPolicy>,
}

#[derive(Debug, Serialize)]
pub struct OpeningSector {
    pub sector: Sector,
    pub firms: usize,
    pub workers: usize,
    pub consumption_share: f64,
    pub payroll_share: f64,
}

fn allocate(weights: [f64; 4], total: usize) -> [usize; 4] {
    let quotas = weights.map(|weight| weight * total as f64);
    let mut seats = quotas.map(|quota| quota.floor() as usize);
    let mut order = [0_usize, 1, 2, 3];
    order.sort_by(|&a, &b| quotas[b].fract().total_cmp(&quotas[a].fract()));
    for &index in order.iter().take(total - seats.iter().sum::<usize>()) {
        seats[index] += 1;
    }
    seats
}

pub(crate) fn install(
    economy: &mut Economy,
    spec: ProductionOpeningSpec,
    employed: usize,
) -> OwgResult<ProductionOpeningReport> {
    let valid = |weights: [f64; 4]| {
        weights
            .into_iter()
            .all(|weight| weight > 0.0 && weight < 1.0)
            && (weights.iter().sum::<f64>() - 1.0).abs() < 1e-12
    };
    let supported = match (&spec.inventory, &spec.household_demand) {
        (None, None) => spec.schema_version == "owg.historical-production.v1",
        (Some(policy), None) => {
            spec.schema_version == "owg.historical-production.v2" && policy.valid()
        }
        (Some(policy), Some(demand)) => {
            spec.schema_version == "owg.historical-production.v3"
                && policy.valid()
                && demand.valid()
        }
        (None, Some(_)) => false,
    };
    if !supported
        || spec.source_sha256.len() != 64
        || !spec.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || !valid(spec.consumption_shares)
        || !valid(spec.payroll_shares)
    {
        return Err(OwgError::InvalidState(
            "invalid historical production opening".to_owned(),
        ));
    }
    let seeds = SeedBook::new(economy.root_seed);
    let firms = economy.firms.len();
    let counts = allocate(spec.payroll_shares, firms - 4).map(|count| count + 1);
    let workers = allocate(spec.payroll_shares, employed);
    let mut order: Vec<_> = (0..firms).collect();
    order.sort_by_cached_key(|index| {
        seeds
            .unit("production-opening-sector", *index as u64, 0, 0)
            .to_bits()
    });
    let scale = economy
        .firms
        .expected_demand
        .iter()
        .zip(&economy.firms.price)
        .map(|(quantity, price)| quantity * price.as_major_f64())
        .sum::<f64>();
    let baseline = &mut economy.parameters.sector;
    economy.parameters.weekly_energy_subsistence_floor *=
        spec.consumption_shares[0] / baseline.consumption_share[0];
    economy.parameters.weekly_food_subsistence_floor *=
        spec.consumption_shares[1] / baseline.consumption_share[1];
    baseline.consumption_share = spec.consumption_shares;
    let mut start = 0;
    for sector in Sector::ALL {
        let index = sector.index();
        let stop = start + counts[index];
        let members = &order[start..stop];
        let demand_weights: f64 = members
            .iter()
            .map(|&firm| economy.firms.expected_demand[firm])
            .sum();
        let staffing = (workers[index] as f64 / counts[index] as f64).max(1.0);
        for &firm in members {
            let old_demand = economy.firms.expected_demand[firm];
            let price = Money::from_major_f64(
                baseline.base_price_major[index]
                    * seeds.range("firm-price", firm as u64, 0, 0, 0.94, 1.06),
            )?;
            let demand = scale * spec.consumption_shares[index] * old_demand
                / (price.as_major_f64() * demand_weights);
            economy.firms.sector[firm] = sector;
            economy.firms.price[firm] = price;
            economy.firms.expected_demand[firm] = demand;
            economy.firms.productivity[firm] = demand / staffing;
            economy.firms.capacity[firm] *= demand / old_demand;
            economy.firms.inventory[firm] *= demand / old_demand;
            if let Some(policy) = &spec.inventory {
                if sector == Sector::Services {
                    economy.firms.inventory[firm] = 0.0;
                } else if let Some(rule) = policy.rule(sector) {
                    economy.firms.inventory[firm] = demand * rule.opening_months;
                }
            }
            economy.firms.energy_intensity[firm] = baseline.energy_intensity[index]
                * seeds.range("firm-energy", firm as u64, 0, 0, 0.85, 1.15);
        }
        start = stop;
    }
    economy.base_indirect_tax = baseline.indirect_tax_rate;
    economy.base_sector_prices = economy.average_sector_prices();
    Ok(ProductionOpeningReport {
        source_sha256: spec.source_sha256,
        inventory: spec.inventory,
        household_demand: spec.household_demand,
        sectors: Sector::ALL.map(|sector| {
            let index = sector.index();
            OpeningSector {
                sector,
                firms: counts[index],
                workers: workers[index],
                consumption_share: spec.consumption_shares[index],
                payroll_share: spec.payroll_shares[index],
            }
        }),
    })
}
