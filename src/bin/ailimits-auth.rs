use ailimits::i18n::t;
use ailimits::tr;
// ailimits-auth — console utility for provider authorization.
//
// Commands:
//   ailimits-auth status                     show detected auth sources
//   ailimits-auth set <provider>             store an API key / PAT in Credential Manager
//   ailimits-auth remove <provider>          remove the key
//   ailimits-auth set-usage-token <p>        store a manual usage token (claude | codex)
//   ailimits-auth remove-usage-token <p>     remove the usage token
//
// Secrets are stored ONLY in Windows Credential Manager (service "ailimits");
// config.toml holds nothing but a label.

use ailimits::config::{
    schema::{AuthMethod, Config},
    storage,
};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

/// Credential Manager service name.
const KEYRING_SERVICE: &str = "ailimits";

/// Providers that accept a stored key (Codex uses a usage token instead).
const PROVIDERS: &[(&str, &str)] = &[("claude", "claude_api_key"), ("copilot", "copilot_pat")];

/// Manual usage tokens: tried by the widget BEFORE the CLI-owned token files.
const USAGE_TOKEN_PROVIDERS: &[(&str, &str)] = &[
    ("claude", "claude_usage_token"),
    ("codex", "codex_usage_token"),
];

const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const CLAUDE_USAGE_BETA_HEADER: &str = "oauth-2025-04-20";
const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

fn main() -> Result<()> {
    // Read preferences without creating or rewriting config on --help/status.
    let preferences = std::fs::read_to_string(storage::config_path())
        .ok()
        .and_then(|text| toml::from_str::<Config>(&text).ok())
        .unwrap_or_default();
    ailimits::i18n::set_language(preferences.general.language);
    ailimits::network::set_mode(preferences.network.proxy_mode);
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("status") => status(),
        Some("set") => set(args.get(1).map(String::as_str)),
        Some("remove") => remove(args.get(1).map(String::as_str)),
        Some("set-usage-token") => set_usage_token(args.get(1).map(String::as_str)),
        Some("remove-usage-token") => remove_usage_token(args.get(1).map(String::as_str)),
        _ => {
            print_usage();
            Ok(())
        }
    }
}

fn print_usage() {
    println!("{}", help_text());
}

fn help_text() -> String {
    [
        "ailimits-auth — provider authorization for the QuotaBar widget",
        "",
        "Usage:",
        "  ailimits-auth status                    show auth status",
        "  ailimits-auth set <provider>            store a key (claude: API key, copilot: PAT)",
        "  ailimits-auth remove <provider>         remove the key",
        "  ailimits-auth set-usage-token <p>       store a manual usage token (claude | codex)",
        "  ailimits-auth remove-usage-token <p>    remove the usage token",
        "",
        "By default everything works through subscriptions, no keys needed:",
        "  Claude — Claude Code token, Codex — Codex CLI token, Copilot — gh CLI token.",
    ]
    .into_iter()
    .map(t)
    .collect::<Vec<_>>()
    .join("\n")
}

/// Key label for a provider.
fn label_for(provider: &str) -> Result<&'static str> {
    PROVIDERS
        .iter()
        .find(|(id, _)| *id == provider)
        .map(|(_, label)| *label)
        .with_context(|| {
            tr!(
                "provider '{provider}' has no key (expected: claude or copilot)",
                provider = provider
            )
        })
}

/// Usage-token label for a provider.
fn usage_label_for(provider: &str) -> Result<&'static str> {
    USAGE_TOKEN_PROVIDERS
        .iter()
        .find(|(id, _)| *id == provider)
        .map(|(_, label)| *label)
        .with_context(|| {
            tr!(
                "usage token is supported for claude and codex, got '{provider}'",
                provider = provider
            )
        })
}

/// Show the state of every auth source.
fn status() -> Result<()> {
    println!("{}", t("── Auth sources ─────────────────────────────────"));

    // Claude Code subscription: OAuth token.
    let claude_dir = dirs::home_dir().unwrap_or_default().join(".claude");
    let creds_path = claude_dir.join(".credentials.json");
    if creds_path.exists() {
        match read_oauth_expiry(&creds_path) {
            Some((expires_at, sub_type)) => {
                let valid = expires_at > Utc::now();
                let mark = if valid { t("OK") } else { t("no (expired)") };
                println!(
                    "{}",
                    tr!(
                        "Claude OAuth ({sub_type} subscription): {mark}",
                        sub_type = sub_type,
                        mark = mark
                    )
                );
                println!(
                    "{}",
                    tr!(
                        "  token valid until {expiry} UTC",
                        expiry = expires_at.format("%Y-%m-%d %H:%M")
                    )
                );
                if !valid {
                    println!(
                        "{}",
                        t("  → run Claude Code, it refreshes the token automatically")
                    );
                }
            }
            None => println!(
                "{}",
                t("Claude OAuth: no (.credentials.json has no valid token)")
            ),
        }
    } else {
        println!(
            "{}",
            t("Claude OAuth: no (.credentials.json missing — run Claude Code once)")
        );
    }

    // Claude Code local files.
    let statusline = claude_dir.join("statusline.jsonl").exists();
    let stats = claude_dir.join("stats-cache.json").exists();
    println!(
        "statusline.jsonl: {}",
        if statusline { t("OK") } else { t("no") }
    );
    println!(
        "stats-cache.json: {}",
        if stats { t("OK") } else { t("no") }
    );

    // Manual usage tokens.
    for (id, label) in USAGE_TOKEN_PROVIDERS {
        let present = keyring::Entry::new(KEYRING_SERVICE, label)
            .and_then(|e| e.get_password())
            .is_ok();
        println!(
            "{}",
            tr!(
                "{id} usage token ({label}): {state}",
                id = id,
                label = label,
                state = if present { t("stored") } else { "—" }
            )
        );
    }

    // Codex CLI auth.json.
    let codex_auth = dirs::home_dir()
        .unwrap_or_default()
        .join(".codex")
        .join("auth.json");
    println!(
        "Codex auth.json: {}",
        if codex_auth.exists() {
            t("OK")
        } else {
            t("no (run codex login)")
        }
    );

    // gh CLI for Copilot. Run from the system dir so a planted gh.exe in the
    // current directory cannot be invoked instead (CWD-first search order).
    let mut gh_cmd = std::process::Command::new("gh");
    gh_cmd.args(["auth", "token"]);
    if let Some(root) = std::env::var_os("SystemRoot") {
        gh_cmd.current_dir(std::path::Path::new(&root).join("System32"));
    }
    let gh_ok = gh_cmd.output().map(|o| o.status.success()).unwrap_or(false);
    println!(
        "{}",
        tr!(
            "gh CLI token (Copilot): {state}",
            state = if gh_ok {
                t("OK")
            } else {
                t("no (gh auth login, or store a PAT)")
            }
        )
    );

    // Antigravity (Google): the Credential Manager session first, then the
    // legacy Gemini CLI file — the same order the provider tries.
    println!(
        "{}",
        tr!(
            "Antigravity keyring (gemini:antigravity): {state}",
            state = if ailimits::providers::antigravity::keyring_token_present() {
                t("OK")
            } else {
                t("no (run Antigravity once)")
            }
        )
    );
    let gemini_creds = dirs::home_dir()
        .unwrap_or_default()
        .join(".gemini")
        .join("oauth_creds.json");
    println!(
        "{}",
        tr!(
            "legacy Gemini CLI oauth_creds.json: {state}",
            state = if gemini_creds.exists() {
                t("OK")
            } else {
                t("no")
            }
        )
    );

    // Stored keys.
    println!();
    println!(
        "{}",
        t("── Keys (Credential Manager) ─────────────────────")
    );
    for (id, label) in PROVIDERS {
        let present = keyring::Entry::new(KEYRING_SERVICE, label)
            .and_then(|e| e.get_password())
            .is_ok();
        println!(
            "{id:<8} ({label}): {}",
            if present { t("stored") } else { "—" }
        );
    }

    // Current config.
    println!();
    println!(
        "{}",
        tr!(
            "── Config ({path}) ──",
            path = storage::config_path().display()
        )
    );
    let config = load_config()?;
    for p in &config.providers {
        let method = match p.auth_method {
            AuthMethod::Subscription => t("subscription"),
            AuthMethod::ApiKey => "api_key",
        };
        let state = if p.enabled {
            t("enabled")
        } else {
            t("disabled")
        };
        println!(
            "{}",
            tr!(
                "{provider} {state}, method: {method}",
                provider = format!("{:<8}", p.id),
                state = state,
                method = method
            )
        );
    }

    Ok(())
}

/// Store a key and update the config.
fn set(provider: Option<&str>) -> Result<()> {
    let provider = provider.context(t("specify a provider: ailimits-auth set <claude|copilot>"))?;
    let label = label_for(provider)?;

    // Hidden input: the key is invisible and stays out of terminal history.
    let key =
        rpassword::prompt_password(tr!("Paste the key for {provider}: ", provider = provider))
            .context(t("failed to read input"))?;
    let key = key.trim();
    if key.is_empty() {
        bail!("{}", t("empty key — nothing stored"));
    }

    keyring::Entry::new(KEYRING_SERVICE, label)
        .context(t("keyring unavailable"))?
        .set_password(key)
        .context(t("failed to store the key in Credential Manager"))?;

    // Claude: a key implies the api_key method. Copilot: the PAT is just
    // a token source for the subscription method.
    let mut config = load_config()?;
    if let Some(p) = config.providers.iter_mut().find(|p| p.id == provider) {
        p.enabled = true;
        p.credential_label = label.to_string();
        if provider == "claude" {
            p.auth_method = AuthMethod::ApiKey;
        }
    }
    save_config(&config)?;

    println!(
        "{}",
        tr!(
            "Key for '{provider}' stored in Credential Manager (label: {label})",
            provider = provider,
            label = label
        )
    );
    println!("{}", t("→ Restart the widget to apply"));
    Ok(())
}

/// Store a manual usage token without touching config/auth_method.
fn set_usage_token(provider: Option<&str>) -> Result<()> {
    let provider = provider.context(t(
        "specify a provider: ailimits-auth set-usage-token <claude|codex>",
    ))?;
    let label = usage_label_for(provider)?;

    let token = rpassword::prompt_password(tr!(
        "Paste the {provider} usage token: ",
        provider = provider
    ))
    .context(t("failed to read input"))?;
    let token = token.trim();
    if token.is_empty() {
        bail!("{}", t("empty token — nothing stored"));
    }
    validate_usage_token(provider, token)?;

    keyring::Entry::new(KEYRING_SERVICE, label)
        .context(t("keyring unavailable"))?
        .set_password(token)
        .context(t("failed to store the usage token in Credential Manager"))?;

    println!(
        "{}",
        tr!(
            "{provider} usage token stored (label: {label})",
            provider = provider,
            label = label
        )
    );
    println!("{}", t("→ Restart the widget to apply"));
    Ok(())
}

/// One read-only request to the provider's usage endpoint to verify the token.
fn validate_usage_token(provider: &str, token: &str) -> Result<()> {
    let url = match provider {
        "claude" => CLAUDE_USAGE_URL,
        _ => CODEX_USAGE_URL,
    };
    let status = tokio::runtime::Runtime::new()?.block_on(async {
        let client = ailimits::network::client(ailimits::network::Profile::Auth)?;
        let mut req = client.get(url).bearer_auth(token);
        if provider == "claude" {
            req = req.header("anthropic-beta", CLAUDE_USAGE_BETA_HEADER);
        } else {
            req = req.header("User-Agent", "ailimits-widget");
        }
        Ok::<_, anyhow::Error>(req.send().await?.status())
    })?;

    if status.as_u16() == 200 {
        Ok(())
    } else {
        bail!(
            "{}",
            tr!(
                "the usage endpoint rejected the token: HTTP {status}",
                status = status
            )
        );
    }
}

fn remove_usage_token(provider: Option<&str>) -> Result<()> {
    let provider = provider.context(t(
        "specify a provider: ailimits-auth remove-usage-token <claude|codex>",
    ))?;
    let label = usage_label_for(provider)?;

    match keyring::Entry::new(KEYRING_SERVICE, label).and_then(|e| e.delete_credential()) {
        Ok(()) => println!(
            "{}",
            tr!("{provider} usage token removed", provider = provider)
        ),
        Err(keyring::Error::NoEntry) => println!(
            "{}",
            tr!("— no {provider} usage token stored", provider = provider)
        ),
        Err(e) => bail!(
            "{}",
            tr!("failed to remove the usage token: {error}", error = e)
        ),
    }

    println!("{}", t("→ Restart the widget to apply"));
    Ok(())
}

/// Remove a key and switch the provider back to the subscription method.
fn remove(provider: Option<&str>) -> Result<()> {
    let provider = provider.context(t(
        "specify a provider: ailimits-auth remove <claude|copilot>",
    ))?;
    let label = label_for(provider)?;

    match keyring::Entry::new(KEYRING_SERVICE, label).and_then(|e| e.delete_credential()) {
        Ok(()) => println!(
            "{}",
            tr!(
                "Key '{label}' removed from Credential Manager",
                label = label
            )
        ),
        Err(keyring::Error::NoEntry) => {
            println!("{}", tr!("— no key '{label}' stored", label = label))
        }
        Err(e) => bail!("{}", tr!("failed to remove the key: {error}", error = e)),
    }

    let mut config = load_config()?;
    if let Some(p) = config.providers.iter_mut().find(|p| p.id == provider) {
        // Without a key everything runs on the subscription method.
        p.credential_label = String::new();
        p.auth_method = AuthMethod::Subscription;
        println!(
            "{}",
            tr!(
                "'{provider}' switched back to the subscription method",
                provider = provider
            )
        );
    }
    save_config(&config)?;

    println!("{}", t("→ Restart the widget to apply"));
    Ok(())
}

/// Read expiresAt and the subscription type from .credentials.json.
fn read_oauth_expiry(path: &std::path::Path) -> Option<(DateTime<Utc>, String)> {
    let content = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    let oauth = value.get("claudeAiOauth")?;
    let ms = oauth.get("expiresAt")?.as_i64()?;
    let sub = oauth
        .get("subscriptionType")
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string();
    DateTime::<Utc>::from_timestamp_millis(ms).map(|dt| (dt, sub))
}

/// Config helpers — storage is async, the CLI is sync.
fn load_config() -> Result<Config> {
    tokio::runtime::Runtime::new()?.block_on(storage::load_or_default())
}

fn save_config(config: &Config) -> Result<()> {
    tokio::runtime::Runtime::new()?.block_on(storage::save(config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailimits::config::schema::Language;

    #[test]
    fn help_and_validation_messages_follow_the_selected_language() {
        ailimits::i18n::set_language(Language::Chinese);
        assert!(help_text().contains("查看认证状态"));
        assert!(help_text().contains("ailimits-auth set-usage-token"));
        assert!(label_for("unknown")
            .unwrap_err()
            .to_string()
            .contains("不支持密钥"));
        ailimits::i18n::set_language(Language::English);
        assert!(help_text().contains("show auth status"));
        assert!(label_for("unknown")
            .unwrap_err()
            .to_string()
            .contains("has no key"));
    }
}
