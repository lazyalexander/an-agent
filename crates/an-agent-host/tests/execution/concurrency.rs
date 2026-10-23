//! Product host calls in one multithreaded process, with competing forks.
//! No test-process isolation, retries, or test-only publisher implementation.
use super::*;
use std::{
    os::unix::process::CommandExt,
    sync::{
        Barrier,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};

#[test]
fn concurrent_embedded_hosts_publish_and_execute_while_other_threads_fork() -> Result<()> {
    const WORKERS: usize = 3;
    const RUNS: usize = 12;
    let fixture = Fixture::new()?;
    let start = Barrier::new(WORKERS + 1);
    let stop = AtomicBool::new(false);
    let forks = AtomicUsize::new(0);
    struct Stop<'a>(&'a AtomicBool);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    thread::scope(|scope| -> Result<()> {
        // Inside scope: always stop the fork worker before implicit joins,
        // including early errors and unwinds in a host worker.
        let stop_guard = Stop(&stop);
        let sibling = scope.spawn(|| -> std::result::Result<(), String> {
            while !stop.load(Ordering::SeqCst) {
                let mut command = Command::new("/usr/bin/true");
                // SAFETY: raw nanosleep only after fork. This deliberately
                // widens descriptor-inheritance races; it is fault injection,
                // never a product sleep, retry or launch synchronization.
                unsafe {
                    command.pre_exec(|| {
                        let delay = libc::timespec {
                            tv_sec: 0,
                            tv_nsec: 20_000_000,
                        };
                        libc::nanosleep(&delay, std::ptr::null_mut());
                        Ok(())
                    });
                }
                let status = command.status().map_err(|error| error.to_string())?;
                if !status.success() {
                    return Err("fork competitor failed".into());
                }
                forks.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        });
        let mut workers = vec![];
        for worker in 0..WORKERS {
            let fixture = &fixture;
            let start = &start;
            workers.push(scope.spawn(move || -> std::result::Result<(), String> {
                start.wait();
                (|| -> Result<()> {
                    let store = fixture.root.join(format!("concurrent-store-{worker}"));
                    let input = fixture.root.join(format!("input-{worker}.txt"));
                    let release = execution::register(&fixture.deployment, &store)?["registered"]
                        .as_str()
                        .ok_or("missing registered identity")?
                        .to_owned();
                    for run in 0..RUNS {
                        let expected = format!("worker {worker} run {run}");
                        fs::write(&input, expected.as_bytes())?;
                        let result = execution::run(&store, &release, &input, true)?;
                        let receipt_path =
                            Path::new(result["receipt"].as_str().ok_or("missing receipt")?);
                        let receipt: Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
                        if result["status"] != "succeeded" {
                            return Err(format!(
                                "concurrent host failed: {receipt}; stderr: {}",
                                fs::read_to_string(
                                    receipt_path.parent().unwrap().join("stderr.txt")
                                )?
                            )
                            .into());
                        }
                        assert_eq!(receipt["publication"]["method"], "separate-process-copy-v1");
                        assert_eq!(receipt["publication"]["published"], true);
                        assert_eq!(receipt["publication"]["helper"]["reaped"], true);
                        assert_eq!(receipt["publication"]["helper"]["exit_code"], 0);
                        assert_eq!(receipt["publication"]["retry_attempted"], false);
                        assert_eq!(receipt["process"]["reaped"], true);
                        assert_ne!(
                            receipt["publication"]["helper"]["pid"],
                            receipt["process"]["pid"]
                        );
                        assert_eq!(
                            fs::read(result["stdout"].as_str().unwrap())?,
                            expected.as_bytes()
                        );
                        assert!(!store.join("host.lock").exists());
                    }
                    assert_eq!(fs::read_dir(store.join("runs"))?.count(), RUNS);
                    Ok(())
                })()
                .map_err(|error| error.to_string())
            }));
        }
        start.wait();
        let results: Vec<_> = workers.into_iter().map(|worker| worker.join()).collect();
        drop(stop_guard);
        sibling
            .join()
            .map_err(|_| "fork worker panicked")?
            .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
        for result in results {
            result
                .map_err(|_| "host worker panicked")?
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
        }
        assert!(
            forks.load(Ordering::SeqCst) > 0,
            "no competing fork was exercised"
        );
        Ok(())
    })
}
