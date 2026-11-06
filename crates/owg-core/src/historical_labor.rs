use crate::economy::Economy;
use crate::error::{OwgError, OwgResult};
use crate::historical_households::{
    HouseholdDemand, HouseholdDemandOpening, HouseholdDemandPolicy,
};
use crate::historical_production::{InventoryPolicy, ProductionOpeningReport};
use crate::historical_recruiting::{Recruiting, RecruitingFrame, RecruitingSpec};
use crate::rng::SeedBook;
use serde::{Deserialize, Serialize};

pub const HISTORICAL_LABOR_VERSION: &str = "owg.historical-labor.v1";
pub const RECRUITING_LABOR_VERSION: &str = "owg.historical-labor.v2";
const EMPLOYED: usize = 0;
const UNEMPLOYED: usize = 1;
const OUTSIDE: usize = 2;

/// Row and column order is employed, unemployed, outside the labor force.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoricalLaborSpec {
    pub schema_version: String,
    pub source_manifest_sha256: String,
    pub opening_unemployment: f64,
    pub opening_participation: f64,
    pub transitions: [[f64; 3]; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recruiting: Option<RecruitingSpec>,
}

impl HistoricalLaborSpec {
    pub(crate) fn opening_population(&self, count: usize) -> (usize, usize) {
        let workers =
            ((count as f64 * self.opening_participation).round() as usize).clamp(1, count);
        let jobless = (workers as f64 * self.opening_unemployment).round() as usize;
        (workers, jobless.min(workers))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct LaborFrame {
    pub population: [usize; 3],
    pub transitions: [[usize; 3]; 3],
    pub applications: usize,
    pub vacancies: usize,
    pub matches: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recruiting: Option<RecruitingFrame>,
}

/// Experimental run-local state. Ordinary scenarios and checkpoints keep
/// their original labor model and serialized state.
#[derive(Debug)]
pub(crate) struct HistoricalLabor {
    spec: HistoricalLaborSpec,
    calibrated_production: bool,
    inventory: Option<InventoryPolicy>,
    pub(crate) household_demand: Option<HouseholdDemand>,
    recruiting: Option<Recruiting>,
    outside: Vec<bool>,
    applications: usize,
    vacancies: usize,
    matches: usize,
}

impl HistoricalLabor {
    pub(crate) fn initialize(
        spec: HistoricalLaborSpec,
        economy: &mut Economy,
        production: Option<&ProductionOpeningReport>,
    ) -> OwgResult<Self> {
        let share = |value: f64| (0.0..=1.0).contains(&value);
        let supported = match &spec.recruiting {
            None => spec.schema_version == HISTORICAL_LABOR_VERSION,
            Some(recruiting) => {
                spec.schema_version == RECRUITING_LABOR_VERSION && recruiting.valid()
            }
        };
        if !supported
            || spec.source_manifest_sha256.len() != 64
            || !spec
                .source_manifest_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
            || !share(spec.opening_unemployment)
            || !share(spec.opening_participation)
            || spec.opening_participation == 0.0
            || spec.transitions.iter().any(|row| {
                row.iter().any(|p| !share(*p)) || (row.iter().sum::<f64>() - 1.0).abs() > 1e-9
            })
        {
            return Err(OwgError::InvalidState(
                "invalid historical labor input".to_owned(),
            ));
        }
        let count = economy.households.len();
        let (workers, jobless) = spec.opening_population(count);
        let employed = workers - jobless;
        let seeds = SeedBook::new(economy.root_seed);
        let mut order: Vec<_> = (0..count).collect();
        order.sort_by_cached_key(|index| {
            seeds
                .unit("historical-labor-opening", *index as u64, 0, 0)
                .to_bits()
        });
        let employer_slots: Vec<_> = if let Some(production) = production {
            production
                .sectors
                .iter()
                .flat_map(|sector| {
                    economy
                        .firms
                        .sector
                        .iter()
                        .enumerate()
                        .filter(move |(_, industry)| **industry == sector.sector)
                        .map(|(index, _)| index)
                        .cycle()
                        .take(sector.workers)
                })
                .collect()
        } else {
            (0..economy.firms.len()).cycle().take(employed).collect()
        };
        let mut outside = vec![false; count];
        for (rank, index) in order.into_iter().enumerate() {
            economy.households.employer[index] = employer_slots.get(rank).copied();
            outside[index] = rank >= workers;
        }
        economy.last_unemployment = jobless as f64 / workers as f64;
        let recruiting = spec.recruiting.as_ref().map(|input| {
            let mut counts = vec![0; economy.firms.len()];
            for &firm in economy.households.employer.iter().flatten() {
                counts[firm] += 1;
            }
            Recruiting::new(input.clone(), counts, seeds)
        });
        Ok(Self {
            spec,
            calibrated_production: production.is_some(),
            inventory: production.and_then(|report| report.inventory.clone()),
            household_demand: None,
            recruiting,
            outside,
            applications: 0,
            vacancies: 0,
            matches: 0,
        })
    }

    pub(crate) fn bind_demand(
        &mut self,
        economy: &mut Economy,
        policy: HouseholdDemandPolicy,
    ) -> OwgResult<HouseholdDemandOpening> {
        let (demand, report) = HouseholdDemand::initialize(policy, economy, &self.outside)?;
        self.household_demand = Some(demand);
        Ok(report)
    }

    pub(crate) fn calibrated_production(&self) -> bool {
        self.calibrated_production
    }

    pub(crate) fn inventory(&self) -> Option<&InventoryPolicy> {
        self.inventory.as_ref()
    }

    pub(crate) fn outside(&self, index: usize) -> bool {
        self.outside[index]
    }

    pub(crate) fn statuses(&self, economy: &Economy) -> Vec<usize> {
        economy
            .households
            .employer
            .iter()
            .enumerate()
            .map(|(index, employer)| {
                if employer.is_some() {
                    EMPLOYED
                } else if self.outside[index] {
                    OUTSIDE
                } else {
                    UNEMPLOYED
                }
            })
            .collect()
    }

    pub(crate) fn frame(&self, before: &[usize], economy: &Economy) -> LaborFrame {
        let mut population = [0; 3];
        let mut transitions = [[0; 3]; 3];
        for (&from, to) in before.iter().zip(self.statuses(economy)) {
            population[to] += 1;
            transitions[from][to] += 1;
        }
        LaborFrame {
            population,
            transitions,
            applications: self.applications,
            vacancies: self.vacancies,
            matches: self.matches,
            recruiting: self.recruiting.as_ref().map(Recruiting::frame),
        }
    }

    pub(crate) fn match_workers(
        &mut self,
        economy: &mut Economy,
        desired: &[usize],
        mut employees: Vec<usize>,
        month: u32,
        seeds: SeedBook,
    ) {
        let previous = self.statuses(economy);
        let initial_staffing = employees.clone();
        let mut applicants = Vec::new();
        for (worker, from) in previous.into_iter().enumerate() {
            let [mut to_e, mut to_u, to_out] = self.spec.transitions[from];
            if let Some(recruiting) = &self.recruiting {
                if let Some(firm) = economy.households.employer[worker] {
                    let stress = recruiting.stress_separation(firm, desired, &initial_staffing);
                    to_e -= stress.min(to_e);
                    to_u = 1.0 - to_out - to_e;
                } else if from == UNEMPLOYED {
                    to_e = recruiting.employment_draw(to_e, to_out);
                    to_u = 1.0 - to_out - to_e;
                } else {
                    to_e = recruiting.employment_draw(to_e, to_u);
                }
            }
            let draw = seeds.unit("historical-labor-transition", worker as u64, month, 0);
            let proposed = if draw < to_e {
                EMPLOYED
            } else if draw < to_e + to_u {
                UNEMPLOYED
            } else {
                OUTSIDE
            };
            if let Some(firm) = economy.households.employer[worker] {
                if economy.firms.active[firm] && proposed == EMPLOYED {
                    continue;
                }
                if economy.firms.active[firm] {
                    employees[firm] -= 1;
                }
                economy.households.employer[worker] = None;
                self.outside[worker] = proposed == OUTSIDE;
            } else if proposed == EMPLOYED {
                applicants.push(worker);
            } else {
                self.outside[worker] = proposed == OUTSIDE;
            }
        }

        let mut unconstrained;
        let contacts = self.recruiting.as_ref().map(Recruiting::contacts);
        let vacancies = if let Some(recruiting) = self.recruiting.as_mut() {
            recruiting.advance(desired, &employees, month, seeds)
        } else {
            unconstrained = desired
                .iter()
                .zip(&employees)
                .map(|(wanted, staffed)| wanted.saturating_sub(*staffed))
                .collect::<Vec<_>>();
            &mut unconstrained
        };
        self.applications = applicants.len();
        self.vacancies = vacancies.iter().sum();
        self.matches = 0;
        // Vary search order each period so array position does not confer priority.
        applicants.sort_by_cached_key(|worker| {
            seeds
                .unit("historical-labor-priority", *worker as u64, month, 0)
                .to_bits()
        });
        for worker in applicants {
            let selected = if let Some(contacts) = contacts {
                (0..contacts)
                    .map(|draw| {
                        seeds.index(
                            "recruiting-contact",
                            worker as u64,
                            month,
                            draw as u32,
                            vacancies.len(),
                        )
                    })
                    .find(|firm| vacancies[*firm] > 0)
            } else {
                let start = seeds.index("job-search", worker as u64, month, 0, vacancies.len());
                (0..vacancies.len().min(12))
                    .map(|offset| (start + offset) % vacancies.len())
                    .find(|firm| vacancies[*firm] > 0)
            };
            if let Some(firm) = selected {
                vacancies[firm] -= 1;
                economy.households.employer[worker] = Some(firm);
                self.outside[worker] = false;
                self.matches += 1;
            }
        }
    }
}
