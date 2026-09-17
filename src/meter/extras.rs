//! Bounded, on-demand App Server reads. Never starts a model turn.
use super::identity;
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Clone, PartialEq)]
pub enum Availability<T> {
    NotLoaded,
    Loading,
    Ready(T),
    Unavailable(&'static str),
}
#[derive(Debug, Clone, PartialEq)]
pub struct Credit {
    pub granted: Option<DateTime<Utc>>,
    pub expires: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Credits {
    pub count: u64,
    pub items: Option<Vec<Credit>>,
}
impl Credit {
    pub fn remaining(&self, now: DateTime<Utc>) -> Option<f32> {
        let duration = (self.expires? - self.granted?).num_seconds();
        super::time_remaining(self.expires, u64::try_from(duration).ok(), now)
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub account: Option<String>,
    pub credits: Availability<Credits>,
    pub tokens: Availability<BTreeMap<NaiveDate, u64>>,
    pub credits_at: Option<Instant>,
    pub tokens_at: Option<Instant>,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            account: None,
            credits: Availability::NotLoaded,
            tokens: Availability::NotLoaded,
            credits_at: None,
            tokens_at: None,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Request {
    pub credits: bool,
    pub tokens: bool,
}
impl Request {
    pub fn any(self) -> bool {
        self.credits || self.tokens
    }
}
impl Snapshot {
    pub fn needed(&self, enabled: Request, force: bool) -> Request {
        let old = |at: Option<Instant>| {
            force || at.is_none_or(|t| t.elapsed() >= Duration::from_secs(300))
        };
        Request {
            credits: enabled.credits && old(self.credits_at),
            tokens: enabled.tokens && old(self.tokens_at),
        }
    }
    pub fn merge(&mut self, incoming: Self) {
        if self.account != incoming.account {
            *self = incoming;
            return;
        }
        if incoming.credits_at.is_some() {
            self.credits = incoming.credits;
            self.credits_at = incoming.credits_at;
        }
        if incoming.tokens_at.is_some() {
            self.tokens = incoming.tokens;
            self.tokens_at = incoming.tokens_at;
        }
    }
}
pub fn parse_credits(v: &Value) -> Availability<Credits> {
    let c = &v["rateLimitResetCredits"];
    let Some(count) = c["availableCount"].as_u64() else {
        return Availability::Unavailable("Reset opportunities unavailable");
    };
    let items = c["credits"].as_array().map(|rows| {
        rows.iter()
            .filter(|r| r["status"] == "available")
            .map(|r| Credit {
                granted: r["grantedAt"]
                    .as_i64()
                    .and_then(|s| DateTime::from_timestamp(s, 0)),
                expires: r["expiresAt"]
                    .as_i64()
                    .and_then(|s| DateTime::from_timestamp(s, 0)),
            })
            .collect()
    });
    Availability::Ready(Credits { count, items })
}
pub fn parse_tokens(v: &Value) -> Availability<BTreeMap<NaiveDate, u64>> {
    let Some(rows) = v["dailyUsageBuckets"].as_array() else {
        return Availability::Unavailable("Daily token data unavailable");
    };
    let mut days = BTreeMap::new();
    for r in rows {
        let Some((date, n)) = r["startDate"]
            .as_str()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .zip(r["tokens"].as_u64())
        else {
            return Availability::Unavailable("Daily token data incomplete");
        };
        days.insert(date, n);
    }
    Availability::Ready(days)
}

/// A process-wide gate also covers repeated manual clicks while a read is in flight.
#[derive(Default)]
pub struct Gate(std::sync::Arc<std::sync::atomic::AtomicBool>);
pub struct Permit(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl Gate {
    pub fn enter(&self) -> Option<Permit> {
        use std::sync::atomic::Ordering;
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .ok()
            .map(|_| Permit(self.0.clone()))
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}

struct Client {
    input: tokio::process::ChildStdin,
    output: BufReader<tokio::process::ChildStdout>,
    id: u64,
}
impl Client {
    async fn send(&mut self, v: Value) -> anyhow::Result<()> {
        let mut bytes = serde_json::to_vec(&v)?;
        bytes.push(b'\n');
        self.input.write_all(&bytes).await?;
        self.input.flush().await?;
        Ok(())
    }
    async fn call(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.id += 1;
        let id = self.id;
        self.send(json!({"id":id,"method":method,"params":params}))
            .await?;
        loop {
            let mut line = Vec::new();
            let n = (&mut self.output)
                .take(1024 * 1024 + 1)
                .read_until(b'\n', &mut line)
                .await?;
            anyhow::ensure!(n > 0 && n <= 1024 * 1024, "invalid App Server frame");
            let v: Value = serde_json::from_slice(&line)?;
            // No server-initiated action is authorized by this read-only client.
            if v.get("method").is_some() && v.get("id").is_some() {
                self.send(
                    json!({"id":v["id"],"error":{"code":-32601,"message":"Read-only client"}}),
                )
                .await?;
                continue;
            }
            if v["id"].as_u64() == Some(id) {
                anyhow::ensure!(v.get("error").is_none(), "App Server method unavailable");
                return Ok(v["result"].clone());
            }
        }
    }
}

#[cfg(windows)]
struct ProcessJob(windows::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for ProcessJob {}
#[cfg(windows)]
impl ProcessJob {
    /// Assign while suspended, before any helper can create a child process.
    fn attach(child: &tokio::process::Child) -> anyhow::Result<Self> {
        use windows::Win32::{
            Foundation::*,
            System::{Diagnostics::ToolHelp::*, JobObjects::*, Threading::*},
        };
        unsafe {
            let job = Self(CreateJobObjectW(None, None)?);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as _,
                std::mem::size_of_val(&info) as u32,
            )?;
            let pid = child.id().ok_or_else(|| anyhow::anyhow!("helper exited"))?;
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid)?;
            let assigned = AssignProcessToJobObject(job.0, process);
            let _ = CloseHandle(process);
            assigned?;
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)?;
            let mut entry = THREADENTRY32 {
                dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            let mut found = false;
            if Thread32First(snapshot, &mut entry).is_ok() {
                loop {
                    if entry.th32OwnerProcessID == pid {
                        if let Ok(thread) =
                            OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID)
                        {
                            found = ResumeThread(thread) != u32::MAX;
                            let _ = CloseHandle(thread);
                        }
                        break;
                    }
                    if Thread32Next(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snapshot);
            anyhow::ensure!(found, "cannot resume helper");
            Ok(job)
        }
    }
}
#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

fn executable() -> Option<std::path::PathBuf> {
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let p = dir.join(if cfg!(windows) { "codex.exe" } else { "codex" });
        if p.is_file() {
            return Some(p);
        }
    }
    // Codex Desktop does not always export its bundled CLI on the user's PATH.
    let root = dirs::data_local_dir()?.join("OpenAI/Codex/bin");
    let mut versions: Vec<_> = std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path().join("codex.exe"))
        .filter(|p| p.is_file())
        .collect();
    versions.sort_by_key(|p| p.metadata().and_then(|m| m.modified()).ok());
    versions.pop()
}
async fn read_inner(expected: &str, request: Request, result: &mut Snapshot) -> anyhow::Result<()> {
    let exe = executable().ok_or_else(|| anyhow::anyhow!("Codex is not installed"))?;
    let local =
        identity::local_identity().ok_or_else(|| anyhow::anyhow!("Account cannot be matched"))?;
    anyhow::ensure!(local.key == expected, "Account cannot be matched");
    let mut command = tokio::process::Command::new(exe);
    command
        .args([
            "app-server",
            "--listen",
            "stdio://",
            "-c",
            "cli_auth_credentials_store=\"file\"",
        ])
        .env("CODEX_HOME", identity::codex_home())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000 | 0x00000004); // NO_WINDOW | SUSPENDED
    let mut child = command.spawn()?;
    #[cfg(windows)]
    let job = match ProcessJob::attach(&child) {
        Ok(j) => j,
        Err(e) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(e);
        }
    };
    let work = async {
        let mut client = Client {
            input: child
                .stdin
                .take()
                .ok_or_else(|| anyhow::anyhow!("stdin unavailable"))?,
            output: BufReader::new(
                child
                    .stdout
                    .take()
                    .ok_or_else(|| anyhow::anyhow!("stdout unavailable"))?,
            ),
            id: 0,
        };
        client.call("initialize",json!({"clientInfo":{"name":"quotabar","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}})).await?;
        client.send(json!({"method":"initialized"})).await?;
        let account = client
            .call("account/read", json!({"refreshToken":false}))
            .await?;
        anyhow::ensure!(
            identity::matches_account(expected, &local, &account["account"]),
            "Account cannot be matched"
        );
        if request.credits {
            result.credits = match client.call("account/rateLimits/read", Value::Null).await {
                Ok(v) => {
                    if let Some(account) = v["accountId"].as_str() {
                        anyhow::ensure!(
                            local.account_id.as_deref() == Some(account),
                            "Account cannot be matched"
                        );
                    }
                    parse_credits(&v)
                }
                Err(_) => Availability::Unavailable("Reset opportunities unavailable"),
            };
        }
        if request.tokens {
            result.tokens = match client.call("account/usage/read", Value::Null).await {
                Ok(v) => parse_tokens(&v),
                Err(_) => Availability::Unavailable("Daily token data unavailable"),
            };
        }
        let after = identity::local_identity()
            .ok_or_else(|| anyhow::anyhow!("Account cannot be matched"))?;
        let account = client
            .call("account/read", json!({"refreshToken":false}))
            .await?;
        anyhow::ensure!(
            identity::matches_account(expected, &after, &account["account"]),
            "Account cannot be matched"
        );
        Ok::<_, anyhow::Error>(())
    };
    let status = tokio::time::timeout(Duration::from_secs(25), work).await;
    // Closing the job kills descendants too, even if the parent already exited.
    #[cfg(windows)]
    drop(job);
    let _ = child.kill().await;
    let _ = child.wait().await;
    status.map_err(|_| anyhow::anyhow!("Codex read timed out"))?
}
pub async fn read(expected: Option<String>, request: Request) -> Snapshot {
    let mut result = Snapshot {
        account: expected.clone(),
        ..Default::default()
    };
    let status = match expected {
        Some(key) => read_inner(&key, request, &mut result).await,
        None => Err(anyhow::anyhow!(if executable().is_none() {
            "Codex is not installed"
        } else {
            "Account cannot be matched"
        })),
    };
    if let Err(e) = status {
        let reason = match e.to_string().as_str() {
            "Account cannot be matched" => "Account cannot be matched",
            "Codex is not installed" => "Codex is not installed",
            "Codex read timed out" => "Codex read timed out",
            _ => "Codex extension unavailable",
        };
        if request.credits {
            result.credits = Availability::Unavailable(reason);
        }
        if request.tokens {
            result.tokens = Availability::Unavailable(reason);
        }
    }
    if request.credits {
        result.credits_at = Some(Instant::now());
    }
    if request.tokens {
        result.tokens_at = Some(Instant::now());
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[tokio::test]
    async fn protocol_timeout_reaps_the_entire_process_tree() {
        use windows::Win32::{Foundation::*, System::Threading::*};
        // An isolated protocol fixture. No Codex credentials or network requests.
        let script = r#"
$fixtureChild = Start-Process -FilePath $env:ComSpec -ArgumentList '/d /c ping -n 60 127.0.0.1 >nul' -WindowStyle Hidden -PassThru
while ($null -ne ($line = [Console]::In.ReadLine())) {
  $request = $line | ConvertFrom-Json
  if ($request.method -eq 'hang') { Start-Sleep -Seconds 60 }
  if ($request.method -eq 'probe') { @{id=$request.id; result=@{childPid=$fixtureChild.Id}} | ConvertTo-Json -Compress }
}
"#;
        let mut command = tokio::process::Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .creation_flags(0x08000000 | 0x00000004);
        let mut child = command.spawn().unwrap();
        let job = ProcessJob::attach(&child).unwrap();
        let mut client = Client {
            input: child.stdin.take().unwrap(),
            output: BufReader::new(child.stdout.take().unwrap()),
            id: 0,
        };
        let reply =
            tokio::time::timeout(Duration::from_secs(15), client.call("probe", Value::Null))
                .await
                .unwrap()
                .unwrap();
        let pid = reply["childPid"].as_u64().unwrap() as u32;
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid).unwrap() };
        assert!(
            tokio::time::timeout(Duration::from_millis(100), client.call("hang", Value::Null))
                .await
                .is_err()
        );
        drop(job);
        drop(client);
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        let exited = unsafe { WaitForSingleObject(handle, 5000) };
        unsafe {
            let _ = CloseHandle(handle);
        }
        assert_eq!(
            exited, WAIT_OBJECT_0,
            "a descendant survived helper timeout"
        );
    }
    #[test]
    fn quantities_null_and_partial_fields() {
        assert!(matches!(
            parse_credits(&json!({"rateLimitResetCredits":null})),
            Availability::Unavailable(_)
        ));
        assert_eq!(
            parse_credits(&json!({"rateLimitResetCredits":{"availableCount":0,"credits":[]}})),
            Availability::Ready(Credits {
                count: 0,
                items: Some(vec![])
            })
        );
        assert_eq!(
            parse_credits(&json!({"rateLimitResetCredits":{"availableCount":3}})),
            Availability::Ready(Credits {
                count: 3,
                items: None
            })
        );
        assert!(matches!(
            parse_tokens(&json!({})),
            Availability::Unavailable(_)
        ));
        assert!(matches!(
            parse_tokens(&json!({"dailyUsageBuckets":[{"startDate":"2026-09-17","tokens":-1}]})),
            Availability::Unavailable(_)
        ));
        let now = Utc::now();
        assert_eq!(
            Credit {
                granted: Some(now - chrono::Duration::hours(1)),
                expires: Some(now + chrono::Duration::hours(1))
            }
            .remaining(now),
            Some(50.)
        );
    }
    #[test]
    fn coalescing_and_cache() {
        let gate = Gate::default();
        let p = gate.enter().unwrap();
        assert!(gate.enter().is_none());
        drop(p);
        assert!(gate.enter().is_some());
        let s = Snapshot {
            credits_at: Some(Instant::now()),
            tokens_at: None,
            ..Default::default()
        };
        assert_eq!(
            s.needed(
                Request {
                    credits: true,
                    tokens: true
                },
                false
            ),
            Request {
                credits: false,
                tokens: true
            }
        );
        assert!(!s.needed(Request::default(), true).any());
    }
}
