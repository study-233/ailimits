//! One isolated integration-test process owns proxy environment changes.
//! No real service credentials, Internet requests or Windows settings writes.
use ailimits::config::schema::ProxyMode;
use ailimits::network::{client, set_mode, Profile};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

const ENV: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
    "REQUEST_METHOD",
];
struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
impl Environment {
    fn clear() -> Self {
        let saved = ENV
            .iter()
            .map(|&key| (key, std::env::var_os(key)))
            .collect();
        for key in ENV {
            std::env::remove_var(key);
        }
        Self(saved)
    }
}
impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn serve_once() -> (String, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
    serve_reply(None)
}

fn serve_reply(
    reply: Option<String>,
) -> (String, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    loop {
                        let mut byte = [0];
                        if stream.read_exact(&mut byte).is_err() {
                            break;
                        }
                        bytes.push(byte[0]);
                        if bytes.ends_with(b"\r\n\r\n") {
                            break;
                        }
                        assert!(bytes.len() < 16_384);
                    }
                    let request = String::from_utf8(bytes).unwrap();
                    let response = if let Some(ref reply) = reply {
                        reply.as_str()
                    } else if request.starts_with("CONNECT ") {
                        "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    } else {
                        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                    };
                    let _ = stream.write_all(response.as_bytes());
                    tx.send(request).unwrap();
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "no request reached mock server"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("{e}"),
            }
        }
    });
    (format!("http://{address}"), rx, handle)
}

fn captured(rx: mpsc::Receiver<String>, thread: std::thread::JoinHandle<()>) -> String {
    let request = rx.recv_timeout(Duration::from_secs(8)).unwrap();
    thread.join().unwrap();
    request
}

#[test]
fn proxy_routing_profiles_reload_bypass_and_failure() {
    let _environment = Environment::clear();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        set_mode(ProxyMode::System);
        std::env::set_var("NO_PROXY", "never-bypass.invalid");
        // Every profile uses the shared proxy policy. Each new port invalidates
        // previously cached clients, without restarting the app or provider.
        for profile in [Profile::Provider, Profile::Auth, Profile::Updater] {
            let (proxy, rx, thread) = serve_once();
            std::env::set_var("HTTP_PROXY", &proxy);
            let response = client(profile)
                .unwrap()
                .get("http://service.invalid/usage")
                .send()
                .await
                .unwrap();
            assert_eq!(response.text().await.unwrap(), "ok");
            assert!(captured(rx, thread).starts_with("GET http://service.invalid/usage HTTP/1.1"));
        }

        // HTTPS is tunneled using CONNECT. The mock refuses the tunnel; the
        // app must fail, not send the bearer token in clear text or go direct.
        let (proxy, rx, thread) = serve_once();
        std::env::set_var("HTTPS_PROXY", &proxy);
        let result = client(Profile::Provider)
            .unwrap()
            .get("https://service.invalid/usage")
            .bearer_auth("synthetic-test-token")
            .send()
            .await;
        assert!(result.is_err());
        let request = captured(rx, thread);
        assert!(request.starts_with("CONNECT service.invalid:443 HTTP/1.1"));
        assert!(!request.contains("synthetic-test-token"));

        // NO_PROXY bypasses a configured, unreachable proxy.
        let (direct, rx, thread) = serve_once();
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:1");
        std::env::set_var("NO_PROXY", "127.0.0.1");
        assert!(client(Profile::Provider)
            .unwrap()
            .get(format!("{direct}/bypass"))
            .send()
            .await
            .unwrap()
            .status()
            .is_success());
        assert!(captured(rx, thread).starts_with("GET /bypass HTTP/1.1"));

        // Direct mode ignores environment proxies and system proxies alike.
        std::env::set_var("NO_PROXY", "never-bypass.invalid");
        set_mode(ProxyMode::Direct);
        let (direct, rx, thread) = serve_once();
        assert!(client(Profile::Auth)
            .unwrap()
            .get(format!("{direct}/direct"))
            .send()
            .await
            .is_ok());
        assert!(captured(rx, thread).starts_with("GET /direct HTTP/1.1"));

        // Switching back to system mode applies the bad proxy and never falls
        // back to direct, even though the target is available locally.
        set_mode(ProxyMode::System);
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        assert!(client(Profile::Provider)
            .unwrap()
            .get(format!("http://{}/", target.local_addr().unwrap()))
            .send()
            .await
            .is_err());
        assert_eq!(
            target.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );

        // A live client held by an existing operation remains usable when the
        // configured mode changes; the next acquisition gets the new policy.
        set_mode(ProxyMode::Direct);
        let old = client(Profile::Provider).unwrap();
        set_mode(ProxyMode::System);
        let (direct, rx, thread) = serve_once();
        assert!(old.get(format!("{direct}/in-flight")).send().await.is_ok());
        assert!(captured(rx, thread).starts_with("GET /in-flight HTTP/1.1"));

        // Auth/provider requests must not follow redirects; updates still may.
        set_mode(ProxyMode::Direct);
        for profile in [Profile::Provider, Profile::Auth] {
            let (address, rx, thread) = serve_reply(Some("HTTP/1.1 302 Found\r\nLocation: http://redirect.invalid/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()));
            let response = client(profile).unwrap().get(&address).send().await.unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::FOUND);
            captured(rx, thread);
        }
        let (destination, dest_rx, dest_thread) = serve_once();
        let (address, rx, thread) = serve_reply(Some(format!("HTTP/1.1 302 Found\r\nLocation: {destination}/installer\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")));
        assert_eq!(client(Profile::Updater).unwrap().get(&address).send().await.unwrap().text().await.unwrap(), "ok");
        captured(rx, thread);
        assert!(captured(dest_rx, dest_thread).starts_with("GET /installer HTTP/1.1"));

        // SOCKS5h forwards the destination hostname to the local proxy rather
        // than resolving the deliberately nonexistent hostname on this PC.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy = format!("socks5h://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let socks = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(8);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(std::time::Instant::now() < deadline);
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut greeting = [0; 2];
            stream.read_exact(&mut greeting).unwrap();
            assert_eq!(greeting[0], 5);
            let mut methods = vec![0; greeting[1] as usize];
            stream.read_exact(&mut methods).unwrap();
            assert!(methods.contains(&0));
            stream.write_all(&[5, 0]).unwrap();
            let mut request = [0; 5];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(&request[..4], &[5, 1, 0, 3]);
            let mut host = vec![0; request[4] as usize];
            stream.read_exact(&mut host).unwrap();
            let mut port = [0; 2];
            stream.read_exact(&mut port).unwrap();
            assert_eq!(host, b"service.invalid");
            assert_eq!(u16::from_be_bytes(port), 80);
            // Refuse the request after verifying negotiation; no Internet.
            stream.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
        });
        std::env::set_var("HTTP_PROXY", &proxy);
        std::env::set_var("ALL_PROXY", &proxy);
        set_mode(ProxyMode::System);
        assert!(client(Profile::Provider).unwrap().get("http://service.invalid/usage").send().await.is_err());
        socks.join().unwrap();
    });
}
