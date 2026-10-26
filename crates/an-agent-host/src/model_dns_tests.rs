//! DNS interruption exercises the synchronous boundary without public DNS.
#![allow(clippy::unwrap_used, clippy::disallowed_methods)]
use super::*;
use std::{sync::atomic::AtomicUsize, time::Instant};

pub(super) struct PendingDns(Arc<AtomicUsize>);
struct Active(Arc<AtomicUsize>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl reqwest::dns::Resolve for PendingDns {
    fn resolve(&self, _: reqwest::dns::Name) -> reqwest::dns::Resolving {
        self.0.fetch_add(1, Ordering::SeqCst);
        let active = Active(self.0.clone());
        Box::pin(async move {
            let _active = active;
            std::future::pending().await
        })
    }
}

#[test]
fn dns_cancellation_and_deadline_return_and_release_every_attempt() {
    let active = Arc::new(AtomicUsize::new(0));
    for cancel in [false, true, false, true] {
        let flag = Arc::new(AtomicBool::new(false));
        let control = ModelCallControl::new(flag.clone(), Duration::from_millis(150)).unwrap();
        let mut client = HttpModelClient::new()
            .unwrap()
            .with_control(control.clone());
        client.resolver = Some(Arc::new(PendingDns(active.clone())));
        let observed = active.clone();
        let worker = std::thread::spawn(move || {
            let start = Instant::now();
            while observed.load(Ordering::SeqCst) == 0 {
                assert!(start.elapsed() < Duration::from_secs(3));
                std::thread::sleep(Duration::from_millis(1));
            }
            if cancel {
                flag.store(true, Ordering::SeqCst);
            }
        });
        let start = Instant::now();
        let error = client
            .complete(
                &ModelSpec {
                    base_url: "http://pending.invalid".into(),
                    model: "test".into(),
                    extra_body: None,
                },
                vec![],
            )
            .unwrap_err();
        worker.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(
            control.status(),
            if cancel { "cancelled" } else { "timed_out" }
        );
        assert!(error.contains(if cancel { "cancelled" } else { "timed out" }));
    }
}
