//! Prompts
//!
//! Interactive terminal prompts for the setup wizard.
//! Uses the `dialoguer` crate for input handling.

use anyhow::Result;
use colored::Colorize;
use dialoguer::Input;
use regex::Regex;

/// Prompt the user for a required string value.
/// Repeats until a non-empty value is entered.
pub fn prompt_required(label: &str) -> Result<String> {
    loop {
        let value: String = Input::new()
            .with_prompt(format!("  {} {}", "\u{2192}".cyan(), label.white()))
            .allow_empty(true)
            .interact_text()?;

        let trimmed = value.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
        println!("{}", "  This field is required.".yellow());
    }
}

/// Prompt the user for multiline text input.
/// The user types their prompt and presses Enter twice to finish.
pub fn prompt_multiline(label: &str) -> Result<String> {
    println!();
    println!("  {}", label.white());
    println!(
        "{}",
        "  Type your prompt, then press Enter twice to finish:".dimmed()
    );
    println!();

    let mut lines: Vec<String> = Vec::new();
    let mut last_was_empty = false;

    loop {
        let line: String = Input::new()
            .with_prompt("  ")
            .allow_empty(true)
            .interact_text()?;

        if line.is_empty() && last_was_empty && !lines.is_empty() {
            // Remove the trailing empty line we added
            lines.pop();
            break;
        }

        if line.is_empty() && !lines.is_empty() {
            last_was_empty = true;
            lines.push(String::new());
        } else {
            last_was_empty = false;
            lines.push(line);
        }
    }

    let result = lines.join("\n").trim().to_string();
    if result.is_empty() {
        println!("{}", "  Genesis prompt is required. Try again.".yellow());
        return prompt_multiline(label);
    }

    Ok(result)
}

/// Prompt the user for an Ethereum address with validation.
/// Must be 0x followed by 40 hex characters.
pub fn prompt_address(label: &str) -> Result<String> {
    let re = Regex::new(r"^0x[0-9a-fA-F]{40}$")?;

    loop {
        let value: String = Input::new()
            .with_prompt(format!("  {} {}", "\u{2192}".cyan(), label.white()))
            .allow_empty(true)
            .interact_text()?;

        let trimmed = value.trim().to_string();
        if re.is_match(&trimmed) {
            return Ok(trimmed);
        }
        println!(
            "{}",
            "  Invalid Ethereum address. Must be 0x followed by 40 hex characters.".yellow()
        );
    }
}
