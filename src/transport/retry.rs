use std::time::Duration;

use tracing::warn;

/// Configuration for automatic retry with exponential backoff.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    pub max_retries: u32,
    /// Initial delay before first retry.
    pub initial_delay: Duration,
    /// Maximum delay between retries.
    pub max_delay: Duration,
    /// Multiplier applied to delay after each retry.
    pub backoff_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            backoff_factor: 2.0,
        }
    }
}

impl RetryConfig {
    /// Calculate the delay for a given attempt number (0-indexed).
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let delay_ms = self.initial_delay.as_millis() as f64
            * self.backoff_factor.powi(attempt as i32);
        let delay = Duration::from_millis(delay_ms as u64);
        delay.min(self.max_delay)
    }

    /// Whether a given HTTP status code should trigger a retry.
    pub fn should_retry_status(&self, status: u16) -> bool {
        matches!(status, 429 | 500 | 502 | 503 | 504)
    }
}

/// Sleep for the retry delay, respecting a Retry-After header if present.
pub async fn retry_delay(config: &RetryConfig, attempt: u32, retry_after: Option<u64>) {
    let delay = if let Some(secs) = retry_after {
        Duration::from_secs(secs)
    } else {
        config.delay_for_attempt(attempt)
    };

    warn!(
        attempt = attempt + 1,
        max = config.max_retries,
        delay_ms = delay.as_millis() as u64,
        "retrying after delay"
    );

    tokio::time::sleep(delay).await;
}

/// Simple token-bucket rate limiter.
#[derive(Debug)]
pub struct RateLimiter {
    max_per_second: f64,
    state: tokio::sync::Mutex<RateLimiterState>,
}

#[derive(Debug)]
struct RateLimiterState {
    available: f64,
    last_check: tokio::time::Instant,
}

impl RateLimiter {
    /// Create a new rate limiter allowing `max_per_second` requests per second.
    pub fn new(max_per_second: u32) -> Self {
        Self {
            max_per_second: max_per_second as f64,
            state: tokio::sync::Mutex::new(RateLimiterState {
                available: max_per_second as f64,
                last_check: tokio::time::Instant::now(),
            }),
        }
    }

    /// Wait until a request token is available.
    pub async fn acquire(&self) {
        loop {
            {
                let mut state = self.state.lock().await;
                let now = tokio::time::Instant::now();
                let elapsed = now.duration_since(state.last_check).as_secs_f64();
                state.available =
                    (state.available + elapsed * self.max_per_second).min(self.max_per_second);
                state.last_check = now;

                if state.available >= 1.0 {
                    state.available -= 1.0;
                    return;
                }
            }
            // Wait a fraction of the refill interval.
            tokio::time::sleep(Duration::from_millis(
                (1000.0 / self.max_per_second) as u64,
            ))
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.backoff_factor, 2.0);
    }

    #[test]
    fn test_delay_calculation() {
        let config = RetryConfig {
            initial_delay: Duration::from_millis(100),
            backoff_factor: 2.0,
            max_delay: Duration::from_secs(10),
            ..Default::default()
        };
        assert_eq!(config.delay_for_attempt(0), Duration::from_millis(100));
        assert_eq!(config.delay_for_attempt(1), Duration::from_millis(200));
        assert_eq!(config.delay_for_attempt(2), Duration::from_millis(400));
    }

    #[test]
    fn test_delay_caps_at_max() {
        let config = RetryConfig {
            initial_delay: Duration::from_secs(1),
            backoff_factor: 10.0,
            max_delay: Duration::from_secs(5),
            ..Default::default()
        };
        // 1 * 10^3 = 1000s, capped at 5s
        assert_eq!(config.delay_for_attempt(3), Duration::from_secs(5));
    }

    #[test]
    fn test_should_retry_status() {
        let config = RetryConfig::default();
        assert!(config.should_retry_status(429));
        assert!(config.should_retry_status(503));
        assert!(!config.should_retry_status(200));
        assert!(!config.should_retry_status(400));
        assert!(!config.should_retry_status(404));
    }
}
