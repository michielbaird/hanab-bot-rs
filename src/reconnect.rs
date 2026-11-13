//! Keeps a bot connected: runs a connection, and when it ends for any reason, a panic included, runs another.

use std::future::Future;
use std::time::{Duration, Instant};

/// How a connection ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ended {
    /// It is worth another try: the network, the server, or a bug of ours (a panic).
    Lost(String),
    /// It is not: the server refused the credentials, and trying again only gets the address blocked.
    Refused(String),
}

/// How long to wait between connections.
#[derive(Clone, Copy, Debug)]
pub struct Backoff {
    /// The wait after the first connection that failed.
    pub first: Duration,
    /// The longest wait.
    pub cap: Duration,
    /// A connection that lasted this long was a good one: the wait starts over after it.
    pub healthy: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            first: Duration::from_secs(2),
            cap: Duration::from_secs(300),
            healthy: Duration::from_secs(60),
        }
    }
}

impl Backoff {
    /// The wait after this many connections in a row that did not last (at least one).
    pub fn wait(&self, failures: u32) -> Duration {
        let doublings = failures.saturating_sub(1).min(20);
        self.first
            .checked_mul(1 << doublings)
            .map_or(self.cap, |wait| wait.min(self.cap))
    }

    /// How many connections in a row did not last, after one that lasted `lasted`.
    pub fn failures_after(&self, failures: u32, lasted: Duration) -> u32 {
        if lasted >= self.healthy {
            1
        } else {
            failures.saturating_add(1)
        }
    }
}

/// Runs connections one after another, for as long as they are worth another try. `connect` starts a connection (it is a
/// task of its own, so that a panic in it ends the connection and nothing else). Returns why the credentials were refused:
/// the only thing that ends it.
pub async fn keep_connected<F, Fut>(name: &str, backoff: Backoff, mut connect: F) -> String
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Ended> + Send + 'static,
{
    let mut failures = 0;
    loop {
        let started = Instant::now();
        let ended = match tokio::spawn(connect()).await {
            Ok(ended) => ended,
            Err(e) => Ended::Lost(format!("the bot's task failed: {e}")),
        };

        match ended {
            Ended::Refused(why) => return why,
            Ended::Lost(why) => {
                failures = backoff.failures_after(failures, started.elapsed());
                let wait = backoff.wait(failures);
                println!(
                    "[{name}] Connection lost: {why}. Trying again in {} s.",
                    wait.as_secs_f32()
                );
                tokio::time::sleep(wait).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    const QUICK: Backoff = Backoff {
        first: Duration::from_millis(1),
        cap: Duration::from_millis(4),
        healthy: Duration::from_secs(60),
    };

    /// Runs `keep_connected` over connections that end as `ends` says (a `None` panics), then refuse the credentials. Returns
    /// why it stopped, and how many connections it ran.
    async fn run(ends: Vec<Option<Ended>>) -> (String, u32) {
        let calls = Arc::new(AtomicU32::new(0));
        let counter = calls.clone();
        let why = keep_connected("test", QUICK, move || {
            let n = counter.fetch_add(1, Ordering::SeqCst) as usize;
            let end = ends.get(n).cloned();
            async move {
                match end {
                    Some(Some(ended)) => ended,
                    Some(None) => panic!("the bot panicked"),
                    None => Ended::Refused("stop".to_string()),
                }
            }
        })
        .await;
        (why, calls.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn a_connection_that_panics_is_run_again() {
        let (why, calls) = run(vec![None, None, Some(Ended::Lost("closed".into()))]).await;
        assert_eq!((why.as_str(), calls), ("stop", 4));
    }

    #[tokio::test]
    async fn it_does_not_give_up_on_a_connection_that_fails() {
        // (It used to stop after five.)
        let lost = || Some(Ended::Lost("no route".into()));
        let (why, calls) = run((0..12).map(|_| lost()).collect()).await;
        assert_eq!((why.as_str(), calls), ("stop", 13));
    }

    #[tokio::test]
    async fn refused_credentials_end_it_at_once() {
        let (why, calls) = run(vec![Some(Ended::Refused("wrong password".into()))]).await;
        assert_eq!((why.as_str(), calls), ("wrong password", 1));
    }

    #[test]
    fn the_wait_doubles_up_to_the_cap() {
        let backoff = Backoff::default();
        let waits: Vec<u64> = (1..=10).map(|n| backoff.wait(n).as_secs()).collect();
        assert_eq!(waits, [2, 4, 8, 16, 32, 64, 128, 256, 300, 300]);
        // ... however many there have been.
        assert_eq!(backoff.wait(u32::MAX), backoff.cap);
        assert_eq!(backoff.wait(0), backoff.first);
    }

    #[test]
    fn a_good_connection_starts_the_wait_over() {
        let backoff = Backoff::default();
        let short = Duration::from_secs(5);
        assert_eq!(backoff.failures_after(0, short), 1);
        assert_eq!(backoff.failures_after(4, short), 5);
        // One that lasted is a first failure again, not a fifth.
        assert_eq!(backoff.failures_after(4, backoff.healthy), 1);
    }
}
