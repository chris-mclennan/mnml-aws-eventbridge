//! `aws events list-event-buses` / `list-rules` shell-outs +
//! structured response models. Pure CLI — no SDK dep.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventBus {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Arn", default)]
    pub arn: String,
    #[serde(rename = "Policy", default)]
    pub policy: Option<String>,
    #[serde(rename = "CreationTime", default)]
    pub creation_time: Option<f64>,
    #[serde(rename = "LastModifiedTime", default)]
    pub last_modified_time: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ListBusesResponse {
    #[serde(rename = "EventBuses")]
    event_buses: Vec<EventBus>,
    #[serde(rename = "NextToken", default)]
    next_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Arn", default)]
    pub arn: String,
    #[serde(rename = "State", default)]
    pub state: Option<String>,
    #[serde(rename = "Description", default)]
    pub description: Option<String>,
    #[serde(rename = "EventPattern", default)]
    pub event_pattern: Option<String>,
    #[serde(rename = "ScheduleExpression", default)]
    pub schedule_expression: Option<String>,
    #[serde(rename = "EventBusName", default)]
    pub event_bus_name: Option<String>,
    #[serde(rename = "RoleArn", default)]
    pub role_arn: Option<String>,
    #[serde(rename = "ManagedBy", default)]
    pub managed_by: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListRulesResponse {
    #[serde(rename = "Rules")]
    rules: Vec<Rule>,
    #[serde(rename = "NextToken", default)]
    next_token: Option<String>,
}

/// Unified focused-item type so the items list + detail panel can
/// be shape-shared between `buses` and `rules` tabs.
#[derive(Debug, Clone)]
pub enum Item {
    Bus(EventBus),
    Rule(Rule),
}

impl Item {
    pub fn primary_label(&self) -> &str {
        match self {
            Item::Bus(b) => &b.name,
            Item::Rule(r) => &r.name,
        }
    }
    pub fn secondary_label(&self) -> String {
        match self {
            Item::Bus(_) => String::from("event bus"),
            Item::Rule(r) => match (&r.state, &r.schedule_expression) {
                (Some(s), Some(sched)) => format!("{s} · {sched}"),
                (Some(s), None) => s.clone(),
                (None, Some(sched)) => sched.clone(),
                _ => String::new(),
            },
        }
    }
    pub fn arn(&self) -> &str {
        match self {
            Item::Bus(b) => &b.arn,
            Item::Rule(r) => &r.arn,
        }
    }
}

pub fn list_event_buses(region: Option<&str>) -> Result<Vec<EventBus>> {
    let mut all = Vec::new();
    let mut token: Option<String> = None;

    loop {
        let mut cmd = Command::new("aws");
        cmd.args(["events", "list-event-buses", "--output", "json"]);
        if let Some(r) = region {
            cmd.args(["--region", r]);
        }
        if let Some(t) = &token {
            cmd.args(["--next-token", t]);
        }

        let output = cmd
            .output()
            .with_context(|| "spawn `aws events list-event-buses`")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "aws events list-event-buses failed: {}",
                stderr.trim()
            ));
        }

        let resp: ListBusesResponse = serde_json::from_slice(&output.stdout)
            .with_context(|| "parse list-event-buses JSON")?;
        all.extend(resp.event_buses);

        match resp.next_token {
            Some(t) if !t.is_empty() => token = Some(t),
            _ => break,
        }
    }

    all.sort_by_key(|b| b.name.to_lowercase());
    Ok(all)
}

pub fn list_rules(event_bus_name: &str, region: Option<&str>) -> Result<Vec<Rule>> {
    let mut all = Vec::new();
    let mut token: Option<String> = None;

    loop {
        let mut cmd = Command::new("aws");
        cmd.args([
            "events",
            "list-rules",
            "--event-bus-name",
            event_bus_name,
            "--output",
            "json",
        ]);
        if let Some(r) = region {
            cmd.args(["--region", r]);
        }
        if let Some(t) = &token {
            cmd.args(["--next-token", t]);
        }

        let output = cmd
            .output()
            .with_context(|| format!("spawn `aws events list-rules` for bus {event_bus_name}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "aws events list-rules failed for bus {event_bus_name}: {}",
                stderr.trim()
            ));
        }

        let resp: ListRulesResponse =
            serde_json::from_slice(&output.stdout).with_context(|| "parse list-rules JSON")?;
        all.extend(resp.rules);

        match resp.next_token {
            Some(t) if !t.is_empty() => token = Some(t),
            _ => break,
        }
    }

    all.sort_by_key(|r| r.name.to_lowercase());
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list_event_buses_response() {
        let json = r#"{
            "EventBuses": [
                {
                    "Name": "default",
                    "Arn": "arn:aws:events:us-east-1:1:event-bus/default"
                },
                {
                    "Name": "orders-events",
                    "Arn": "arn:aws:events:us-east-1:1:event-bus/orders-events"
                }
            ]
        }"#;
        let resp: ListBusesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.event_buses.len(), 2);
        assert_eq!(resp.event_buses[0].name, "default");
    }

    #[test]
    fn parses_list_rules_response() {
        let json = r#"{
            "Rules": [
                {
                    "Name": "daily-cleanup",
                    "Arn": "arn:aws:events:us-east-1:1:rule/default/daily-cleanup",
                    "State": "ENABLED",
                    "ScheduleExpression": "rate(1 day)"
                },
                {
                    "Name": "order-created",
                    "Arn": "arn:aws:events:us-east-1:1:rule/default/order-created",
                    "State": "ENABLED",
                    "EventPattern": "{\"source\":[\"orders\"]}"
                }
            ]
        }"#;
        let resp: ListRulesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.rules.len(), 2);
        assert_eq!(
            resp.rules[0].schedule_expression.as_deref(),
            Some("rate(1 day)")
        );
    }

    #[test]
    fn pagination_token_parsed() {
        let json = r#"{"Rules": [], "NextToken": "tok"}"#;
        let resp: ListRulesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.next_token.as_deref(), Some("tok"));
    }

    #[test]
    fn item_secondary_label_for_scheduled_rule() {
        let r = Rule {
            name: "x".into(),
            arn: "arn".into(),
            state: Some("ENABLED".into()),
            description: None,
            event_pattern: None,
            schedule_expression: Some("rate(1 hour)".into()),
            event_bus_name: None,
            role_arn: None,
            managed_by: None,
        };
        let label = Item::Rule(r).secondary_label();
        assert!(label.contains("ENABLED"));
        assert!(label.contains("rate(1 hour)"));
    }
}
