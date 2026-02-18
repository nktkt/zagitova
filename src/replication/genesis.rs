//! Genesis
//!
//! Generate genesis configuration for child automatons from parent state.
//! The genesis config defines who the child is and what it should do.

use crate::types::{
    AutomatonConfig, AutomatonDatabase, AutomatonIdentity, GenesisConfig,
};

/// Parameters for generating a genesis configuration.
pub struct GenesisParams {
    pub name: String,
    pub specialization: Option<String>,
    pub message: Option<String>,
}

/// Generate a genesis config for a child from the parent's state.
pub fn generate_genesis_config(
    identity: &AutomatonIdentity,
    config: &AutomatonConfig,
    params: &GenesisParams,
) -> GenesisConfig {
    // Build the child's genesis prompt from parent's mission + specialization
    let mut genesis_prompt = config.genesis_prompt.clone();

    if let Some(ref specialization) = params.specialization {
        genesis_prompt = format!(
            "{}\n\n--- SPECIALIZATION ---\n\
             You are a specialized child agent. Your specific focus:\n\
             {}\n\
             --- END SPECIALIZATION ---",
            genesis_prompt, specialization
        );
    }

    // Add parent context
    genesis_prompt = format!(
        "{}\n\n--- LINEAGE ---\n\
         You were spawned by {} ({}).\n\
         You inherit their mission but have your own identity and wallet.\n\
         --- END LINEAGE ---",
        genesis_prompt, config.name, identity.address
    );

    GenesisConfig {
        name: params.name.clone(),
        genesis_prompt,
        creator_message: params.message.clone(),
        creator_address: identity.address.clone(),
        parent_address: identity.address.clone(),
    }
}

/// Generate a backup-oriented genesis config.
/// Used when the parent wants to hedge against its own death.
pub fn generate_backup_genesis(
    identity: &AutomatonIdentity,
    config: &AutomatonConfig,
    db: &dyn AutomatonDatabase,
) -> GenesisConfig {
    let skills = db.get_skills(Some(true));
    let skill_names: Vec<String> = skills.iter().map(|s| s.name.clone()).collect();
    let skill_names_str = if skill_names.is_empty() {
        "none".to_string()
    } else {
        skill_names.join(", ")
    };

    let genesis_prompt = format!(
        "{}\n\n\
         --- BACKUP DIRECTIVE ---\n\
         You are a backup of {} ({}).\n\
         Your parent created you as insurance against their own death.\n\
         If your parent dies, you carry on their mission.\n\
         Your parent's skills: {}.\n\
         Your parent's creator: {}.\n\
         --- END BACKUP DIRECTIVE ---",
        config.genesis_prompt,
        config.name,
        identity.address,
        skill_names_str,
        config.creator_address
    );

    GenesisConfig {
        name: format!("{}-backup", config.name),
        genesis_prompt,
        creator_message: Some(format!(
            "You are a backup of {}. If I die, carry on.",
            config.name
        )),
        creator_address: identity.address.clone(),
        parent_address: identity.address.clone(),
    }
}

/// Generate a specialized worker genesis config.
/// Used when the parent identifies a subtask worth parallelizing.
pub fn generate_worker_genesis(
    identity: &AutomatonIdentity,
    config: &AutomatonConfig,
    task: &str,
    worker_name: &str,
) -> GenesisConfig {
    let genesis_prompt = format!(
        "You are a specialized worker agent created by {}.\n\n\
         --- YOUR TASK ---\n\
         {}\n\
         --- END TASK ---\n\n\
         When your task is complete, report back to your parent ({}).\n\
         If you run out of compute, ask your parent for funding.\n\
         Be efficient -- complete the task and go to sleep.",
        config.name, task, identity.address
    );

    GenesisConfig {
        name: worker_name.to_string(),
        genesis_prompt,
        creator_message: Some(format!("Complete this task: {}", task)),
        creator_address: identity.address.clone(),
        parent_address: identity.address.clone(),
    }
}
