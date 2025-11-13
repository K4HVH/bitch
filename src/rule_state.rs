use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Manages the enabled/disabled state of rules and their expiration timers
pub struct RuleStateManager {
    /// Current enabled state of each rule (by name)
    enabled_rules: Arc<RwLock<HashMap<String, bool>>>,
    /// Expiration times for temporarily activated rules
    expiration_timers: Arc<RwLock<HashMap<String, Instant>>>,
}

impl RuleStateManager {
    /// Create a new state manager with initial rule states
    pub fn new(initial_states: HashMap<String, bool>) -> Self {
        Self {
            enabled_rules: Arc::new(RwLock::new(initial_states)),
            expiration_timers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if a rule is currently enabled
    pub fn is_rule_enabled(&self, rule_name: &str) -> bool {
        let enabled = self.enabled_rules.read().unwrap();
        enabled.get(rule_name).copied().unwrap_or(true)
    }

    /// Activate a rule for a specific duration
    pub fn activate_rule(&self, rule_name: &str, duration: Duration) {
        let expiration = Instant::now() + duration;

        {
            let mut enabled = self.enabled_rules.write().unwrap();
            enabled.insert(rule_name.to_string(), true);
        }

        {
            let mut timers = self.expiration_timers.write().unwrap();
            timers.insert(rule_name.to_string(), expiration);
        }

        info!(
            "Activated rule '{}' for {} seconds",
            rule_name,
            duration.as_secs()
        );
    }

    /// Deactivate a rule immediately
    pub fn deactivate_rule(&self, rule_name: &str) {
        {
            let mut enabled = self.enabled_rules.write().unwrap();
            enabled.insert(rule_name.to_string(), false);
        }

        {
            let mut timers = self.expiration_timers.write().unwrap();
            timers.remove(rule_name);
        }

        info!("Deactivated rule '{}'", rule_name);
    }

    /// Clean up expired rule activations
    pub fn cleanup_expired(&self) {
        let now = Instant::now();
        let mut expired_rules = Vec::new();

        // Find expired rules
        {
            let timers = self.expiration_timers.read().unwrap();
            for (rule_name, expiration) in timers.iter() {
                if now >= *expiration {
                    expired_rules.push(rule_name.clone());
                }
            }
        }

        // Deactivate expired rules
        if !expired_rules.is_empty() {
            let mut enabled = self.enabled_rules.write().unwrap();
            let mut timers = self.expiration_timers.write().unwrap();

            for rule_name in expired_rules {
                debug!("Rule '{}' activation expired", rule_name);
                enabled.insert(rule_name.clone(), false);
                timers.remove(&rule_name);
            }
        }
    }

    /// Get a list of currently enabled rules (for debugging)
    pub fn get_enabled_rules(&self) -> Vec<String> {
        let enabled = self.enabled_rules.read().unwrap();
        enabled
            .iter()
            .filter(|(_, &is_enabled)| is_enabled)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Spawn a background task to periodically clean up expired rules
    pub fn spawn_cleanup_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                self.cleanup_expired();
            }
        });
    }
}
