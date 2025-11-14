use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing::{debug, info};
use serde_json::Value as JsonValue;

/// Activation state for a rule
#[derive(Debug, Clone)]
struct RuleActivation {
    /// Whether the rule is enabled
    enabled: bool,
    /// Optional expiration time for temporary activations
    expiration: Option<Instant>,
    /// Context data passed from the triggering rule
    context: HashMap<String, JsonValue>,
}

/// Manages the enabled/disabled state of rules and their expiration timers
pub struct RuleStateManager {
    /// Current activation state of each rule (by name)
    activations: Arc<RwLock<HashMap<String, RuleActivation>>>,
}

impl RuleStateManager {
    /// Create a new state manager with initial rule states
    pub fn new(initial_states: HashMap<String, bool>) -> Self {
        let activations = initial_states
            .into_iter()
            .map(|(name, enabled)| {
                (
                    name,
                    RuleActivation {
                        enabled,
                        expiration: None,
                        context: HashMap::new(),
                    },
                )
            })
            .collect();

        Self {
            activations: Arc::new(RwLock::new(activations)),
        }
    }

    /// Check if a rule is currently enabled
    pub fn is_rule_enabled(&self, rule_name: &str) -> bool {
        let activations = self.activations.read().unwrap();
        activations
            .get(rule_name)
            .map(|a| a.enabled)
            .unwrap_or(true)
    }

    /// Get the trigger context for a rule (if activated with context)
    pub fn get_trigger_context(&self, rule_name: &str) -> HashMap<String, JsonValue> {
        let activations = self.activations.read().unwrap();
        activations
            .get(rule_name)
            .map(|a| a.context.clone())
            .unwrap_or_default()
    }

    /// Activate a rule for a specific duration with optional context data
    pub fn activate_rule(&self, rule_name: &str, duration: Duration, context: HashMap<String, JsonValue>) {
        let expiration = Instant::now() + duration;

        self.activations.write().unwrap().insert(
            rule_name.to_string(),
            RuleActivation {
                enabled: true,
                expiration: Some(expiration),
                context,
            },
        );

        info!("Activated rule '{}' for {} seconds", rule_name, duration.as_secs());
    }

    /// Deactivate a rule immediately
    pub fn deactivate_rule(&self, rule_name: &str) {
        self.activations.write().unwrap().insert(
            rule_name.to_string(),
            RuleActivation {
                enabled: false,
                expiration: None,
                context: HashMap::new(),
            },
        );

        info!("Deactivated rule '{}'", rule_name);
    }

    /// Clean up expired rule activations
    pub fn cleanup_expired(&self) {
        let now = Instant::now();
        let mut activations = self.activations.write().unwrap();

        for (rule_name, activation) in activations.iter_mut() {
            if let Some(expiration) = activation.expiration {
                if now >= expiration {
                    debug!("Rule '{}' activation expired", rule_name);
                    activation.enabled = false;
                    activation.expiration = None;
                    activation.context.clear();
                }
            }
        }
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
