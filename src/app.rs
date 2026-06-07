//! App state — per-tab list of EventBridge items (buses OR rules) +
//! a selection cursor. One tab is one filter view.

use crate::config::{Config, Tab};
use crate::eventbridge::{self, Item};
use anyhow::Result;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct TabSpec {
    pub kind: String,
    pub event_bus_name: Option<String>,
    pub region: Option<String>,
}

impl TabSpec {
    pub fn resolve(t: &Tab, default_region: Option<&str>) -> Result<Self> {
        let region = t
            .region
            .clone()
            .or_else(|| default_region.map(str::to_string));
        match t.kind.as_str() {
            "buses" => Ok(Self {
                kind: "buses".into(),
                event_bus_name: None,
                region,
            }),
            "rules" => {
                let bus = t.event_bus_name.clone().unwrap_or_default();
                if bus.trim().is_empty() {
                    anyhow::bail!("tab `{}`: kind=\"rules\" requires `event_bus_name`", t.name);
                }
                Ok(Self {
                    kind: "rules".into(),
                    event_bus_name: Some(bus),
                    region,
                })
            }
            other => anyhow::bail!("tab `{}`: unknown kind {other:?}", t.name),
        }
    }
}

pub struct ItemsTab {
    pub items: Vec<Item>,
    pub selected: usize,
    pub last_loaded: Option<Instant>,
    pub last_error: Option<String>,
    pub loading: bool,
}

impl ItemsTab {
    fn empty() -> Self {
        ItemsTab {
            items: Vec::new(),
            selected: 0,
            last_loaded: None,
            last_error: None,
            loading: false,
        }
    }
}

pub struct TabState {
    pub name: String,
    pub spec: TabSpec,
    pub data: ItemsTab,
}

pub struct App {
    pub cfg: Config,
    pub tabs: Vec<TabState>,
    pub active_tab: usize,
    pub status: String,
    /// Lazily-loaded targets for the focused rule. `Some((cache_key,
    /// targets))` where `cache_key` is `"<bus>::<rule>"`; `None`
    /// when no rule is focused or the lookup hasn't run yet for the
    /// current selection. Re-fetched on every cursor move so the
    /// detail panel always shows the right rule's targets.
    pub focused_targets: Option<(String, Vec<crate::eventbridge::Target>)>,
}

impl App {
    pub fn new(cfg: Config) -> Result<Self> {
        let mut tabs = Vec::with_capacity(cfg.tabs.len());
        for t in &cfg.tabs {
            let spec = TabSpec::resolve(t, cfg.region.as_deref())?;
            tabs.push(TabState {
                name: t.name.clone(),
                data: ItemsTab::empty(),
                spec,
            });
        }
        let mut app = App {
            cfg,
            tabs,
            active_tab: 0,
            status: String::new(),
            focused_targets: None,
        };
        app.refresh_active();
        app.ensure_focused_targets_loaded();
        Ok(app)
    }

    pub fn active(&self) -> &TabState {
        &self.tabs[self.active_tab]
    }
    pub fn active_mut(&mut self) -> &mut TabState {
        &mut self.tabs[self.active_tab]
    }

    pub fn switch_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.active_tab = idx;
            if self.tabs[idx].data.items.is_empty() && self.tabs[idx].data.last_error.is_none() {
                self.refresh_active();
            }
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        {
            let tab = self.active_mut();
            if tab.data.items.is_empty() {
                return;
            }
            let n = tab.data.items.len() as isize;
            let cur = tab.data.selected as isize;
            let next = (cur + delta).clamp(0, n - 1);
            tab.data.selected = next as usize;
        }
        // Selection moved → refresh targets if the new focus is a rule.
        self.ensure_focused_targets_loaded();
    }

    /// Reload targets for the focused rule if the cache key is stale.
    /// No-op when the focus is a bus (buses don't have targets) or
    /// when nothing's focused.
    pub fn ensure_focused_targets_loaded(&mut self) {
        let Some(item) = self.focused_item() else {
            self.focused_targets = None;
            return;
        };
        let crate::eventbridge::Item::Rule(rule) = item else {
            self.focused_targets = None;
            return;
        };
        let bus = rule
            .event_bus_name
            .clone()
            .unwrap_or_else(|| "default".into());
        let rule_name = rule.name.clone();
        let cache_key = format!("{bus}::{rule_name}");
        if let Some((k, _)) = &self.focused_targets
            && k == &cache_key
        {
            return;
        }
        let region = self.active().spec.region.clone();
        match crate::eventbridge::list_targets_by_rule(&rule_name, &bus, region.as_deref()) {
            Ok(targets) => {
                self.focused_targets = Some((cache_key, targets));
            }
            Err(e) => {
                self.status = format!("targets: {e}");
                self.focused_targets = Some((cache_key, vec![]));
            }
        }
    }

    pub fn refresh_active(&mut self) {
        let idx = self.active_tab;
        let spec = self.tabs[idx].spec.clone();
        let name = self.tabs[idx].name.clone();
        self.status = format!("loading {name}…");
        self.tabs[idx].data.loading = true;

        let result: Result<Vec<Item>> = match spec.kind.as_str() {
            "buses" => eventbridge::list_event_buses(spec.region.as_deref())
                .map(|bs| bs.into_iter().map(Item::Bus).collect()),
            "rules" => {
                let bus = spec
                    .event_bus_name
                    .as_deref()
                    .expect("rules tab requires event_bus_name (validated)");
                eventbridge::list_rules(bus, spec.region.as_deref())
                    .map(|rs| rs.into_iter().map(Item::Rule).collect())
            }
            _ => unreachable!("validated in TabSpec::resolve"),
        };

        let t = &mut self.tabs[idx];
        t.data.loading = false;
        match result {
            Ok(items) => {
                let count = items.len();
                t.data.items = items;
                t.data.selected = t.data.selected.min(count.saturating_sub(1));
                t.data.last_loaded = Some(Instant::now());
                t.data.last_error = None;
                let kind_label = match spec.kind.as_str() {
                    "buses" => "buses",
                    "rules" => "rules",
                    _ => "items",
                };
                self.status = format!("{name}: {count} {kind_label}");
            }
            Err(e) => {
                t.data.last_error = Some(e.to_string());
                self.status = format!("error: {e}");
            }
        }
    }

    pub fn tick(&mut self) -> bool {
        let interval = self.cfg.refresh_interval_secs;
        if interval == 0 {
            return false;
        }
        let idx = self.active_tab;
        let stale = match self.tabs[idx].data.last_loaded {
            Some(t) => t.elapsed().as_secs() >= interval,
            None => true,
        };
        if stale && !self.tabs[idx].data.loading {
            self.refresh_active();
            true
        } else {
            false
        }
    }

    pub fn drain(&mut self) -> bool {
        false
    }

    pub fn focused_item(&self) -> Option<&Item> {
        let t = self.active();
        t.data.items.get(t.data.selected)
    }

    /// `o` — open console URL for the focused item.
    pub fn open_console(&mut self) {
        let Some(item) = self.focused_item() else {
            self.status = "no item under cursor".into();
            return;
        };
        let region = self.active().spec.region.as_deref().unwrap_or("us-east-1");
        let url = match item {
            Item::Bus(b) => format!(
                "https://{region}.console.aws.amazon.com/events/home?region={region}#/eventbus/{}",
                b.name
            ),
            Item::Rule(r) => {
                let bus = r.event_bus_name.as_deref().unwrap_or("default");
                format!(
                    "https://{region}.console.aws.amazon.com/events/home?region={region}#/eventbus/{bus}/rules/{}",
                    r.name
                )
            }
        };
        match webbrowser::open(&url) {
            Ok(()) => self.status = format!("opened {url}"),
            Err(e) => self.status = format!("open failed: {e}"),
        }
    }

    /// `y` — yank focused item's ARN to clipboard.
    pub fn yank_arn(&mut self) {
        let Some(item) = self.focused_item() else {
            self.status = "no item under cursor".into();
            return;
        };
        let arn = item.arn().to_string();
        match crate::clipboard::copy(&arn) {
            Ok(()) => self.status = format!("copied ARN ({} chars)", arn.len()),
            Err(e) => self.status = format!("copy failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Tab;

    #[test]
    fn tab_spec_resolve_uses_default_region() {
        let t = Tab {
            name: "x".into(),
            kind: "buses".into(),
            event_bus_name: None,
            region: None,
        };
        let spec = TabSpec::resolve(&t, Some("us-west-2")).unwrap();
        assert_eq!(spec.region.as_deref(), Some("us-west-2"));
    }

    #[test]
    fn tab_spec_rejects_rules_without_bus() {
        let t = Tab {
            name: "bad".into(),
            kind: "rules".into(),
            event_bus_name: None,
            region: None,
        };
        assert!(TabSpec::resolve(&t, None).is_err());
    }

    #[test]
    fn tab_spec_rejects_unknown_kind() {
        let t = Tab {
            name: "bad".into(),
            kind: "garbage".into(),
            event_bus_name: None,
            region: None,
        };
        assert!(TabSpec::resolve(&t, None).is_err());
    }
}
