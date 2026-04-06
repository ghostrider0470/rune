use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircuitBreakerSnapshot {
    pub failures: u32,
    pub is_open: bool,
}

#[derive(Debug, Clone)]
struct CircuitBreakerState {
    failures: u32,
    opened_at: Option<Instant>,
}

impl CircuitBreakerState {
    fn new() -> Self {
        Self {
            failures: 0,
            opened_at: None,
        }
    }

    #[cfg(test)]
    fn is_open(&self, now: Instant, cooldown: Duration) -> bool {
        self.opened_at
            .map(|opened_at| now.duration_since(opened_at) < cooldown)
            .unwrap_or(false)
    }
}

#[derive(Debug)]
pub struct CircuitBreakerRegistry {
    breakers: Mutex<HashMap<String, CircuitBreakerState>>,
    threshold: u32,
    cooldown: Duration,
}

impl CircuitBreakerRegistry {
    #[must_use]
    pub fn new(threshold: u32, cooldown: Duration) -> Self {
        Self {
            breakers: Mutex::new(HashMap::new()),
            threshold: threshold.max(1),
            cooldown,
        }
    }

    pub fn allow(&self, key: &str) -> Result<(), Duration> {
        let now = Instant::now();
        let mut breakers = self.breakers.lock().expect("circuit breakers poisoned");
        let state = breakers
            .entry(key.to_string())
            .or_insert_with(CircuitBreakerState::new);

        if let Some(opened_at) = state.opened_at {
            let elapsed = now.duration_since(opened_at);
            if elapsed < self.cooldown {
                return Err(self.cooldown - elapsed);
            }
        }

        if state.opened_at.is_some() {
            state.opened_at = None;
            state.failures = 0;
        }

        Ok(())
    }

    pub fn record_success(&self, key: &str) {
        let mut breakers = self.breakers.lock().expect("circuit breakers poisoned");
        let state = breakers
            .entry(key.to_string())
            .or_insert_with(CircuitBreakerState::new);
        state.failures = 0;
        state.opened_at = None;
    }

    pub fn record_retriable_failure(&self, key: &str) -> Option<u32> {
        let mut breakers = self.breakers.lock().expect("circuit breakers poisoned");
        let state = breakers
            .entry(key.to_string())
            .or_insert_with(CircuitBreakerState::new);
        state.failures = state.failures.saturating_add(1);
        if state.failures >= self.threshold {
            state.opened_at = Some(Instant::now());
            return Some(state.failures);
        }
        None
    }

    pub fn record_non_retriable_failure(&self, key: &str) {
        let mut breakers = self.breakers.lock().expect("circuit breakers poisoned");
        let state = breakers
            .entry(key.to_string())
            .or_insert_with(CircuitBreakerState::new);
        state.failures = 0;
        state.opened_at = None;
    }

    #[cfg(test)]
    pub fn snapshot(&self, key: &str) -> Option<CircuitBreakerSnapshot> {
        let now = Instant::now();
        let breakers = self.breakers.lock().expect("circuit breakers poisoned");
        breakers.get(key).map(|state| CircuitBreakerSnapshot {
            failures: state.failures,
            is_open: state.is_open(now, self.cooldown),
        })
    }
}
