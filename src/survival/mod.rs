//! Survival Module
//!
//! Resource monitoring, low-compute mode management, and funding strategies
//! for autonomous operation. Ensures the automaton can sustain itself
//! by tracking resources, adapting compute usage, and securing funding.

pub mod funding;
pub mod low_compute;
pub mod monitor;

pub use funding::{execute_funding_strategies, FundingAttempt};
pub use low_compute::{
    apply_tier_restrictions, can_run_inference, get_model_for_tier, record_transition,
    ModeTransition,
};
pub use monitor::{check_resources, format_resource_report, ResourceStatus};
