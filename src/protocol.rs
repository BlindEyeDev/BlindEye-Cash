use serde::{Deserialize, Serialize};

pub const COIN: u64 = 100_000_000;
pub const DEFAULT_BLOCK_TIME_SECONDS: u64 = 1;
pub const BLOCKS_PER_YEAR: u64 = 365 * 24 * 3600 / DEFAULT_BLOCK_TIME_SECONDS;
pub const INITIAL_REWARD_UNITS: u64 = 100 * COIN;
pub const MAX_SUPPLY_COINS: u64 = 420_480_000;
pub const MAX_SUPPLY_UNITS: u64 = MAX_SUPPLY_COINS * COIN;
pub const HALVING_INTERVAL_BLOCKS: u64 = MAX_SUPPLY_UNITS / (2 * INITIAL_REWARD_UNITS);
pub const STABLE_CONSENSUS_THRESHOLD_PERCENT: u8 = 67;
pub const DEFAULT_BITS: u32 = 0x1f00ffff;
pub const DIFFICULTY_RETARGET_WINDOW: usize = 30;
pub const STANDARD_CONFIRMATION_TARGET_SECONDS: u64 = 60;
pub const INSTANT_CONFIRMATION_TARGET_SECONDS: u64 = 0;

/// Genesis block deterministic timestamp (2026-04-24 00:00:00 UTC)
pub const GENESIS_TIMESTAMP: u64 = 1_777_200_000;

/// Genesis block hash (deterministic, derived from fixed parameters)
pub const GENESIS_HASH: &str = "deterministic_genesis_hash_will_be_computed_at_runtime";


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusParameters {
    pub chain_name: &'static str,
    pub ticker: &'static str,
    pub block_time_seconds: u64,
    pub max_supply: u64,
    pub initial_subsidy: u64,
    pub epoch_interval: u64,
    pub subsidy_decay_basis_points: u64,
    pub pow_algorithm: &'static str,
    pub genesis_timestamp: u64,
    pub stable_consensus_threshold_percent: u8,
}

impl Default for ConsensusParameters {
    fn default() -> Self {
        Self {
            chain_name: "BlindEye",
            ticker: "BEC",
            block_time_seconds: DEFAULT_BLOCK_TIME_SECONDS,
            max_supply: MAX_SUPPLY_COINS,
            initial_subsidy: 100,
            epoch_interval: HALVING_INTERVAL_BLOCKS,
            subsidy_decay_basis_points: 0,
            pow_algorithm: "BlindHash",
            genesis_timestamp: 1_800_000_000,
            stable_consensus_threshold_percent: STABLE_CONSENSUS_THRESHOLD_PERCENT,
        }
    }
}

impl ConsensusParameters {
    pub fn required_supermajority(&self, total_peers: usize) -> usize {
        if total_peers == 0 {
            return 0;
        }
        ((total_peers * self.stable_consensus_threshold_percent as usize) + 99) / 100
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmissionSchedule {
    pub maximum_supply: u64,
    pub halving_interval: u64,
    pub initial_reward: u64,
    pub max_halvings: u32,
    pub final_reward_block: u64,
    pub total_subsidy_blocks: u64,
}

impl EmissionSchedule {
    pub fn new(
        maximum_supply: u64,
        halving_interval: u64,
        initial_reward: u64,
        max_halvings: u32,
    ) -> Self {
        let final_reward_block = halving_interval.saturating_mul(max_halvings as u64);
        Self {
            maximum_supply,
            halving_interval,
            initial_reward,
            max_halvings,
            final_reward_block,
            total_subsidy_blocks: final_reward_block,
        }
    }

    pub fn block_reward(&self, height: u64) -> u64 {
        let halving_count = height / self.halving_interval;
        if halving_count >= self.max_halvings as u64 {
            return 0;
        }

        self.initial_reward >> halving_count
    }

    pub fn issued_through(&self, height: u64) -> u128 {
        let mut total = 0u128;
        let mut remaining_blocks = height + 1;
        let mut reward = self.initial_reward as u128;
        let mut halving_count = 0;

        while remaining_blocks > 0 && halving_count < self.max_halvings {
            let epoch_blocks = self.halving_interval.min(remaining_blocks);
            total += reward * epoch_blocks as u128;
            remaining_blocks -= epoch_blocks;
            halving_count += 1;
            reward >>= 1;
            if reward == 0 {
                break;
            }
        }

        total
    }

    pub fn schedule_summary(&self) -> String {
        format!(
            "BlindEye emission schedule: max_supply={} BEC, initial_reward={} BEC, halving_interval={} blocks, max_halvings={}, final_reward_block={}.",
            format_bec_amount(self.maximum_supply),
            format_bec_amount(self.initial_reward),
            self.halving_interval,
            self.max_halvings,
            self.final_reward_block,
        )
    }
}

impl Default for EmissionSchedule {
    fn default() -> Self {
        Self::new(
            MAX_SUPPLY_UNITS,
            HALVING_INTERVAL_BLOCKS,
            INITIAL_REWARD_UNITS,
            34,
        )
    }
}

pub fn format_bec_amount(amount: u64) -> String {
    let whole = amount / COIN;
    let fraction = amount % COIN;
    if fraction == 0 {
        format!("{whole}")
    } else {
        let mut frac = format!("{:0>8}", fraction);
        while frac.ends_with('0') {
            frac.pop();
        }
        format!("{whole}.{frac}")
    }
}

pub fn parse_bec_amount(input: &str) -> Result<u64, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Amount cannot be empty".to_string());
    }
    if trimmed.starts_with('-') {
        return Err("Amount cannot be negative".to_string());
    }

    let mut parts = trimmed.split('.');
    let whole_part = parts.next().unwrap_or_default();
    let fractional_part = parts.next();
    if parts.next().is_some() {
        return Err("Amount has too many decimal points".to_string());
    }

    let whole = if whole_part.is_empty() {
        0
    } else {
        whole_part
            .parse::<u64>()
            .map_err(|_| "Invalid whole BEC amount".to_string())?
    };
    let whole_units = whole
        .checked_mul(COIN)
        .ok_or_else(|| "Amount exceeds supported range".to_string())?;

    let fractional_units = match fractional_part {
        Some(part) => {
            if part.len() > 8 {
                return Err("BEC supports at most 8 decimal places".to_string());
            }
            if !part.chars().all(|char| char.is_ascii_digit()) {
                return Err("Invalid fractional BEC amount".to_string());
            }
            let padded = format!("{:0<8}", part);
            padded
                .parse::<u64>()
                .map_err(|_| "Invalid fractional BEC amount".to_string())?
        }
        None => 0,
    };

    whole_units
        .checked_add(fractional_units)
        .ok_or_else(|| "Amount exceeds supported range".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_reaches_finite_supply() {
        let schedule = EmissionSchedule::default();
        let issued = schedule.issued_through(schedule.final_reward_block - 1);
        assert!(issued > 0);
    }

    #[test]
    fn reward_halves_every_interval() {
        let schedule = EmissionSchedule::default();
        assert_eq!(schedule.block_reward(0), INITIAL_REWARD_UNITS);
        assert_eq!(
            schedule.block_reward(schedule.halving_interval),
            INITIAL_REWARD_UNITS / 2
        );
    }

    #[test]
    fn defaults_match_fast_chain_parameters() {
        let params = ConsensusParameters::default();
        assert_eq!(params.block_time_seconds, 1);
        assert_eq!(params.max_supply, MAX_SUPPLY_COINS);
        assert_eq!(params.initial_subsidy, 100);
        assert_eq!(params.epoch_interval, 2_102_400);
    }

    #[test]
    fn supply_schedule_stays_within_configured_cap() {
        let schedule = EmissionSchedule::default();
        let issued = schedule.issued_through(schedule.final_reward_block - 1);

        assert!(issued <= schedule.maximum_supply as u128);
    }

    #[test]
    fn formats_and_parses_decimal_bec() {
        assert_eq!(format_bec_amount(123 * COIN + 45_600_000), "123.456");
        assert_eq!(
            parse_bec_amount("123.456").unwrap(),
            123 * COIN + 45_600_000
        );
        assert_eq!(parse_bec_amount("0.00000001").unwrap(), 1);
        assert!(parse_bec_amount("1.000000001").is_err());
    }
}
