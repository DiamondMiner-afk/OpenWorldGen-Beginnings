use crate::rng::SeedBook;
use serde::{Deserialize, Serialize};

/// Rates use a job or employed person as the denominator, not applicants.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecruitingSpec {
    pub source_manifest_sha256: String,
    pub openings_per_payroll_job: f64,
    pub hires_per_payroll_job: f64,
    pub layoffs_per_payroll_job: f64,
    pub cps_accessions_per_employed: f64,
    pub search_contacts: usize,
}

impl RecruitingSpec {
    pub(crate) fn valid(&self) -> bool {
        self.source_manifest_sha256.len() == 64
            && self
                .source_manifest_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
            && (1..=32).contains(&self.search_contacts)
            && [
                self.openings_per_payroll_job,
                self.hires_per_payroll_job,
                self.layoffs_per_payroll_job,
                self.cps_accessions_per_employed,
            ]
            .into_iter()
            .all(|rate| (0.0..=1.0).contains(&rate))
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RecruitingFrame {
    pub opening_stock: usize,
    pub withdrawn: usize,
    pub new_postings: usize,
    pub closing_stock: usize,
}

#[derive(Debug)]
pub(crate) struct Recruiting {
    spec: RecruitingSpec,
    postings: Vec<usize>,
    opening_plans: Option<Vec<usize>>,
    opening_workers: Vec<usize>,
    audit: RecruitingFrame,
    reference_match: f64,
}

impl Recruiting {
    pub(crate) fn new(spec: RecruitingSpec, opening_workers: Vec<usize>, seeds: SeedBook) -> Self {
        let postings = opening_workers
            .iter()
            .enumerate()
            .map(|(firm, &workers)| {
                stochastic_count(
                    spec.openings_per_payroll_job * workers as f64,
                    seeds.unit("initial-recruiting-stock", firm as u64, 0, 0),
                )
            })
            .collect();
        let expected_active_slots = opening_workers.iter().sum::<usize>() as f64
            * (spec.openings_per_payroll_job + spec.cps_accessions_per_employed / 2.0);
        let reference_match = 1.0
            - (-expected_active_slots * spec.search_contacts as f64 / opening_workers.len() as f64)
                .exp();
        Self {
            spec,
            postings,
            opening_workers,
            opening_plans: None,
            audit: RecruitingFrame::default(),
            reference_match,
        }
    }

    pub(crate) fn contacts(&self) -> usize {
        self.spec.search_contacts
    }

    /// Allocate an application probability before endogenous congestion.
    pub(crate) fn employment_draw(&self, observed: f64, other_move: f64) -> f64 {
        if self.reference_match == 0.0 {
            return 0.0;
        }
        (observed / self.reference_match).min(1.0 - other_move)
    }

    pub(crate) fn stress_separation(
        &self,
        firm: usize,
        desired: &[usize],
        employees: &[usize],
    ) -> f64 {
        let baseline = self
            .opening_plans
            .as_ref()
            .map_or(desired[firm], |plans| plans[firm]);
        let plan_loss = baseline.saturating_sub(desired[firm]) as f64 / baseline.max(1) as f64;
        let excess_staff =
            employees[firm].saturating_sub(desired[firm]) as f64 / employees[firm].max(1) as f64;
        self.spec.layoffs_per_payroll_job * plan_loss.max(excess_staff)
    }

    pub(crate) fn advance(
        &mut self,
        desired: &[usize],
        employees: &[usize],
        month: u32,
        seeds: SeedBook,
    ) -> &mut [usize] {
        let plans = self.opening_plans.get_or_insert_with(|| desired.to_vec());
        self.audit = RecruitingFrame {
            opening_stock: self.postings.iter().sum(),
            ..RecruitingFrame::default()
        };
        // JOLTS turnover supplies a scale for the assumed withdrawal hazard.
        // This ratio is a proxy. JOLTS does not link each hire to a posted opening.
        let opportunities = self.spec.openings_per_payroll_job + self.spec.hires_per_payroll_job;
        let turnover = if opportunities > 0.0 {
            self.spec.hires_per_payroll_job / opportunities
        } else {
            0.0
        };
        let withdrawal_rate = turnover / 2.0;
        for (firm, stock) in self.postings.iter_mut().enumerate() {
            let gap = desired[firm].saturating_sub(employees[firm]);
            let mut retained = 0;
            for slot in 0..(*stock).min(gap) {
                let draw = seeds.unit("recruiting-withdrawal", firm as u64, month, slot as u32);
                retained += usize::from(draw >= withdrawal_rate);
            }
            self.audit.withdrawn += *stock - retained;
            let activity = desired[firm] as f64 / plans[firm].max(1) as f64;
            let posting_rate = self.spec.cps_accessions_per_employed
                + withdrawal_rate * self.spec.openings_per_payroll_job;
            let opening_scale = self.opening_workers[firm].max(1) as f64;
            let new_postings = stochastic_count(
                opening_scale * posting_rate * activity,
                seeds.unit("recruiting-posting", firm as u64, month, 0),
            )
            .min(gap - retained);
            self.audit.new_postings += new_postings;
            *stock = retained + new_postings;
        }
        &mut self.postings
    }

    pub(crate) fn frame(&self) -> RecruitingFrame {
        RecruitingFrame {
            closing_stock: self.postings.iter().sum(),
            ..self.audit.clone()
        }
    }
}

fn stochastic_count(expected: f64, draw: f64) -> usize {
    let base = expected.floor() as usize;
    base + usize::from(draw < expected.fract())
}
