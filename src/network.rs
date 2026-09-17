//! All app-owned HTTP traffic uses this cache. A request takes a client snapshot;
//! changes affect the next request without interrupting requests already in flight.
use crate::config::schema::ProxyMode;
use anyhow::Result;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Provider,
    Auth,
    Updater,
}

// Never derive Debug: environment variables may contain proxy credentials.
#[derive(Default, PartialEq, Eq)]
struct Snapshot {
    environment: Vec<Option<std::ffi::OsString>>,
    system: Vec<Option<Vec<u8>>>,
}

impl Snapshot {
    fn read(mode: ProxyMode) -> Self {
        if mode == ProxyMode::Direct {
            return Self::default();
        }
        Self {
            environment: [
                "HTTP_PROXY",
                "http_proxy",
                "HTTPS_PROXY",
                "https_proxy",
                "ALL_PROXY",
                "all_proxy",
                "NO_PROXY",
                "no_proxy",
                "REQUEST_METHOD",
            ]
            .iter()
            .map(std::env::var_os)
            .collect(),
            system: system_snapshot(),
        }
    }
}

#[cfg(windows)]
fn system_snapshot() -> Vec<Option<Vec<u8>>> {
    use windows::core::w;
    use windows::Win32::System::Registry::*;
    // Read the same current-user Internet Settings values as reqwest. These
    // bytes are only a change detector; reqwest remains the proxy parser.
    let mut result = Vec::new();
    for name in [w!("ProxyEnable"), w!("ProxyServer"), w!("ProxyOverride")] {
        let mut size = 0u32;
        let key = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings");
        unsafe {
            let status = RegGetValueW(
                HKEY_CURRENT_USER,
                key,
                name,
                RRF_RT_ANY,
                None,
                None,
                Some(&mut size),
            );
            if status.is_err() {
                result.push(None);
                continue;
            }
            let mut bytes = vec![0u8; size as usize];
            let status = RegGetValueW(
                HKEY_CURRENT_USER,
                key,
                name,
                RRF_RT_ANY,
                None,
                Some(bytes.as_mut_ptr().cast()),
                Some(&mut size),
            );
            result.push(status.is_ok().then(|| {
                bytes.truncate(size as usize);
                bytes
            }));
        }
    }
    result
}

#[cfg(not(windows))]
fn system_snapshot() -> Vec<Option<Vec<u8>>> {
    Vec::new()
}

#[derive(Default)]
struct Cache {
    mode: ProxyMode,
    snapshot: Option<Snapshot>,
    clients: Vec<(Profile, reqwest::Client)>,
}

impl Cache {
    fn refresh(&mut self, snapshot: Snapshot) {
        if self.snapshot.as_ref() != Some(&snapshot) {
            self.clients.clear();
            self.snapshot = Some(snapshot);
        }
    }
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

pub fn set_mode(mode: ProxyMode) {
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    if cache.mode != mode {
        cache.mode = mode;
        cache.clients.clear();
        cache.snapshot = None;
    }
}

pub fn client(profile: Profile) -> Result<reqwest::Client> {
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    let snapshot = Snapshot::read(cache.mode);
    cache.refresh(snapshot);
    if let Some((_, client)) = cache.clients.iter().find(|(p, _)| *p == profile) {
        return Ok(client.clone());
    }
    let client = build_client(profile, cache.mode)?;
    cache.clients.push((profile, client.clone()));
    Ok(client)
}

fn build_client(profile: Profile, mode: ProxyMode) -> Result<reqwest::Client> {
    let seconds = match profile {
        Profile::Provider => 10,
        Profile::Auth => 15,
        Profile::Updater => 120,
    };
    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(seconds));
    if profile == Profile::Updater {
        builder = builder.user_agent(concat!("ailimits/", env!("CARGO_PKG_VERSION")));
    } else {
        builder = builder.redirect(reqwest::redirect::Policy::none());
    }
    if mode == ProxyMode::Direct {
        builder = builder.no_proxy();
    }
    // Do not include underlying builder diagnostics, which can contain a
    // credential-bearing proxy URL, in user-visible errors or logs.
    builder
        .build()
        .map_err(|_| anyhow::anyhow!("could not build HTTP client"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_changed_system_settings_invalidate_cached_clients() {
        let snapshot = |enabled: u8, port: &str, bypass: &str| Snapshot {
            environment: Vec::new(),
            system: vec![
                Some(vec![enabled]),
                Some(port.as_bytes().to_vec()),
                Some(bypass.as_bytes().to_vec()),
            ],
        };
        let mut cache = Cache::default();
        cache.refresh(snapshot(1, "127.0.0.1:7890", "<local>"));
        cache.clients.push((
            Profile::Provider,
            build_client(Profile::Provider, ProxyMode::Direct).unwrap(),
        ));
        cache.refresh(snapshot(1, "127.0.0.1:7890", "<local>"));
        assert_eq!(
            cache.clients.len(),
            1,
            "unchanged settings preserve the pool"
        );
        cache.refresh(snapshot(1, "127.0.0.1:7891", "<local>"));
        assert!(cache.clients.is_empty());
        cache.clients.push((
            Profile::Auth,
            build_client(Profile::Auth, ProxyMode::Direct).unwrap(),
        ));
        cache.refresh(snapshot(0, "127.0.0.1:7891", "<local>"));
        assert!(cache.clients.is_empty());
        cache.clients.push((
            Profile::Updater,
            build_client(Profile::Updater, ProxyMode::Direct).unwrap(),
        ));
        cache.refresh(snapshot(0, "127.0.0.1:7891", "*.example.invalid"));
        assert!(cache.clients.is_empty());
    }
}
