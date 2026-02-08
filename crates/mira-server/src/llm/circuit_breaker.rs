// crates/mira-server/src/llm/circuit_breaker.rs
// Circuit breaker for LLM providers — tracks failures and temporarily excludes
// providers that are down or rate-limited.

use crate::llm::provider::Provider;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// How many failures within the tracking window before we trip the circuit.
const FAILURE_THRESHOLD: u32 = 3;

/// Window in which failures are counted. Failures older than this are ignored.
const FAILURE_WINDOW: Duration = Duration::from_secs(5 * 60); // 5 minutes

/// How long a tripped circuit stays open before allowing a single probe request.
const COOLDOWN: Duration = Duration::from_secs(2 * 60); // 2 minutes

/// Circuit state for a single provider.
#[derive(Debug, Clone)]
enum State {
    /// Normal operation — tracking recent failures.
    Closed { failures: Vec<Instant> },
    /// Tripped — all requests are rejected until cooldown expires.
    Open { tripped_at: Instant },
    /// Cooldown expired — allow exactly one probe request.
    HalfOpen,
}

impl Default for State {
    fn default() -> Self {
        Self::Closed {
            failures: Vec::new(),
        }
    }
}

/// Thread-safe circuit breaker that tracks per-provider health.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    states: Arc<Mutex<HashMap<Provider, State>>>,
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self {
            states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check whether a provider is currently available.
    ///
    /// Returns `true` if the circuit is Closed or transitions to HalfOpen
    /// (allowing a single probe). Returns `false` if the circuit is Open
    /// and cooldown has not yet elapsed.
    pub fn is_available(&self, provider: Provider) -> bool {
        let Ok(mut states) = self.states.lock() else {
            return true; // If mutex is poisoned, allow the request
        };
        let state = states.entry(provider).or_default();

        match state {
            State::Closed { .. } => true,
            State::Open { tripped_at } => {
                if tripped_at.elapsed() >= COOLDOWN {
                    info!(provider = %provider, "Circuit half-open, allowing probe request");
                    *state = State::HalfOpen;
                    true
                } else {
                    false
                }
            }
            State::HalfOpen => {
                // Already half-open — a probe is in flight. Block additional
                // callers until the probe resolves.
                false
            }
        }
    }

    /// Record a successful request — resets the circuit to Closed.
    pub fn record_success(&self, provider: Provider) {
        let Ok(mut states) = self.states.lock() else {
            return;
        };
        let state = states.entry(provider).or_default();

        let was_half_open = matches!(state, State::HalfOpen);
        *state = State::Closed {
            failures: Vec::new(),
        };

        if was_half_open {
            info!(provider = %provider, "Circuit recovered (half-open probe succeeded)");
        }
    }

    /// Record a failed request — may trip the circuit.
    pub fn record_failure(&self, provider: Provider) {
        let Ok(mut states) = self.states.lock() else {
            return;
        };
        let state = states.entry(provider).or_default();
        let now = Instant::now();

        match state {
            State::Closed { failures } => {
                failures.push(now);
                // Evict failures outside the window
                failures.retain(|t| now.duration_since(*t) < FAILURE_WINDOW);

                if failures.len() as u32 >= FAILURE_THRESHOLD {
                    warn!(
                        provider = %provider,
                        failures = failures.len(),
                        "Circuit tripped — provider will be skipped for {}s",
                        COOLDOWN.as_secs()
                    );
                    *state = State::Open { tripped_at: now };
                }
            }
            State::HalfOpen => {
                // Probe failed — re-trip immediately.
                warn!(provider = %provider, "Half-open probe failed — circuit re-tripped");
                *state = State::Open { tripped_at: now };
            }
            State::Open { .. } => {
                // Already open; nothing to do.
            }
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_provider_is_available() {
        let cb = CircuitBreaker::new();
        assert!(cb.is_available(Provider::DeepSeek));
    }

    #[test]
    fn test_single_failure_does_not_trip() {
        let cb = CircuitBreaker::new();
        cb.record_failure(Provider::DeepSeek);
        assert!(cb.is_available(Provider::DeepSeek));
    }

    #[test]
    fn test_threshold_failures_trips_circuit() {
        let cb = CircuitBreaker::new();
        for _ in 0..FAILURE_THRESHOLD {
            cb.record_failure(Provider::DeepSeek);
        }
        assert!(!cb.is_available(Provider::DeepSeek));
    }

    #[test]
    fn test_success_resets_failures() {
        let cb = CircuitBreaker::new();
        cb.record_failure(Provider::DeepSeek);
        cb.record_failure(Provider::DeepSeek);
        cb.record_success(Provider::DeepSeek);
        // After success, counter resets — one more failure should not trip
        cb.record_failure(Provider::DeepSeek);
        assert!(cb.is_available(Provider::DeepSeek));
    }

    #[test]
    fn test_independent_providers() {
        let cb = CircuitBreaker::new();
        for _ in 0..FAILURE_THRESHOLD {
            cb.record_failure(Provider::DeepSeek);
        }
        // DeepSeek tripped, but Zhipu should be fine
        assert!(!cb.is_available(Provider::DeepSeek));
        assert!(cb.is_available(Provider::Zhipu));
    }

    #[test]
    fn test_open_circuit_transitions_to_half_open_after_cooldown() {
        let cb = CircuitBreaker::new();

        // Manually inject an Open state with a tripped_at in the past
        {
            let mut states = cb.states.lock().unwrap();
            states.insert(
                Provider::DeepSeek,
                State::Open {
                    tripped_at: Instant::now() - COOLDOWN - Duration::from_secs(1),
                },
            );
        }

        // Should transition to HalfOpen and return true
        assert!(cb.is_available(Provider::DeepSeek));
    }

    #[test]
    fn test_half_open_success_closes_circuit() {
        let cb = CircuitBreaker::new();

        {
            let mut states = cb.states.lock().unwrap();
            states.insert(Provider::DeepSeek, State::HalfOpen);
        }

        cb.record_success(Provider::DeepSeek);
        assert!(cb.is_available(Provider::DeepSeek));
    }

    #[test]
    fn test_half_open_failure_retrips_circuit() {
        let cb = CircuitBreaker::new();

        {
            let mut states = cb.states.lock().unwrap();
            states.insert(Provider::DeepSeek, State::HalfOpen);
        }

        cb.record_failure(Provider::DeepSeek);
        assert!(!cb.is_available(Provider::DeepSeek));
    }
}
