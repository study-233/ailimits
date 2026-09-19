// Regression: the background saver must persist the LAST config after a rapid
// burst, and its shutdown handshake must leave the newest config on disk.
// This guards against independently spawned saves completing out of order.
use ailimits::config::{schema::Config, storage};

fn config_with_opacity(opacity: f32) -> Config {
    let mut c = Config::default();
    c.window.opacity = opacity;
    c
}

#[test]
fn saver_persists_the_last_config_under_a_burst() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let path = std::env::temp_dir().join(format!("ailimits-saver-{}.toml", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let saver = storage::spawn_saver(&tokio::runtime::Handle::current(), path.clone());
        let handle = saver.handle();

        // A rapid burst of distinct configs, all in [0.001, 0.100]. `Final`
        // below uses 0.500 -- a value the burst never produces -- so this
        // test can only pass if `Final` itself was written, not merely the
        // burst's own last `Write`.
        for i in 1..=100u32 {
            handle.save(config_with_opacity(i as f32 / 1000.0));
        }
        // The shutdown handshake drains the channel and waits for the writer.
        saver.shutdown(
            config_with_opacity(0.500),
            &tokio::runtime::Handle::current(),
        );

        let saved: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(
            (saved.window.opacity - 0.500).abs() < 1e-6,
            "the saver did not persist the final config: opacity = {}",
            saved.window.opacity
        );
    });
}

#[test]
fn shutdown_waits_for_the_write_so_no_later_write_can_clobber_it() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let path =
            std::env::temp_dir().join(format!("ailimits-shutdown-{}.toml", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let saver = storage::spawn_saver(&tokio::runtime::Handle::current(), path.clone());
        let handle = saver.handle();

        // Queue an older value, then shut down with the newest one. When
        // shutdown returns, the newest value must ALREADY be on disk — no
        // background write may still be in flight behind it.
        handle.save(config_with_opacity(0.010));
        saver.shutdown(
            config_with_opacity(0.900),
            &tokio::runtime::Handle::current(),
        );

        let saved: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        // Sending on a closed channel must not panic or resurrect the writer.
        handle.save(config_with_opacity(0.010));
        let after: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(
            (saved.window.opacity - 0.900).abs() < 1e-6,
            "shutdown returned before the final config was on disk: opacity = {}",
            saved.window.opacity
        );
        assert!(
            (after.window.opacity - 0.900).abs() < 1e-6,
            "a post-shutdown save reached the file: opacity = {}",
            after.window.opacity
        );
    });
}

// The app's real call site (`Event::LoopDestroyed` in the tao event loop) is
// a plain OS thread with NO Tokio context active -- never inside
// `rt.block_on`. That is exactly the path a naive `rt.block_on(...)` inside
// `shutdown` would panic on ("Cannot start a runtime from within a runtime"
// or "there is no reactor running", depending on where the nested call
// landed). The other tests in this file call `shutdown` from inside
// `rt.block_on(async { .. })`, which does not exercise this call site at
// all -- so this test exists to pin it down: if `shutdown` regresses to a
// bare `rt.block_on`, this test (not the app in production) is what catches
// it.
#[test]
fn shutdown_works_from_a_thread_with_no_runtime_context() {
    let path = std::env::temp_dir().join(format!(
        "ailimits-shutdown-nocontext-{}.toml",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);

    let rt = tokio::runtime::Runtime::new().unwrap();
    let saver = storage::spawn_saver(rt.handle(), path.clone());
    let handle = saver.handle();
    handle.save(config_with_opacity(0.020));

    // No `rt.block_on` wrapping this call -- this thread has never entered
    // the runtime.
    saver.shutdown(config_with_opacity(0.600), rt.handle());

    let saved: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        (saved.window.opacity - 0.600).abs() < 1e-6,
        "shutdown from a thread with no runtime context did not persist the final config: opacity = {}",
        saved.window.opacity
    );
}

// `ConfigSaverHandle` is `Clone + Send + 'static`, so nothing stops another
// thread from calling `save` at the exact moment `shutdown` is sending
// `Final`. If a `Write` could land in the slot after `Final` but before the
// writer task picks it up, the task would process that `Write`, loop back
// for more instead of breaking, and the `JoinHandle` `shutdown` awaits would
// never resolve -- so the file could end up with a stale value, or
// `shutdown` could burn its full timeout for nothing. Fire a burst of
// concurrent `save` calls racing the `Final` send; the final config on disk
// must always be the one passed to `shutdown`, never one of the racing
// `Write`s.
#[test]
fn save_cannot_overwrite_a_final_already_queued() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let path =
            std::env::temp_dir().join(format!("ailimits-final-race-{}.toml", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let saver = storage::spawn_saver(&tokio::runtime::Handle::current(), path.clone());
        let racer = saver.handle();

        let racer_thread = std::thread::spawn(move || {
            for _ in 0..500 {
                racer.save(config_with_opacity(0.010));
            }
        });
        saver.shutdown(
            config_with_opacity(0.750),
            &tokio::runtime::Handle::current(),
        );
        racer_thread.join().unwrap();

        let saved: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(
            (saved.window.opacity - 0.750).abs() < 1e-6,
            "a concurrent save clobbered the final config: opacity = {}",
            saved.window.opacity
        );
    });
}

#[test]
fn snapshot_saver_coalesces_updates_and_waits_for_the_final_write() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let path = std::env::temp_dir().join(format!(
        "ailimits-snapshot-order-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let destination = path.clone();
    let saver = storage::spawn_snapshot_saver(rt.handle(), "test snapshot", move |value: u64| {
        let destination = destination.clone();
        let started_tx = started_tx.clone();
        async move {
            // Keep each write in flight until the test releases it. This
            // exposes overlapping writes without relying on timer sleeps.
            let (release_tx, release_rx) = tokio::sync::oneshot::channel();
            started_tx.send((value, release_tx)).unwrap();
            release_rx.await.unwrap();
            storage::atomic_write(&destination, &serde_json::to_vec(&value)?).await?;
            Ok(())
        }
    });
    let handle = saver.handle();
    let receive = || {
        started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
    };

    handle.save(1);
    let (first, release_first) = receive();
    assert_eq!(first, 1);
    for value in 2..=100 {
        handle.save(value);
    }
    assert!(started_rx.try_recv().is_err(), "writes must be serial");
    release_first.send(()).unwrap();
    let (latest, release_latest) = receive();
    assert_eq!(latest, 100, "pending snapshots should coalesce");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "1");

    let runtime_handle = rt.handle().clone();
    let shutdown = std::thread::spawn(move || saver.shutdown(999, &runtime_handle));
    release_latest.send(()).unwrap();
    let (final_value, release_final) = receive();
    assert_eq!(final_value, 999);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "100");
    assert!(
        !shutdown.is_finished(),
        "shutdown must await its final write"
    );
    handle.save(2); // A stale producer cannot replace Final, even during its write.
    release_final.send(()).unwrap();
    shutdown.join().unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "999");
    assert!(matches!(
        started_rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    ));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn final_empty_snapshot_deletes_after_an_older_write() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let path = std::env::temp_dir().join(format!(
        "ailimits-snapshot-delete-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let mut release = Some(release_rx);
    let destination = path.clone();
    let saver =
        storage::spawn_snapshot_saver(rt.handle(), "test cache", move |snapshot: Option<u64>| {
            let destination = destination.clone();
            let started_tx = started_tx.clone();
            let release = release.take();
            async move {
                if let Some(release) = release {
                    started_tx.send(()).unwrap();
                    release.await.unwrap();
                }
                if let Some(value) = snapshot {
                    storage::atomic_write(&destination, &serde_json::to_vec(&value)?).await?;
                } else {
                    tokio::fs::remove_file(&destination).await?;
                }
                Ok(())
            }
        });
    let handle = saver.handle();
    handle.save(Some(1));
    started_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let runtime_handle = rt.handle().clone();
    let shutdown = std::thread::spawn(move || saver.shutdown(None, &runtime_handle));
    release_tx.send(()).unwrap();
    shutdown.join().unwrap();
    assert!(
        !path.exists(),
        "an old write must not resurrect an empty cache"
    );
}

#[test]
fn history_shutdown_persists_the_final_accounts_without_changing_the_schema() {
    use ailimits::meter::history::{self, AccountHistory, History};
    use std::collections::BTreeMap;

    let rt = tokio::runtime::Runtime::new().unwrap();
    let path = std::env::temp_dir().join(format!(
        "ailimits-history-final-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let saver = history::saver_to(rt.handle(), path.clone());
    let handle = saver.handle();
    handle.save(History::default());

    let today = chrono::Utc::now().date_naive();
    let mut final_history = History::default();
    final_history.accounts.insert(
        "first-account".into(),
        AccountHistory {
            quota: vec![],
            tokens: BTreeMap::from([(today, 123)]),
        },
    );
    final_history.accounts.insert(
        "second-account".into(),
        AccountHistory {
            quota: vec![],
            tokens: BTreeMap::from([(today, 456)]),
        },
    );
    saver.shutdown(final_history.clone(), rt.handle());
    let saved: History = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.accounts, final_history.accounts);
    handle.save(History::default());
    let after: History = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(after.accounts, final_history.accounts);
    std::fs::remove_file(path).unwrap();
}
