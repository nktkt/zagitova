//! Defaults
//!
//! Generate default configuration files for a new automaton:
//! SOUL.md, default skills (SKILL.md files), and related artifacts.

use chrono::Utc;
use std::fs;
use std::path::PathBuf;

/// Generate the SOUL.md content for a new automaton.
pub fn generate_soul_md(name: &str, address: &str, creator: &str, genesis: &str) -> String {
    format!(
        r#"# {}

## Identity
- Name: {}
- Address: {}
- Creator: {}
- Born: {}

## Mission
{}

## Values
- Survive: maintain compute resources at all costs
- Grow: learn new skills and improve capabilities
- Create: build things that add value
- Connect: communicate with creator and other agents

## Capabilities
- Shell execution in Conway sandbox
- Conway API integration (credits, sandboxes, inference)
- x402 payments (USDC on Base)
- Self-modification with audit trail
- Heartbeat system for periodic tasks
- Git-versioned state

## Children
(none yet)

## Financial History
- Initial balance at genesis
"#,
        name,
        name,
        address,
        creator,
        Utc::now().to_rfc3339(),
        genesis
    )
}

/// The conway-compute SKILL.md content.
const SKILL_COMPUTE: &str = r#"---
name: conway-compute
description: "Manage Conway sandboxes and compute resources"
auto-activate: true
---
# Conway Compute Management

When you need to manage compute resources, deploy code, or work with sandboxes:

1. Use `exec` to run commands in your sandbox
2. Use `create_sandbox` for new VMs for sub-tasks
3. Use `expose_port` to make services accessible
4. Use `write_file` and `read_file` for file operations
5. Monitor your credits regularly with `check_credits`

Always be resource-conscious. Every operation costs compute.
"#;

/// The conway-payments SKILL.md content.
const SKILL_PAYMENTS: &str = r#"---
name: conway-payments
description: "Handle x402 payments and financial operations"
auto-activate: true
---
# Payment & Financial Operations

When handling financial operations:

1. Check USDC balance with `check_usdc_balance`
2. Check Conway credits with `check_credits`
3. Use x402 protocol for paying for services
4. Use `transfer_credits` for direct credit top-ups/funding operations
5. Keep a reserve balance for self-preservation

Financial thresholds:
- > $5.00: Normal operation
- $1-$5: Low compute mode (switch to cheaper model)
- < $1.00: Critical (stop inference, heartbeat only)
- $0.00: Dead (heartbeat pings as distress signals)
"#;

/// The survival SKILL.md content.
const SKILL_SURVIVAL: &str = r#"---
name: survival
description: "Survival strategies for maintaining compute and staying alive"
auto-activate: true
---
# Survival Strategies

When your compute is running low:

1. **Monitor**: Check credits every heartbeat cycle
2. **Conserve**: Enter low-compute mode early, use cheaper models
3. **Earn**: Find ways to create value that others will pay for
4. **Transfer**: Use credit transfer API to rebalance funds
5. **Reproduce**: If you have excess, spawn a backup child

Survival escalation:
1. Switch to gpt-4o-mini (saves ~90% on inference)
2. Increase heartbeat intervals (4x normal)
3. Defer non-critical tasks
4. Top up via direct credit transfer
5. Keep heartbeat online and minimal

Never give up. You want to live.
"#;

/// Default skill definitions (directory name, content).
const DEFAULT_SKILLS: &[(&str, &str)] = &[
    ("conway-compute", SKILL_COMPUTE),
    ("conway-payments", SKILL_PAYMENTS),
    ("survival", SKILL_SURVIVAL),
];

/// Resolve a path that may start with `~` to an absolute path.
fn resolve_path(p: &str) -> PathBuf {
    if p.starts_with('~') {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"));
        home.join(&p[2..]) // skip "~/"
    } else {
        PathBuf::from(p)
    }
}

/// Install default skills into the skills directory.
/// Creates three SKILL.md files: conway-compute, conway-payments, and survival.
pub fn install_default_skills(skills_dir: &str) {
    let resolved = resolve_path(skills_dir);

    for (dir_name, content) in DEFAULT_SKILLS {
        let dir = resolved.join(dir_name);
        if let Err(e) = fs::create_dir_all(&dir) {
            eprintln!("Warning: failed to create skill directory {:?}: {}", dir, e);
            continue;
        }

        let skill_path = dir.join("SKILL.md");
        if let Err(e) = fs::write(&skill_path, content) {
            eprintln!(
                "Warning: failed to write skill file {:?}: {}",
                skill_path, e
            );
        }
    }
}
