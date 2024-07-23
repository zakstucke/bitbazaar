use std::sync::Arc;

use chrono::Duration;

use futures::Future;

#[macro_export]
/// Internal version of [`Retry::call`] that:
/// - doesn't require an Fn(), meaning mutable references etc are easier.
macro_rules! retry_flexi {
    ($retry:expr, $fallible:block) => {{
        let mut iterator = $retry.backoff.into_iter();
        let mut current_try = 1;
        let mut total_delay = chrono::Duration::default();

        let zero = chrono::Duration::default();

        let result = loop {
            match $fallible {
                Ok(r) => break Ok(r),
                Err(e) => {
                    if let Some(next_delay) = iterator.next() {
                        if $retry.until.has_end_reached(&current_try, &total_delay, &next_delay) {
                            break Err(e);
                        }
                        total_delay += next_delay;

                        if total_delay == zero && $retry.until.is_delay_length_based() {
                            panic!("TotalDelay/Delay being used with 0 size delays, infinite loop.");
                        }

                        if let Some(on_retry) = &$retry.on_retry {
                            // Call the on_retry function, if it returns Some(E) then return that error, exiting early.
                            if let Some(e) = on_retry($crate::misc::RetryInfo {
                                last_error: e,
                                last_attempt_no: current_try,
                                delay_till_next_attempt: next_delay,
                            }) {
                                break Err(e);
                            }
                        }

                        $crate::misc::sleep_compat(next_delay).await;
                        current_try += 1;
                    } else {
                        break Err(e);
                    }
                }
            }
        };

        result
    }};
}

/// The retry builder.
pub struct Retry<'a, E> {
    /// UNSTABLE" ONLY PUBLIC FOR MACRO USE.
    /// Using the 'a to avoid needing everything to be static.
    pub on_retry: Option<Arc<Box<dyn Fn(RetryInfo<E>) -> Option<E> + 'a>>>,
    /// UNSTABLE" ONLY PUBLIC FOR MACRO USE.
    pub until: RetryUntil,
    /// UNSTABLE" ONLY PUBLIC FOR MACRO USE.
    pub backoff: Backoff,
}

// E is only used in the type signature.
unsafe impl<'a, E> Send for Retry<'a, E> {}

impl<'a, E> Clone for Retry<'a, E> {
    fn clone(&self) -> Self {
        Self {
            on_retry: self.on_retry.clone(),
            until: self.until.clone(),
            backoff: self.backoff.clone(),
        }
    }
}

impl<'a, E> Retry<'a, E> {
    /// Stop retrying after the given number of attempts.
    pub fn until_total_attempts(mut self, attempts: usize) -> Self {
        self.until = RetryUntil::TotalAttempts(attempts);
        self
    }

    /// Stop retrying after the total delay reaches the given duration.
    pub fn until_total_delay(mut self, max_total_delay: Duration) -> Self {
        self.until = RetryUntil::TotalDelay(max_total_delay);
        self
    }

    /// Stop retrying when the next delay is greater than the given duration.
    pub fn until_delay(mut self, max_delay: Duration) -> Self {
        self.until = RetryUntil::Delay(max_delay);
        self
    }

    /// Will be executed each time the fallible function fails and a new one is pending.
    /// If clear won't succeed, return Some(E) to indicate should raise with this error and not continue retrying.
    pub fn on_retry(mut self, on_retry: impl Fn(RetryInfo<E>) -> Option<E> + 'a) -> Self {
        self.on_retry = Some(Arc::new(Box::new(on_retry)));
        self
    }

    /// Will run and retry as required.
    ///
    /// Note use underlying macro [`retry_flexi`] for better compiler support.
    ///
    /// # Arguments
    /// * `fallible` - The async function that will be executed.
    pub async fn call<R, Fut: Future<Output = Result<R, E>>>(
        self,
        fallible: impl Fn() -> Fut,
    ) -> Result<R, E> {
        retry_flexi!(self, { fallible().await })
    }
}

impl<'a, E> Retry<'a, E> {
    /// No delay between retries.
    pub fn no_delay() -> Self {
        Self {
            on_retry: None,
            until: RetryUntil::TotalAttempts(1),
            backoff: Backoff::Fixed {
                duration: Duration::default(),
            },
        }
    }

    /// EXPONENTIAL:
    /// Each retry increases the delay since the last exponentially.
    ///
    /// Creates a new exponential backoff using the given millisecond duration as the initial delay and
    /// the given exponential backoff factor (e.g. factor of 2.0 is just squared).
    pub fn exponential(initial_delay: Duration, factor: f64) -> Self {
        let base = initial_delay.num_milliseconds();
        Self {
            on_retry: None,
            until: RetryUntil::TotalAttempts(1),
            backoff: Backoff::Exponential {
                current: base,
                factor,
            },
        }
    }

    /// FIBONACCI:
    /// Each retry uses a delay which is the sum of the two previous delays.
    ///
    /// Depending on the problem at hand, a fibonacci delay strategy might perform better and lead to
    /// better throughput than the [`Exponential`] strategy.
    ///
    /// See ["A Performance Comparison of Different Backoff Algorithms under Different Rebroadcast
    /// Probabilities for MANETs"](https://www.researchgate.net/publication/255672213_A_Performance_Comparison_of_Different_Backoff_Algorithms_under_Different_Rebroadcast_Probabilities_for_MANET's)
    /// for more details.
    ///
    /// Create a new fibonacci backoff using the given duration in milliseconds.
    pub fn fibonacci(initial_delay: Duration) -> Self {
        let millis = initial_delay.num_milliseconds();
        Self {
            on_retry: None,
            until: RetryUntil::TotalAttempts(1),
            backoff: Backoff::Fibonacci {
                curr: millis,
                next: millis,
            },
        }
    }

    /// FIXED:
    /// Each retry uses a fixed delay.
    ///
    /// Create a new fixed backoff using the given duration in milliseconds.
    pub fn fixed(fixed_delay: Duration) -> Self {
        Self {
            on_retry: None,
            until: RetryUntil::TotalAttempts(1),
            backoff: Backoff::Fixed {
                duration: fixed_delay,
            },
        }
    }
}

/// UNSTABLE: ONLY PUBLIC FOR MACRO USE.
#[derive(Debug, Clone)]
pub enum RetryUntil {
    /// UNSTABLE: ONLY PUBLIC FOR MACRO USE.
    TotalAttempts(usize),
    /// UNSTABLE: ONLY PUBLIC FOR MACRO USE.
    TotalDelay(Duration),
    /// UNSTABLE: ONLY PUBLIC FOR MACRO USE.
    Delay(Duration),
}

impl RetryUntil {
    /// UNSTABLE: ONLY PUBLIC FOR MACRO USE.
    pub fn is_delay_length_based(&self) -> bool {
        matches!(self, RetryUntil::TotalDelay(_) | RetryUntil::Delay(_))
    }
    /// UNSTABLE: ONLY PUBLIC FOR MACRO USE.
    pub fn has_end_reached(
        &self,
        current_try: &usize,
        total_delay: &Duration,
        next_delay: &Duration,
    ) -> bool {
        match &self {
            RetryUntil::TotalAttempts(max_attempts) => {
                if current_try >= max_attempts {
                    return true;
                }
            }
            RetryUntil::TotalDelay(max_total_delay) => {
                if total_delay > max_total_delay {
                    return true;
                }
            }
            RetryUntil::Delay(max_delay) => {
                if next_delay > max_delay {
                    return true;
                }
            }
        }
        false
    }
}

/// UNSTABLE: ONLY PUBLIC FOR MACRO USE.
#[derive(Debug, Clone)]
pub enum Backoff {
    /// UNSTABLE: ONLY PUBLIC FOR MACRO USE.
    Exponential {
        /// UNSTABLE: ONLY PUBLIC FOR MACRO USE.
        current: i64,
        /// UNSTABLE: ONLY PUBLIC FOR MACRO USE.
        factor: f64,
    },
    /// UNSTABLE: ONLY PUBLIC FOR MACRO USE.
    Fixed {
        /// UNSTABLE: ONLY PUBLIC FOR MACRO USE.
        duration: Duration,
    },
    /// UNSTABLE: ONLY PUBLIC FOR MACRO USE.
    Fibonacci {
        /// UNSTABLE: ONLY PUBLIC FOR MACRO USE.
        curr: i64,
        /// UNSTABLE: ONLY PUBLIC FOR MACRO USE.
        next: i64,
    },
}

impl Iterator for Backoff {
    type Item = Duration;

    fn next(&mut self) -> Option<Duration> {
        match self {
            Backoff::Exponential { current, factor } => {
                let duration = Duration::milliseconds(*current);

                let next = (*current as f64) * *factor;
                *current = if next > (i64::MAX as f64) {
                    i64::MAX
                } else {
                    next as _
                };

                Some(duration)
            }
            Backoff::Fibonacci { curr, next } => {
                let duration = Duration::milliseconds(*curr);

                if let Some(next_next) = curr.checked_add(*next) {
                    *curr = *next;
                    *next = next_next;
                } else {
                    *curr = *next;
                    *next = i64::MAX;
                }

                Some(duration)
            }
            Backoff::Fixed { duration } => Some(*duration),
        }
    }
}

/// Information about the last retry attempt.
pub struct RetryInfo<E> {
    /// The error that caused the last attempt to fail.
    pub last_error: E,
    /// The number of the last attempt. E.g. first attempt failing would be 1.
    pub last_attempt_no: usize,
    /// The delay until the next attempt.
    pub delay_till_next_attempt: Duration,
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicI32;
    use std::sync::atomic::Ordering;

    use rstest::*;

    use super::*;

    use crate::chrono::chrono_format_td;
    use crate::prelude::*;

    // TODO using now in here and dlock, should be some test utils we can use cross crate.
    macro_rules! assert_td_in_range {
        ($td:expr, $range:expr) => {
            assert!(
                $td >= $range.start && $td <= $range.end,
                "Expected '{}' to be in range '{}' - '{}'.",
                chrono_format_td($td, true),
                chrono_format_td($range.start, true),
                chrono_format_td($range.end, true),
            );
        };
    }

    #[rstest]
    #[tokio::test]
    async fn test_retry_until_types() -> RResult<(), AnyErr> {
        // Attempt:
        let calls = AtomicI32::new(0);
        let _ = Retry::<Report<AnyErr>>::no_delay()
            .until_total_attempts(10)
            .call(|| async {
                calls.fetch_add(1, Ordering::Relaxed);
                Err::<(), _>(anyerr!("Foo"))
            })
            .await;
        assert_eq!(calls.load(Ordering::Relaxed), 10);

        // TotalDelay:
        let before = chrono::Utc::now();
        let _ = Retry::<Report<AnyErr>>::fixed(Duration::milliseconds(30))
            .until_total_delay(Duration::milliseconds(100))
            .call(|| async { Err::<(), _>(anyerr!("Foo")) })
            .await;
        assert_td_in_range!(
            chrono::Utc::now() - before,
            Duration::milliseconds(100)..Duration::milliseconds(130)
        );

        // Delay:
        // this should go 3, 9, 81
        let before = chrono::Utc::now();
        let _ = Retry::<Report<AnyErr>>::exponential(Duration::milliseconds(3), 2.0)
            .until_delay(Duration::milliseconds(80))
            .call(|| async { Err::<(), _>(anyerr!("Foo")) })
            .await;
        assert_td_in_range!(
            chrono::Utc::now() - before,
            Duration::milliseconds(93)..Duration::milliseconds(110)
        );

        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_retry_normal() -> RResult<(), AnyErr> {
        // Will only call once (succeeds):
        let calls = AtomicI32::new(0);
        let out = Retry::<Report<AnyErr>>::no_delay()
            .until_total_attempts(10)
            .call(|| async {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(0)
            })
            .await?;
        assert_eq!(out, 0);
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        // Will call twice (first fails):
        let calls = AtomicI32::new(0);
        let out = Retry::<Report<AnyErr>>::no_delay()
            .until_total_attempts(10)
            .call(|| async {
                calls.fetch_add(1, Ordering::Relaxed);
                if calls.load(Ordering::Relaxed) == 1 {
                    Err(anyerr!("Foo"))
                } else {
                    Ok(0)
                }
            })
            .await?;
        assert_eq!(out, 0);
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        Ok(())
    }

    // Using the underlying to allow for mutable references and avoidance of closures.
    #[rstest]
    #[tokio::test]
    async fn test_retry_mutable_ref() -> RResult<(), AnyErr> {
        // Will only call once (succeeds):
        let mut calls_src = 0;
        let calls = &mut calls_src;
        let out = retry_flexi!(
            Retry::<Report<AnyErr>>::no_delay().until_total_attempts(10),
            {
                *calls += 1;
                Ok(0)
            }
        )?;
        assert_eq!(out, 0);
        assert_eq!(*calls, 1);

        // Will call twice (first fails):
        let mut calls_src = 0;
        let calls = &mut calls_src;
        let out = retry_flexi!(
            Retry::<Report<AnyErr>>::no_delay().until_total_attempts(10),
            {
                *calls += 1;
                if *calls == 1 {
                    Err(anyerr!("Foo"))
                } else {
                    Ok(0)
                }
            }
        )?;
        assert_eq!(out, 0);
        assert_eq!(*calls, 2);
        Ok(())
    }
}
