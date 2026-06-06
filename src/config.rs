//! Config file at `~/.config/mnml-aws-eventbridge.toml`. First
//! run writes the scaffold + exits with instructions.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default = "default_refresh")]
    pub refresh_interval_secs: u64,
    #[serde(default)]
    pub tabs: Vec<Tab>,
}

fn default_refresh() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tab {
    /// Human label shown in the tab strip.
    pub name: String,
    /// Tab kind: `buses` (all event buses) or `rules` (rules on a
    /// specific bus). Default = `buses`.
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Event bus name — only consulted when `kind = "rules"`.
    /// Use "default" for the account-wide default bus.
    #[serde(default)]
    pub event_bus_name: Option<String>,
    /// Optional region override for this tab.
    #[serde(default)]
    pub region: Option<String>,
}

fn default_kind() -> String {
    "buses".to_string()
}

impl Config {
    pub const EXAMPLE: &'static str = r##"# mnml-aws-eventbridge config. Edit and re-run.
#
# Optional top-level region (defers to AWS CLI when unset):
# region = "us-east-1"

refresh_interval_secs = 60

# ── Tabs ─────────────────────────────────────────────────────────
# Kinds:
#   "buses" — list every event bus in the region (default)
#   "rules" — list rules on `event_bus_name` (use "default" for the
#            account-wide default bus)

[[tabs]]
name = "Buses"
kind = "buses"

[[tabs]]
name = "Default rules"
kind = "rules"
event_bus_name = "default"

# Example custom-bus tab:
# [[tabs]]
# name = "Orders bus"
# kind = "rules"
# event_bus_name = "orders-events"
"##;

    pub fn validate(&self) -> Result<()> {
        if self.tabs.is_empty() {
            return Err(anyhow!("config: at least one [[tabs]] entry required"));
        }
        for (i, t) in self.tabs.iter().enumerate() {
            match t.kind.as_str() {
                "buses" => {}
                "rules" => {
                    if t.event_bus_name.as_deref().unwrap_or("").trim().is_empty() {
                        return Err(anyhow!(
                            "tab #{i} ({}): kind=\"rules\" requires `event_bus_name`",
                            t.name
                        ));
                    }
                }
                other => {
                    return Err(anyhow!(
                        "tab #{i} ({}): unknown kind {other:?} (expected \"buses\" or \"rules\")",
                        t.name
                    ));
                }
            }
        }
        Ok(())
    }
}

pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("mnml-aws-eventbridge.toml")
}

pub fn load() -> Result<Config> {
    let path = config_path();
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, Config::EXAMPLE)?;
        return Err(anyhow!(
            "wrote config template to {} — edit it then re-run",
            path.display()
        ));
    }
    let text = std::fs::read_to_string(&path)?;
    let cfg: Config = toml::from_str(&text)?;
    cfg.validate()?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_parses_and_validates() {
        let cfg: Config = toml::from_str(Config::EXAMPLE).expect("example parses");
        cfg.validate().expect("example validates");
        assert!(cfg.tabs.len() >= 2);
    }

    #[test]
    fn rejects_no_tabs() {
        let cfg = Config {
            region: None,
            refresh_interval_secs: 60,
            tabs: vec![],
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rejects_unknown_kind() {
        let cfg = Config {
            region: None,
            refresh_interval_secs: 60,
            tabs: vec![Tab {
                name: "bad".into(),
                kind: "bogus".into(),
                event_bus_name: None,
                region: None,
            }],
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rejects_rules_without_bus_name() {
        let cfg = Config {
            region: None,
            refresh_interval_secs: 60,
            tabs: vec![Tab {
                name: "no-bus".into(),
                kind: "rules".into(),
                event_bus_name: None,
                region: None,
            }],
        };
        assert!(cfg.validate().is_err());
    }
}
