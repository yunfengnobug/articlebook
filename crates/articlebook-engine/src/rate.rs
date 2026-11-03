use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Serializes the start time of outbound requests.
pub struct Pace {
    delay: Duration,
    next: Mutex<Instant>,
}

impl Pace {
    pub fn new(delay: Duration) -> Self {
        Self {
            delay,
            next: Mutex::new(Instant::now()),
        }
    }

    pub async fn wait(&self) {
        if self.delay.is_zero() {
            return;
        }
        loop {
            let delay = {
                let mut next = self.next.lock().expect("pace lock");
                let now = Instant::now();
                if *next <= now {
                    *next = now + self.delay;
                    return;
                }
                *next - now
            };
            sleep(delay).await;
        }
    }
}
