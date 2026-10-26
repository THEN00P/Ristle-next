//! The M4 rules, each against the local test server.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use ristle_domain::{ConnectionId, ManualClock, PluginId};
use ristle_net::test_support::{MemoryCacheStore, RecordedRequest, TestResponse, TestServer};
use ristle_net::{
    Attach, CacheKey, CacheMethod, CacheOverride, CachePolicy, CacheScope, Client, ClientConfig,
    Error, GrantedOrigin, HostPattern, HttpCacheStore, Method, Reach, Request, StoreError,
    StoredResponse, cache, headers,
};
use url::Url;

const START: u64 = 1_800_000_000_000;

struct Harness {
    client: Client,
    clock: Arc<ManualClock>,
    store: Arc<MemoryCacheStore>,
}

fn harness() -> Harness {
    let clock = Arc::new(ManualClock::new(START));
    let store = Arc::new(MemoryCacheStore::new());
    let client = Client::new(ClientConfig::default(), clock.clone(), Some(store.clone())).unwrap();
    Harness {
        client,
        clock,
        store,
    }
}

fn scope(connection: &str, generation: u64) -> CacheScope {
    CacheScope {
        plugin: PluginId::new("dev.example.test"),
        connection: ConnectionId::new(connection),
        generation,
    }
}

fn get(server: &TestServer, path: &str) -> Request {
    Request::new(Method::Get, server.url(path), server.reach())
}

fn cached_get(server: &TestServer, path: &str) -> Request {
    Request {
        cache: CachePolicy::Http(scope("c1", 1)),
        ..get(server, path)
    }
}

fn overridden_get(
    server: &TestServer,
    path: &str,
    ttl: u64,
    stale: u64,
    key: Option<&str>,
) -> Request {
    Request {
        cache: CachePolicy::Override(
            scope("c1", 1),
            CacheOverride {
                ttl: Duration::from_secs(ttl),
                stale: Duration::from_secs(stale),
                key: key.map(str::to_owned),
            },
        ),
        ..get(server, path)
    }
}

/// A server answering every request with the same response.
async fn serving(response: TestResponse) -> TestServer {
    TestServer::start(move |_| response.clone()).await.unwrap()
}

/// A server whose answer the test changes.
struct Scripted {
    server: TestServer,
    next: Arc<Mutex<TestResponse>>,
}

impl Scripted {
    async fn new(first: TestResponse) -> Self {
        let next = Arc::new(Mutex::new(first));
        let answer = next.clone();
        let server = TestServer::start(move |_| answer.lock().unwrap().clone())
            .await
            .unwrap();
        Self { server, next }
    }

    fn answer(&self, response: TestResponse) {
        *self.next.lock().unwrap() = response;
    }
}

// Request policy ------------------------------------------------------------

#[tokio::test]
async fn a_get_returns_status_headers_and_body() {
    let h = harness();
    let server = serving(TestResponse::ok("hello").header("X-Test", "1")).await;
    let mut request = get(&server, "/a?b=c#fragment");
    request.headers.push(("Accept".into(), "text/plain".into()));
    let response = h.client.send(request).await.unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, Bytes::from("hello"));
    assert_eq!(headers::get(&response.headers, "x-test"), Some("1"));
    assert_eq!(response.received, response.headers);
    assert!(!response.cached);
    assert!(response.reached_provider);
    assert_eq!(response.fetched_at_ms, START);
    let seen = server.requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].path, "/a?b=c");
    assert_eq!(seen[0].header("accept"), Some("text/plain"));
}

#[tokio::test]
async fn forbidden_headers_fail_before_anything_is_sent() {
    let h = harness();
    let server = serving(TestResponse::ok("")).await;
    for name in [
        "Host",
        "Content-Length",
        "Transfer-Encoding",
        "Connection",
        "Proxy-Authorization",
        ":path",
    ] {
        let mut request = get(&server, "/");
        request.headers.push((name.into(), "x".into()));
        let error = h.client.send(request.clone()).await.unwrap_err();
        assert!(matches!(error, Error::NotAllowed(_)), "{name}: {error:?}");
        let error = h.client.stream(request).await.unwrap_err();
        assert!(matches!(error, Error::NotAllowed(_)), "{name}: {error:?}");
    }
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn attach_rules_obey_the_forbidden_list_too() {
    struct HostHeader;
    impl Attach for HostHeader {
        fn headers(&self, _: &Url) -> Vec<(String, String)> {
            vec![("Host".into(), "evil.example".into())]
        }
    }
    let h = harness();
    let server = serving(TestResponse::ok("")).await;
    let mut request = get(&server, "/");
    request.attach = Some(Arc::new(HostHeader));
    assert!(matches!(
        h.client.send(request).await,
        Err(Error::NotAllowed(_))
    ));
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn undeclared_hosts_and_plain_http_fail_before_anything_is_sent() {
    let h = harness();
    let server = serving(TestResponse::ok("")).await;
    // Declared as a manifest host, but manifest hosts need HTTPS.
    let declared = Arc::new(Reach::hosts(vec![HostPattern::parse("127.0.0.1").unwrap()]));
    let request = Request::new(Method::Get, server.url("/"), declared);
    assert!(matches!(
        h.client.send(request).await,
        Err(Error::NotAllowed(_))
    ));
    // Not declared at all.
    let request = Request::new(Method::Get, server.url("/"), Arc::new(Reach::none()));
    assert!(matches!(
        h.client.send(request).await,
        Err(Error::NotAllowed(_))
    ));
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn manifest_hosts_never_reach_loopback() {
    let h = harness();
    let server = serving(TestResponse::ok("")).await;
    let port = server.url("/").port().unwrap();
    for host in ["localhost", "127.0.0.1"] {
        let reach = Arc::new(Reach::hosts(vec![HostPattern::parse(host).unwrap()]));
        let url = Url::parse(&format!("https://{host}:{port}/")).unwrap();
        let error = h
            .client
            .send(Request::new(Method::Get, url, reach))
            .await
            .unwrap_err();
        match error {
            Error::AddressRefused {
                granted, addresses, ..
            } => {
                assert!(!granted);
                assert!(addresses.iter().all(|address| address.is_loopback()));
            }
            other => panic!("{host}: {other:?}"),
        }
    }
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn a_granted_origin_reaches_only_the_confirmed_address() {
    let h = harness();
    let server = serving(TestResponse::ok("")).await;
    // The user confirmed another device; the name now leads here.
    let moved = GrantedOrigin {
        confirmed: vec!["192.168.1.20".parse().unwrap()],
        ..server.granted()
    };
    let request = Request::new(
        Method::Get,
        server.url("/"),
        Arc::new(Reach::none().with_granted(vec![moved])),
    );
    match h.client.send(request).await.unwrap_err() {
        Error::AddressRefused {
            granted, addresses, ..
        } => {
            assert!(granted);
            assert_eq!(
                addresses,
                vec!["127.0.0.1".parse::<std::net::IpAddr>().unwrap()]
            );
        }
        other => panic!("{other:?}"),
    }
    // Public-only is not enough for a loopback server either.
    let public = GrantedOrigin {
        public: true,
        confirmed: Vec::new(),
        ..server.granted()
    };
    let request = Request::new(
        Method::Get,
        server.url("/"),
        Arc::new(Reach::none().with_granted(vec![public])),
    );
    assert!(matches!(
        h.client.send(request).await,
        Err(Error::AddressRefused { granted: true, .. })
    ));
    assert!(server.requests().is_empty());
    // The confirmed address works.
    assert_eq!(h.client.send(get(&server, "/")).await.unwrap().status, 200);
}

#[tokio::test]
async fn response_headers_are_bounded_while_parsing() {
    let h = harness();
    let mut many = TestResponse::ok("");
    // The server adds Content-Length, so 199 of ours make 200.
    for index in 0..199 {
        many = many.header(&format!("x-{index}"), "v");
    }
    let server = serving(many.clone()).await;
    let response = h.client.send(get(&server, "/")).await.unwrap();
    assert_eq!(response.headers.len(), 200);

    let server = serving(many.header("x-one-more", "v")).await;
    assert!(matches!(
        h.client.send(get(&server, "/")).await,
        Err(Error::TooLarge(_))
    ));

    let big = "v".repeat(40 * 1024);
    let server = serving(TestResponse::ok("").header("x-a", &big).header("x-b", &big)).await;
    assert!(matches!(
        h.client.send(get(&server, "/")).await,
        Err(Error::TooLarge(_))
    ));
    assert!(matches!(
        h.client.stream(get(&server, "/")).await,
        Err(Error::TooLarge(_))
    ));

    let server = serving(TestResponse::ok("").header("x-a", &big)).await;
    assert_eq!(h.client.send(get(&server, "/")).await.unwrap().status, 200);
}

#[tokio::test]
async fn bodies_are_bounded() {
    let h = harness();
    let server = serving(TestResponse::ok(vec![7u8; 2_000])).await;
    let mut request = get(&server, "/");
    request.max_body = 1_999;
    assert!(matches!(
        h.client.send(request.clone()).await,
        Err(Error::TooLarge(_))
    ));
    assert!(matches!(
        h.client.stream(request).await,
        Err(Error::TooLarge(_))
    ));

    // Without a Content-Length the bound holds while reading.
    let chunked = TestResponse {
        chunks: Some(vec![
            Bytes::from(vec![1u8; 1_000]),
            Bytes::from(vec![2u8; 1_000]),
        ]),
        ..TestResponse::ok("")
    };
    let server = serving(chunked).await;
    let mut request = get(&server, "/");
    request.max_body = 1_500;
    assert!(matches!(
        h.client.send(request.clone()).await,
        Err(Error::TooLarge(_))
    ));
    let mut stream = h.client.stream(request).await.unwrap();
    assert_eq!(stream.chunk().await.unwrap().unwrap().len(), 1_000);
    assert!(matches!(stream.chunk().await, Err(Error::TooLarge(_))));

    // A HEAD names the resource's length but has no body.
    let large = serving(TestResponse::ok(vec![7u8; 2_000])).await;
    let mut head = Request::new(Method::Head, large.url("/"), large.reach());
    head.max_body = 10;
    assert_eq!(h.client.send(head.clone()).await.unwrap().status, 200);
    assert_eq!(
        h.client.stream(head).await.unwrap().total_length(),
        Some(2_000)
    );

    // The default bound is 16 MiB.
    let at_limit = serving(TestResponse::ok(vec![
        0u8;
        ristle_net::MAX_BODY_BYTES as usize
    ]))
    .await;
    assert_eq!(
        h.client.send(get(&at_limit, "/")).await.unwrap().body.len() as u64,
        ristle_net::MAX_BODY_BYTES
    );
    let over = serving(TestResponse::ok(vec![
        0u8;
        ristle_net::MAX_BODY_BYTES as usize
            + 1
    ]))
    .await;
    assert!(matches!(
        h.client.send(get(&over, "/")).await,
        Err(Error::TooLarge(_))
    ));
}

#[tokio::test]
async fn requests_time_out() {
    let h = harness();
    let slow = TestResponse {
        delay: Duration::from_secs(5),
        ..TestResponse::ok("late")
    };
    let server = serving(slow).await;
    let mut request = get(&server, "/");
    request.timeout = Duration::from_millis(200);
    assert_eq!(h.client.send(request.clone()).await, Err(Error::Timeout));
    assert!(matches!(
        h.client.stream(request).await,
        Err(Error::Timeout)
    ));
}

#[tokio::test]
async fn a_body_that_breaks_off_is_a_network_error() {
    let h = harness();
    let server = serving(TestResponse {
        truncate: true,
        ..TestResponse::ok("partial")
    })
    .await;
    assert!(matches!(
        h.client.send(get(&server, "/")).await,
        Err(Error::Network(_))
    ));
}

// Redirects -----------------------------------------------------------------

struct Credential {
    host: String,
}

impl Attach for Credential {
    fn headers(&self, url: &Url) -> Vec<(String, String)> {
        if url.host_str() == Some(self.host.as_str()) {
            vec![(
                "Authorization".into(),
                format!("Bearer for-{}", url.port().unwrap()),
            )]
        } else {
            Vec::new()
        }
    }
}

#[tokio::test]
async fn same_origin_redirects_keep_the_request() {
    let h = harness();
    let server = TestServer::start(|request| match request.path.as_str() {
        "/old" => TestResponse::status(301).header("Location", "/new"),
        "/temp" => TestResponse::status(307).header("Location", "/new"),
        _ => TestResponse::ok("new"),
    })
    .await
    .unwrap();
    let mut request = get(&server, "/old");
    request.headers.push(("X-Plugin".into(), "1".into()));
    let response = h.client.send(request).await.unwrap();
    assert_eq!(response.body, Bytes::from("new"));
    assert_eq!(response.url.path(), "/new");
    let seen = server.requests();
    assert_eq!(seen[1].header("x-plugin"), Some("1"));

    // 307 keeps the method and body.
    let mut post = Request::new(Method::Post, server.url("/temp"), server.reach());
    post.body = Bytes::from("payload");
    post.headers
        .push(("Content-Type".into(), "text/plain".into()));
    h.client.send(post).await.unwrap();
    let seen = server.requests();
    let last = seen.last().unwrap();
    assert_eq!(last.method, "POST");
    assert_eq!(last.body, Bytes::from("payload"));
    assert_eq!(last.header("content-type"), Some("text/plain"));
}

#[tokio::test]
async fn see_other_and_moved_posts_become_bodiless_gets() {
    let h = harness();
    let server = TestServer::start(|request| match request.path.as_str() {
        "/form" => TestResponse::status(303).header("Location", "/done"),
        "/moved" => TestResponse::status(302).header("Location", "/done"),
        _ => TestResponse::ok("done"),
    })
    .await
    .unwrap();
    for path in ["/form", "/moved"] {
        let mut post = Request::new(Method::Post, server.url(path), server.reach());
        post.body = Bytes::from("a=1");
        post.headers.push((
            "Content-Type".into(),
            "application/x-www-form-urlencoded".into(),
        ));
        post.headers.push(("X-Plugin".into(), "1".into()));
        h.client.send(post).await.unwrap();
        let seen = server.requests();
        let last = seen.last().unwrap();
        assert_eq!(last.method, "GET", "{path}");
        assert!(last.body.is_empty());
        assert_eq!(last.header("content-type"), None);
        assert_eq!(last.header("x-plugin"), Some("1"));
    }
}

#[tokio::test]
async fn cross_origin_redirects_drop_the_plugins_headers_and_attach_per_host() {
    let h = harness();
    let target = serving(TestResponse::ok("there")).await;
    let to = target.url("/landing");
    let origin =
        TestServer::start(move |_| TestResponse::status(302).header("Location", to.as_str()))
            .await
            .unwrap();
    // Both are 127.0.0.1 on different ports, so different origins; the
    // credential is attached by host and port.
    let reach = Arc::new(Reach::none().with_granted(vec![origin.granted(), target.granted()]));
    let mut request = Request::new(Method::Get, origin.url("/start"), reach);
    request.headers.push(("X-Plugin".into(), "secret".into()));
    request.attach = Some(Arc::new(Credential {
        host: "127.0.0.1".into(),
    }));
    let response = h.client.send(request).await.unwrap();
    assert_eq!(response.body, Bytes::from("there"));
    let first = &origin.requests()[0];
    assert_eq!(first.header("x-plugin"), Some("secret"));
    let origin_port = origin.url("/").port().unwrap();
    assert_eq!(
        first.header("authorization"),
        Some(format!("Bearer for-{origin_port}").as_str())
    );
    let landed = &target.requests()[0];
    assert_eq!(landed.header("x-plugin"), None);
    let target_port = target.url("/").port().unwrap();
    assert_eq!(
        landed.header("authorization"),
        Some(format!("Bearer for-{target_port}").as_str())
    );
}

#[tokio::test]
async fn cross_origin_redirects_that_resend_a_body_are_not_followed() {
    let h = harness();
    let target = serving(TestResponse::ok("there")).await;
    let to = target.url("/landing");
    let origin =
        TestServer::start(move |_| TestResponse::status(307).header("Location", to.as_str()))
            .await
            .unwrap();
    let reach = Arc::new(Reach::none().with_granted(vec![origin.granted(), target.granted()]));
    let mut post = Request::new(Method::Post, origin.url("/upload"), reach.clone());
    post.body = Bytes::from("secret body");
    let response = h.client.send(post).await.unwrap();
    assert_eq!(response.status, 307);
    assert!(target.requests().is_empty());
    // A GET has no body to resend and is followed.
    let response = h
        .client
        .send(Request::new(Method::Get, origin.url("/x"), reach))
        .await
        .unwrap();
    assert_eq!(response.status, 200);
}

#[tokio::test]
async fn bodies_and_state_changing_methods_never_follow_a_redirect_to_another_origin() {
    let h = harness();
    let target = serving(TestResponse::ok("there")).await;
    let to = target.url("/landing");
    let status = Arc::new(Mutex::new(301u16));
    let answer = status.clone();
    let origin = TestServer::start(move |_| {
        TestResponse::status(*answer.lock().unwrap()).header("Location", to.as_str())
    })
    .await
    .unwrap();
    let reach = Arc::new(Reach::none().with_granted(vec![origin.granted(), target.granted()]));
    let request = |method: Method, body: &'static str| {
        let mut request = Request::new(method, origin.url("/x"), reach.clone());
        request.body = Bytes::from(body);
        request
    };
    for code in [301, 302, 307, 308] {
        *status.lock().unwrap() = code;
        // PUT and PATCH keep their method on 301 and 302, so they are replays.
        for method in [Method::Put, Method::Patch, Method::Delete] {
            let response = h.client.send(request(method, "secret")).await.unwrap();
            assert_eq!(response.status, code, "{method:?} {code}");
            let response = h.client.send(request(method, "")).await.unwrap();
            assert_eq!(response.status, code, "{method:?} {code} without a body");
        }
        // A GET that carries a body would replay it.
        let response = h.client.send(request(Method::Get, "secret")).await.unwrap();
        assert_eq!(response.status, code, "GET with a body, {code}");
    }
    assert!(target.requests().is_empty());
    // A POST answered with 302 becomes a bodiless GET, which may go.
    *status.lock().unwrap() = 302;
    let response = h
        .client
        .send(request(Method::Post, "secret"))
        .await
        .unwrap();
    assert_eq!(response.status, 200);
    let landed = target.requests();
    assert_eq!(landed.len(), 1);
    assert_eq!(landed[0].method, "GET");
    assert!(landed[0].body.is_empty());
}

#[tokio::test]
async fn redirects_leave_the_reach_never() {
    let h = harness();
    let elsewhere = serving(TestResponse::ok("elsewhere")).await;
    let to = elsewhere.url("/");
    let server =
        TestServer::start(move |_| TestResponse::status(302).header("Location", to.as_str()))
            .await
            .unwrap();
    let response = h.client.send(get(&server, "/")).await.unwrap();
    assert_eq!(response.status, 302);
    assert!(elsewhere.requests().is_empty());
}

#[tokio::test]
async fn redirect_loops_end() {
    let h = harness();
    let server = serving(TestResponse::status(302).header("Location", "/again")).await;
    assert!(matches!(
        h.client.send(get(&server, "/")).await,
        Err(Error::Network(_))
    ));
    assert_eq!(server.requests().len(), ristle_net::MAX_REDIRECTS + 1);
}

// Rate limits ---------------------------------------------------------------

#[tokio::test]
async fn a_429_holds_the_host_until_retry_after() {
    let h = harness();
    let scripted = Scripted::new(TestResponse::status(429).header("Retry-After", "120")).await;
    let server = &scripted.server;
    assert_eq!(
        h.client.send(get(server, "/")).await,
        Err(Error::RateLimited {
            retry_after_ms: Some(120_000)
        })
    );
    scripted.answer(TestResponse::ok("fine"));
    h.clock.advance(20_000);
    // Held for everyone, buffered or streamed, without reaching the server.
    assert_eq!(
        h.client.send(get(server, "/other")).await,
        Err(Error::RateLimited {
            retry_after_ms: Some(100_000)
        })
    );
    assert!(matches!(
        h.client.stream(get(server, "/media")).await,
        Err(Error::RateLimited { .. })
    ));
    assert_eq!(server.requests().len(), 1);
    assert_eq!(
        h.client.rate_limit_remaining("127.0.0.1"),
        Some(Duration::from_secs(100))
    );
    h.clock.advance(100_000);
    assert_eq!(h.client.send(get(server, "/")).await.unwrap().status, 200);
}

#[tokio::test]
async fn a_429_without_retry_after_holds_for_the_default() {
    let h = harness();
    let server = serving(TestResponse::status(429)).await;
    assert_eq!(
        h.client.send(get(&server, "/")).await,
        Err(Error::RateLimited {
            retry_after_ms: None
        })
    );
    assert_eq!(
        h.client.rate_limit_remaining("127.0.0.1"),
        Some(Duration::from_millis(ristle_net::DEFAULT_HOLD_MS))
    );
}

#[tokio::test]
async fn a_503_is_a_rate_limit_only_with_retry_after() {
    let h = harness();
    let plain = serving(TestResponse::status(503)).await;
    assert_eq!(h.client.send(get(&plain, "/")).await.unwrap().status, 503);
    assert_eq!(h.client.rate_limit_remaining("127.0.0.1"), None);
    let limited = serving(TestResponse::status(503).header("Retry-After", "5")).await;
    assert_eq!(
        h.client.send(get(&limited, "/")).await,
        Err(Error::RateLimited {
            retry_after_ms: Some(5_000)
        })
    );
}

#[tokio::test]
async fn a_reported_limit_holds_the_host_like_a_429() {
    let h = harness();
    let server = serving(TestResponse::ok("")).await;
    h.client
        .report_rate_limit("127.0.0.1", Duration::from_secs(30));
    assert_eq!(
        h.client.send(get(&server, "/")).await,
        Err(Error::RateLimited {
            retry_after_ms: Some(30_000)
        })
    );
    assert!(server.requests().is_empty());
    h.clock.advance(30_000);
    assert_eq!(h.client.send(get(&server, "/")).await.unwrap().status, 200);
}

// Cache ---------------------------------------------------------------------

#[tokio::test]
async fn fresh_responses_are_served_from_the_cache() {
    let h = harness();
    let server = serving(TestResponse::ok("one").header("Cache-Control", "max-age=60")).await;
    let first = h.client.send(cached_get(&server, "/a")).await.unwrap();
    assert!(!first.cached);
    h.clock.advance(59_000);
    let second = h.client.send(cached_get(&server, "/a")).await.unwrap();
    assert!(second.cached);
    assert!(!second.reached_provider);
    assert!(second.received.is_empty());
    assert_eq!(second.body, Bytes::from("one"));
    assert_eq!(second.fetched_at_ms, START);
    assert_eq!(server.requests().len(), 1);
    // Expired: fetched again (no validator, so a full request).
    h.clock.advance(1_000);
    let third = h.client.send(cached_get(&server, "/a")).await.unwrap();
    assert!(!third.cached);
    assert_eq!(server.requests().len(), 2);
}

#[tokio::test]
async fn bypass_and_unsafe_methods_skip_the_cache() {
    let h = harness();
    let server = serving(TestResponse::ok("x").header("Cache-Control", "max-age=60")).await;
    h.client.send(get(&server, "/a")).await.unwrap();
    h.client.send(get(&server, "/a")).await.unwrap();
    assert_eq!(server.requests().len(), 2);
    assert!(h.store.is_empty());
    let mut post = cached_get(&server, "/a");
    post.method = Method::Post;
    h.client.send(post.clone()).await.unwrap();
    h.client.send(post).await.unwrap();
    assert_eq!(server.requests().len(), 4);
    assert!(h.store.is_empty());
}

#[tokio::test]
async fn a_successful_unsafe_request_invalidates_its_url() {
    let h = harness();
    let server = serving(TestResponse::ok("x").header("Cache-Control", "max-age=60")).await;
    h.client.send(cached_get(&server, "/item")).await.unwrap();
    assert_eq!(h.store.len(), 1);
    let mut delete = cached_get(&server, "/item");
    delete.method = Method::Delete;
    h.client.send(delete).await.unwrap();
    assert!(h.store.is_empty());
}

#[tokio::test]
async fn expired_responses_are_revalidated_with_their_validators() {
    let h = harness();
    let scripted = Scripted::new(
        TestResponse::ok("body")
            .header("Cache-Control", "max-age=10")
            .header("ETag", "\"v1\"")
            .header("Last-Modified", "Tue, 01 Sep 2026 10:00:00 GMT")
            .header("X-Old", "1"),
    )
    .await;
    let server = &scripted.server;
    h.client.send(cached_get(server, "/a")).await.unwrap();
    h.clock.advance(11_000);
    scripted.answer(
        TestResponse::status(304)
            .header("Cache-Control", "max-age=10")
            .header("Set-Cookie", "rotated=1"),
    );
    let response = h.client.send(cached_get(server, "/a")).await.unwrap();
    let seen = server.requests();
    assert_eq!(seen[1].header("if-none-match"), Some("\"v1\""));
    assert_eq!(
        seen[1].header("if-modified-since"),
        Some("Tue, 01 Sep 2026 10:00:00 GMT")
    );
    assert_eq!(response.status, 200);
    assert_eq!(response.body, Bytes::from("body"));
    assert!(response.cached);
    assert!(response.reached_provider);
    assert_eq!(response.fetched_at_ms, START + 11_000);
    // Only the 304's own headers count as received.
    assert_eq!(
        headers::get(&response.received, "set-cookie"),
        Some("rotated=1")
    );
    assert_eq!(headers::get(&response.received, "x-old"), None);
    assert_eq!(headers::get(&response.headers, "x-old"), Some("1"));
    assert_eq!(
        headers::get(&response.headers, "set-cookie"),
        Some("rotated=1")
    );
    // The revalidation made it fresh again.
    h.clock.advance(9_000);
    assert!(
        h.client
            .send(cached_get(server, "/a"))
            .await
            .unwrap()
            .cached
    );
    assert_eq!(server.requests().len(), 2);
}

#[tokio::test]
async fn a_revalidation_that_adds_vary_selects_by_it_from_then_on() {
    let h = harness();
    let scripted = Scripted::new(
        TestResponse::ok("de")
            .header("Cache-Control", "max-age=10")
            .header("ETag", "\"v1\""),
    )
    .await;
    let with = |language: &str| {
        let mut request = cached_get(&scripted.server, "/a");
        request
            .headers
            .push(("Accept-Language".into(), language.into()));
        request
    };
    h.client.send(with("de")).await.unwrap();
    h.clock.advance(11_000);
    scripted.answer(
        TestResponse::status(304)
            .header("Cache-Control", "max-age=60")
            .header("Vary", "Accept-Language"),
    );
    assert!(h.client.send(with("de")).await.unwrap().cached);
    assert!(h.client.send(with("de")).await.unwrap().cached);
    assert_eq!(scripted.server.requests().len(), 2);
    scripted.answer(TestResponse::ok("fr").header("Cache-Control", "max-age=60"));
    let french = h.client.send(with("fr")).await.unwrap();
    assert!(!french.cached);
    assert_eq!(french.body, Bytes::from("fr"));
}

#[tokio::test]
async fn cached_responses_respect_the_requests_body_bound() {
    let h = harness();
    let scripted = Scripted::new(
        TestResponse::ok(vec![1u8; 100])
            .header("Cache-Control", "max-age=10, stale-if-error=600")
            .header("ETag", "\"v1\""),
    )
    .await;
    let bounded = |max_body: u64| Request {
        max_body,
        ..cached_get(&scripted.server, "/a")
    };
    h.client.send(bounded(100)).await.unwrap();
    // Fresh.
    assert!(matches!(
        h.client.send(bounded(99)).await,
        Err(Error::TooLarge(_))
    ));
    // Revalidated.
    h.clock.advance(11_000);
    scripted.answer(
        TestResponse::status(304).header("Cache-Control", "max-age=10, stale-if-error=600"),
    );
    assert!(matches!(
        h.client.send(bounded(99)).await,
        Err(Error::TooLarge(_))
    ));
    // Stale on error.
    h.clock.advance(11_000);
    scripted.answer(TestResponse::status(500));
    assert!(matches!(
        h.client.send(bounded(99)).await,
        Err(Error::TooLarge(_))
    ));
    assert_eq!(h.client.send(bounded(100)).await.unwrap().body.len(), 100);
}

#[tokio::test]
async fn a_refresh_revalidates_even_fresh_responses() {
    let h = harness();
    let scripted = Scripted::new(
        TestResponse::ok("body")
            .header("Cache-Control", "max-age=600")
            .header("ETag", "\"v1\""),
    )
    .await;
    h.client
        .send(cached_get(&scripted.server, "/a"))
        .await
        .unwrap();
    scripted.answer(TestResponse::status(304));
    let mut refresh = cached_get(&scripted.server, "/a");
    refresh.revalidate = true;
    let response = h.client.send(refresh).await.unwrap();
    assert!(response.cached && response.reached_provider);
    assert_eq!(
        scripted.server.requests()[1].header("if-none-match"),
        Some("\"v1\"")
    );
    // Without validators a refresh is a full request.
    scripted.answer(TestResponse::ok("new").header("Cache-Control", "max-age=600"));
    let plain = serving(TestResponse::ok("plain").header("Cache-Control", "max-age=600")).await;
    h.client.send(cached_get(&plain, "/p")).await.unwrap();
    let mut refresh = cached_get(&plain, "/p");
    refresh.revalidate = true;
    let response = h.client.send(refresh).await.unwrap();
    assert!(!response.cached);
    assert_eq!(plain.requests().len(), 2);
    assert_eq!(plain.requests()[1].header("if-none-match"), None);
}

#[tokio::test]
async fn no_store_and_no_cache_are_honored() {
    let h = harness();
    let server =
        serving(TestResponse::ok("x").header("Cache-Control", "no-store, max-age=60")).await;
    h.client.send(cached_get(&server, "/a")).await.unwrap();
    assert!(h.store.is_empty());
    let server = serving(
        TestResponse::ok("x")
            .header("Cache-Control", "no-cache, max-age=60")
            .header("ETag", "\"e\""),
    )
    .await;
    h.client.send(cached_get(&server, "/a")).await.unwrap();
    h.client.send(cached_get(&server, "/a")).await.unwrap();
    assert_eq!(server.requests()[1].header("if-none-match"), Some("\"e\""));
    // The request's own no-store keeps it out too.
    let store_server = serving(TestResponse::ok("x").header("Cache-Control", "max-age=60")).await;
    let mut request = cached_get(&store_server, "/b");
    request
        .headers
        .push(("Cache-Control".into(), "no-store".into()));
    h.client.send(request).await.unwrap();
    assert!(h.store.keys().iter().all(|key| !key.key.ends_with("/b")));
}

#[tokio::test]
async fn conditional_requests_go_to_the_provider() {
    let h = harness();
    let scripted = Scripted::new(
        TestResponse::ok("v1")
            .header("Cache-Control", "max-age=600")
            .header("ETag", "\"v1\""),
    )
    .await;
    let server = &scripted.server;
    h.client.send(cached_get(server, "/a")).await.unwrap();
    scripted.answer(TestResponse::status(304));
    let mut conditional = cached_get(server, "/a");
    conditional
        .headers
        .push(("If-None-Match".into(), "\"v1\"".into()));
    let response = h.client.send(conditional).await.unwrap();
    assert_eq!(response.status, 304);
    assert!(!response.cached);
    scripted.answer(TestResponse::status(412));
    let mut precondition = cached_get(server, "/a");
    precondition
        .headers
        .push(("If-Match".into(), "\"v0\"".into()));
    assert_eq!(h.client.send(precondition).await.unwrap().status, 412);
    assert_eq!(server.requests().len(), 3);
    // Neither answer replaced the stored response.
    let response = h.client.send(cached_get(server, "/a")).await.unwrap();
    assert!(response.cached);
    assert_eq!(response.body, Bytes::from("v1"));
}

#[tokio::test]
async fn a_request_asking_for_validation_gets_no_stored_stand_in() {
    let h = harness();
    let scripted = Scripted::new(
        TestResponse::ok("cached").header("Cache-Control", "max-age=600, stale-if-error=600"),
    )
    .await;
    let server = &scripted.server;
    h.client.send(cached_get(server, "/a")).await.unwrap();
    for directive in ["no-cache", "max-age=0"] {
        scripted.answer(TestResponse::status(503));
        let mut request = cached_get(server, "/a");
        request
            .headers
            .push(("Cache-Control".into(), directive.into()));
        let response = h.client.send(request).await.unwrap();
        assert_eq!(response.status, 503, "{directive}");
        assert!(!response.cached, "{directive}");
    }
    let request = cached_get(server, "/a");
    drop(scripted);
    let mut request = request;
    request
        .headers
        .push(("Cache-Control".into(), "no-cache".into()));
    assert!(h.client.send(request).await.is_err());
}

#[tokio::test]
async fn vary_selects_the_cached_response() {
    let h = harness();
    let server = TestServer::start(|request: &RecordedRequest| {
        TestResponse::ok(
            request
                .header("accept-language")
                .unwrap_or("none")
                .to_owned(),
        )
        .header("Cache-Control", "max-age=60")
        .header("Vary", "Accept-Language")
    })
    .await
    .unwrap();
    let with = |language: &str| {
        let mut request = cached_get(&server, "/a");
        request
            .headers
            .push(("Accept-Language".into(), language.into()));
        request
    };
    assert_eq!(
        h.client.send(with("de")).await.unwrap().body,
        Bytes::from("de")
    );
    assert!(h.client.send(with("de")).await.unwrap().cached);
    let french = h.client.send(with("fr")).await.unwrap();
    assert!(!french.cached);
    assert_eq!(french.body, Bytes::from("fr"));
}

#[tokio::test]
async fn vary_selects_by_attached_headers_too() {
    struct Cookie(Mutex<String>);
    impl Attach for Cookie {
        fn headers(&self, _: &Url) -> Vec<(String, String)> {
            vec![("Cookie".into(), self.0.lock().unwrap().clone())]
        }
    }
    let h = harness();
    let server = TestServer::start(|request: &RecordedRequest| {
        TestResponse::ok(request.header("cookie").unwrap_or("none").to_owned())
            .header("Cache-Control", "max-age=60")
            .header("Vary", "Cookie")
    })
    .await
    .unwrap();
    let cookie = Arc::new(Cookie(Mutex::new("session=a".into())));
    let request = Request {
        attach: Some(cookie.clone()),
        ..cached_get(&server, "/me")
    };
    assert_eq!(
        h.client.send(request.clone()).await.unwrap().body,
        Bytes::from("session=a")
    );
    assert!(h.client.send(request.clone()).await.unwrap().cached);
    *cookie.0.lock().unwrap() = "session=b".into();
    let rotated = h.client.send(request).await.unwrap();
    assert!(!rotated.cached);
    assert_eq!(rotated.body, Bytes::from("session=b"));
}

#[tokio::test]
async fn range_requests_bypass_the_cache() {
    let h = harness();
    let server = serving(TestResponse::ok("x").header("Cache-Control", "max-age=60")).await;
    let mut request = cached_get(&server, "/a");
    request.headers.push(("Range".into(), "bytes=0-0".into()));
    h.client.send(request.clone()).await.unwrap();
    h.client.send(request).await.unwrap();
    assert_eq!(server.requests().len(), 2);
    assert!(h.store.is_empty());
}

#[tokio::test]
async fn an_override_ignores_the_headers_and_may_replace_the_key() {
    let h = harness();
    let server = serving(TestResponse::ok("x").header("Cache-Control", "no-store")).await;
    h.client
        .send(overridden_get(
            &server,
            "/a?client_id=1",
            60,
            0,
            Some("search:a"),
        ))
        .await
        .unwrap();
    let again = h
        .client
        .send(overridden_get(
            &server,
            "/a?client_id=2",
            60,
            0,
            Some("search:a"),
        ))
        .await
        .unwrap();
    assert!(again.cached);
    assert_eq!(server.requests().len(), 1);
    assert_eq!(h.store.keys()[0].key, "search:a");
    // Failures are never cached under an override.
    let failing = serving(TestResponse::status(404)).await;
    h.client
        .send(overridden_get(&failing, "/missing", 60, 0, None))
        .await
        .unwrap();
    h.client
        .send(overridden_get(&failing, "/missing", 60, 0, None))
        .await
        .unwrap();
    assert_eq!(failing.requests().len(), 2);
}

#[tokio::test]
async fn stale_responses_stand_in_only_when_the_provider_fails() {
    let h = harness();
    let scripted = Scripted::new(TestResponse::ok("cached")).await;
    let server = &scripted.server;
    h.client
        .send(overridden_get(server, "/a", 10, 600, None))
        .await
        .unwrap();
    h.clock.advance(11_000);
    // Reachable: a stale entry is never served, the Provider is asked.
    scripted.answer(TestResponse::ok("new"));
    let response = h
        .client
        .send(overridden_get(server, "/a", 10, 600, None))
        .await
        .unwrap();
    assert_eq!(response.body, Bytes::from("new"));
    assert!(!response.cached);
    h.clock.advance(11_000);
    // A server error: the stale entry stands in.
    scripted.answer(TestResponse::status(500));
    let response = h
        .client
        .send(overridden_get(server, "/a", 10, 600, None))
        .await
        .unwrap();
    assert_eq!(response.body, Bytes::from("new"));
    assert!(response.cached);
    assert!(!response.reached_provider);
    // Rate-limited: too.
    scripted.answer(TestResponse::status(429).header("Retry-After", "60"));
    let response = h
        .client
        .send(overridden_get(server, "/a", 10, 600, None))
        .await
        .unwrap();
    assert!(response.cached);
    // And while the host is held.
    let response = h
        .client
        .send(overridden_get(server, "/a", 10, 600, None))
        .await
        .unwrap();
    assert!(response.cached);
    // Past the stale window the failure is the answer.
    h.clock.advance(600_000);
    assert!(matches!(
        h.client
            .send(overridden_get(server, "/a", 10, 600, None))
            .await,
        Err(Error::RateLimited { .. })
    ));
}

#[tokio::test]
async fn an_unreachable_provider_is_answered_stale() {
    let h = harness();
    let server = serving(
        TestResponse::ok("cached").header("Cache-Control", "max-age=10, stale-if-error=600"),
    )
    .await;
    let request = cached_get(&server, "/a");
    h.client.send(request.clone()).await.unwrap();
    drop(server);
    h.clock.advance(60_000);
    let response = h.client.send(request).await.unwrap();
    assert!(response.cached);
    assert_eq!(response.body, Bytes::from("cached"));
}

#[tokio::test]
async fn nothing_is_served_thirty_days_after_its_fetch() {
    let h = harness();
    let scripted = Scripted::new(TestResponse::ok("old")).await;
    let server = &scripted.server;
    let year = 365 * 24 * 60 * 60;
    h.client
        .send(overridden_get(server, "/a", year, year, None))
        .await
        .unwrap();
    h.clock.advance(cache::MAX_AGE_MS - 1);
    assert!(
        h.client
            .send(overridden_get(server, "/a", year, year, None))
            .await
            .unwrap()
            .cached
    );
    h.clock.advance(1);
    scripted.answer(TestResponse::status(500));
    // Not even stale-if-error.
    let response = h
        .client
        .send(overridden_get(server, "/a", year, year, None))
        .await
        .unwrap();
    assert_eq!(response.status, 500);
    assert!(!response.cached);
}

#[tokio::test]
async fn expired_responses_are_purged_from_the_store() {
    let h = harness();
    let server = serving(TestResponse::ok("x").header("Cache-Control", "max-age=60")).await;
    h.client.send(cached_get(&server, "/old")).await.unwrap();
    h.clock.advance(cache::MAX_AGE_MS);
    h.client.send(cached_get(&server, "/new")).await.unwrap();
    // The write ran the hourly purge, so the old one is gone from disk.
    let keys = h.store.keys();
    assert_eq!(keys.len(), 1);
    assert!(keys[0].key.ends_with("/new"));
    h.clock.advance(cache::MAX_AGE_MS);
    assert_eq!(h.client.purge_expired().await, Ok(1));
    assert!(h.store.is_empty());
}

#[tokio::test]
async fn connections_and_generations_never_share_entries() {
    let h = harness();
    let server = serving(TestResponse::ok("x").header("Cache-Control", "max-age=60")).await;
    for (connection, generation) in [("c1", 1), ("c1", 2), ("c2", 1)] {
        let request = Request {
            cache: CachePolicy::Http(scope(connection, generation)),
            ..get(&server, "/a")
        };
        assert!(!h.client.send(request).await.unwrap().cached);
    }
    assert_eq!(server.requests().len(), 3);
    assert_eq!(
        h.client
            .remove_cached_connection(&PluginId::new("dev.example.test"), &ConnectionId::new("c1"))
            .await,
        Ok(2)
    );
    assert_eq!(
        h.client
            .remove_cached_plugin(&PluginId::new("dev.example.test"))
            .await,
        Ok(1)
    );
}

#[tokio::test]
async fn every_write_keeps_the_store_within_its_budgets() {
    #[derive(Default)]
    struct Spy {
        inner: MemoryCacheStore,
        trims: Mutex<Vec<(PluginId, u64, u64)>>,
    }
    impl HttpCacheStore for Spy {
        fn get(&self, key: &CacheKey) -> Result<Option<StoredResponse>, StoreError> {
            self.inner.get(key)
        }
        fn put(&self, key: &CacheKey, response: &StoredResponse) -> Result<(), StoreError> {
            self.inner.put(key, response)
        }
        fn remove(&self, key: &CacheKey) -> Result<(), StoreError> {
            self.inner.remove(key)
        }
        fn evict_older_than(&self, before: u64) -> Result<u64, StoreError> {
            self.inner.evict_older_than(before)
        }
        fn remove_connection(
            &self,
            plugin: &PluginId,
            connection: &ConnectionId,
        ) -> Result<u64, StoreError> {
            self.inner.remove_connection(plugin, connection)
        }
        fn remove_plugin(&self, plugin: &PluginId) -> Result<u64, StoreError> {
            self.inner.remove_plugin(plugin)
        }
        fn trim(&self, plugin: &PluginId, per_plugin: u64, total: u64) -> Result<u64, StoreError> {
            self.trims
                .lock()
                .unwrap()
                .push((plugin.clone(), per_plugin, total));
            self.inner.trim(plugin, per_plugin, total)
        }
    }
    let spy = Arc::new(Spy::default());
    let client = Client::new(
        ClientConfig::default(),
        Arc::new(ManualClock::new(START)),
        Some(spy.clone()),
    )
    .unwrap();
    let server = serving(TestResponse::ok("x").header("Cache-Control", "max-age=60")).await;
    client.send(cached_get(&server, "/a")).await.unwrap();
    assert_eq!(
        *spy.trims.lock().unwrap(),
        vec![(
            PluginId::new("dev.example.test"),
            cache::PLUGIN_BUDGET_BYTES,
            cache::TOTAL_BUDGET_BYTES
        )]
    );
}

#[test]
fn the_memory_store_evicts_least_recently_used_first() {
    let store = MemoryCacheStore::new();
    let stored = |size: usize| StoredResponse {
        status: 200,
        headers: Vec::new(),
        body: vec![0; size],
        url: String::new(),
        fetched_at_ms: 0,
        fresh_until_ms: 0,
        stale_until_ms: 0,
        vary: Vec::new(),
    };
    let key = |plugin: &str, name: &str| CacheKey {
        scope: CacheScope {
            plugin: PluginId::new(plugin),
            connection: ConnectionId::new(""),
            generation: 0,
        },
        method: CacheMethod::Get,
        key: name.to_owned(),
    };
    store.put(&key("a", "1"), &stored(100)).unwrap();
    store.put(&key("a", "2"), &stored(100)).unwrap();
    store.put(&key("b", "1"), &stored(100)).unwrap();
    store.get(&key("a", "1")).unwrap();
    assert_eq!(store.trim(&PluginId::new("a"), 150, 1_000), Ok(1));
    assert!(store.peek(&key("a", "2")).is_none());
    assert!(store.peek(&key("a", "1")).is_some());
    assert_eq!(store.trim(&PluginId::new("a"), 1_000, 150), Ok(1));
    assert!(store.peek(&key("b", "1")).is_none());
}

// Streaming -----------------------------------------------------------------

#[tokio::test]
async fn streams_deliver_chunks_as_they_arrive() {
    let h = harness();
    let server = serving(
        TestResponse {
            status: 206,
            chunks: Some(vec![Bytes::from("ab"), Bytes::from("cd")]),
            chunk_delay: Duration::from_millis(20),
            ..TestResponse::ok("")
        }
        .header("Content-Range", "bytes 10-13/1000"),
    )
    .await;
    let mut request = get(&server, "/media");
    request.headers.push(("Range".into(), "bytes=10-13".into()));
    let mut stream = h.client.stream(request).await.unwrap();
    assert_eq!(stream.status, 206);
    assert_eq!(stream.total_length(), Some(1000));
    assert_eq!(stream.chunk().await.unwrap(), Some(Bytes::from("ab")));
    assert_eq!(stream.chunk().await.unwrap(), Some(Bytes::from("cd")));
    assert_eq!(stream.chunk().await.unwrap(), None);
    assert_eq!(server.requests()[0].header("range"), Some("bytes=10-13"));

    let whole = serving(TestResponse::ok("whole")).await;
    let stream = h.client.stream(get(&whole, "/")).await.unwrap();
    assert_eq!(stream.total_length(), Some(5));
}

#[tokio::test]
async fn validators_go_only_to_the_resource_that_supplied_them() {
    let h = harness();
    let server = TestServer::start(|request| match request.path.as_str() {
        "/a" => TestResponse::status(302).header("Location", "/b"),
        _ if request.header("if-none-match").is_some() => TestResponse::status(304),
        _ => TestResponse::ok("b")
            .header("ETag", "\"e\"")
            .header("Cache-Control", "no-cache"),
    })
    .await
    .unwrap();
    h.client
        .send(overridden_get(&server, "/a", 0, 0, None))
        .await
        .unwrap();
    let response = h
        .client
        .send(overridden_get(&server, "/a", 0, 0, None))
        .await
        .unwrap();
    assert_eq!(response.body, Bytes::from("b"));
    assert!(response.cached);
    let seen = server.requests();
    assert_eq!(seen[2].path, "/a");
    assert_eq!(
        seen[2].header("if-none-match"),
        None,
        "sent to the redirecting URL"
    );
    assert_eq!(seen[3].path, "/b");
    assert_eq!(seen[3].header("if-none-match"), Some("\"e\""));
}

#[tokio::test]
async fn a_304_from_another_resource_is_no_revalidation() {
    let h = harness();
    let redirect = Arc::new(Mutex::new(false));
    let toggle = redirect.clone();
    let server = TestServer::start(move |request| {
        match (request.path.as_str(), *toggle.lock().unwrap()) {
            // /a first answers itself, later redirects to /b, which answers
            // 304 whatever it is asked.
            ("/a", false) => TestResponse::ok("a")
                .header("ETag", "\"e\"")
                .header("Cache-Control", "no-cache"),
            ("/a", true) => TestResponse::status(302).header("Location", "/b"),
            _ => TestResponse::status(304),
        }
    })
    .await
    .unwrap();
    h.client.send(cached_get(&server, "/a")).await.unwrap();
    *redirect.lock().unwrap() = true;
    let response = h.client.send(cached_get(&server, "/a")).await.unwrap();
    // The validators were for /a; /b's 304 is passed on, not merged.
    assert_eq!(response.status, 304);
    assert!(!response.cached);
}

#[tokio::test]
async fn an_unsafe_request_invalidates_its_redirect_target() {
    let h = harness();
    let server =
        TestServer::start(
            |request| match (request.method.as_str(), request.path.as_str()) {
                ("DELETE", "/old-item") => TestResponse::status(307).header("Location", "/item"),
                ("DELETE", _) => TestResponse::status(204),
                _ => TestResponse::ok("item").header("Cache-Control", "max-age=600"),
            },
        )
        .await
        .unwrap();
    h.client.send(cached_get(&server, "/item")).await.unwrap();
    assert_eq!(h.store.len(), 1);
    let mut delete = cached_get(&server, "/old-item");
    delete.method = Method::Delete;
    assert_eq!(h.client.send(delete).await.unwrap().status, 204);
    assert!(h.store.is_empty());
}

#[tokio::test]
async fn vary_through_a_redirect_selects_by_the_final_hops_headers() {
    struct CookieForB(Mutex<String>);
    impl Attach for CookieForB {
        fn headers(&self, url: &Url) -> Vec<(String, String)> {
            if url.path() == "/b" {
                vec![("Cookie".into(), self.0.lock().unwrap().clone())]
            } else {
                Vec::new()
            }
        }
    }
    let h = harness();
    let server = TestServer::start(|request| match request.path.as_str() {
        "/a" => TestResponse::status(302).header("Location", "/b"),
        _ => TestResponse::ok(request.header("cookie").unwrap_or("none").to_owned())
            .header("Cache-Control", "max-age=60")
            .header("Vary", "Cookie"),
    })
    .await
    .unwrap();
    let cookie = Arc::new(CookieForB(Mutex::new("b=1".into())));
    let request = Request {
        attach: Some(cookie.clone()),
        ..cached_get(&server, "/a")
    };
    assert_eq!(
        h.client.send(request.clone()).await.unwrap().body,
        Bytes::from("b=1")
    );
    // Reached through a redirect, it is not stored under HTTP rules.
    assert!(!h.client.send(request.clone()).await.unwrap().cached);
    *cookie.0.lock().unwrap() = "b=2".into();
    let rotated = h.client.send(request).await.unwrap();
    assert!(!rotated.cached);
    assert_eq!(rotated.body, Bytes::from("b=2"));
}

#[tokio::test]
async fn a_mutation_invalidates_even_when_its_body_breaks_off() {
    let h = harness();
    let server = TestServer::start(|request| match request.method.as_str() {
        "PUT" => TestResponse {
            truncate: true,
            ..TestResponse::ok("partial")
        },
        _ => TestResponse::ok("item").header("Cache-Control", "max-age=600"),
    })
    .await
    .unwrap();
    h.client.send(cached_get(&server, "/item")).await.unwrap();
    assert_eq!(h.store.len(), 1);
    let mut put = cached_get(&server, "/item");
    put.method = Method::Put;
    assert!(h.client.send(put).await.is_err());
    assert!(h.store.is_empty());
}

#[tokio::test]
async fn a_304_cannot_grow_the_headers_past_their_bounds() {
    let h = harness();
    let scripted = {
        let mut first = TestResponse::ok("x")
            .header("Cache-Control", "no-cache")
            .header("ETag", "\"e\"");
        for index in 0..150 {
            first = first.header(&format!("x-a-{index}"), "v");
        }
        Scripted::new(first).await
    };
    h.client
        .send(cached_get(&scripted.server, "/a"))
        .await
        .unwrap();
    let mut second = TestResponse::status(304);
    for index in 0..100 {
        second = second.header(&format!("x-b-{index}"), "v");
    }
    scripted.answer(second);
    assert!(matches!(
        h.client.send(cached_get(&scripted.server, "/a")).await,
        Err(Error::TooLarge(_))
    ));
}

#[tokio::test]
async fn a_redirect_is_not_answered_from_the_cache_by_its_target_freshness() {
    let h = harness();
    let target = Arc::new(Mutex::new("/item-a"));
    let current = target.clone();
    let server = TestServer::start(move |request| match request.path.as_str() {
        "/current" => TestResponse::status(302).header("Location", *current.lock().unwrap()),
        path => TestResponse::ok(path.to_owned()).header("Cache-Control", "max-age=3600"),
    })
    .await
    .unwrap();
    assert_eq!(
        h.client
            .send(cached_get(&server, "/current"))
            .await
            .unwrap()
            .body,
        Bytes::from("/item-a")
    );
    *target.lock().unwrap() = "/item-b";
    let again = h
        .client
        .send(cached_get(&server, "/current"))
        .await
        .unwrap();
    assert_eq!(again.body, Bytes::from("/item-b"));
    assert!(!again.cached);
}

#[tokio::test]
async fn a_mutation_that_redirects_to_a_failure_still_invalidates() {
    let h = harness();
    let server =
        TestServer::start(
            |request| match (request.method.as_str(), request.path.as_str()) {
                ("POST", "/item") => TestResponse::status(303).header("Location", "/result"),
                (_, "/result") => TestResponse::status(500),
                _ => TestResponse::ok("item").header("Cache-Control", "max-age=600"),
            },
        )
        .await
        .unwrap();
    h.client.send(cached_get(&server, "/item")).await.unwrap();
    assert_eq!(h.store.len(), 1);
    let mut post = cached_get(&server, "/item");
    post.method = Method::Post;
    assert_eq!(h.client.send(post).await.unwrap().status, 500);
    assert!(h.store.is_empty());
}

// Attach query and shared budgets ------------------------------------------

#[tokio::test]
async fn attached_query_parameters_go_to_each_hop_but_not_the_cache_key() {
    struct Token;
    impl Attach for Token {
        fn headers(&self, _: &Url) -> Vec<(String, String)> {
            Vec::new()
        }
        fn query(&self, _: &Url) -> Vec<(String, String)> {
            vec![("client_id".into(), "secret id".into())]
        }
    }
    let h = harness();
    let server = TestServer::start(|request| match request.path.split('?').next() {
        Some("/old") => TestResponse::status(302).header("Location", "/new?page=2"),
        _ => TestResponse::ok("x").header("Cache-Control", "max-age=60"),
    })
    .await
    .unwrap();
    let request = Request {
        attach: Some(Arc::new(Token)),
        ..cached_get(&server, "/old?q=1")
    };
    let response = h.client.send(request).await.unwrap();
    assert_eq!(response.url.as_str(), server.url("/new?page=2").as_str());
    let seen = server.requests();
    assert_eq!(seen[0].path, "/old?q=1&client_id=secret+id");
    assert_eq!(seen[1].path, "/new?page=2&client_id=secret+id");
    assert!(
        h.store
            .keys()
            .iter()
            .all(|key| !key.key.contains("client_id"))
    );
}

#[tokio::test]
async fn a_shared_budget_bounds_every_body_of_a_call() {
    let h = harness();
    let server =
        serving(TestResponse::ok(vec![1u8; 600]).header("Cache-Control", "max-age=60")).await;
    let budget = ristle_net::BodyBudget::new(1_000);
    let with_budget = |path: &str| Request {
        budget: Some(budget.clone()),
        ..cached_get(&server, path)
    };
    assert_eq!(
        h.client.send(with_budget("/a")).await.unwrap().body.len(),
        600
    );
    assert_eq!(budget.remaining(), 400);
    assert!(matches!(
        h.client.send(with_budget("/b")).await,
        Err(Error::BudgetSpent)
    ));
    // A cached body counts too.
    let fresh = ristle_net::BodyBudget::new(500);
    let cached = Request {
        budget: Some(fresh.clone()),
        ..cached_get(&server, "/a")
    };
    assert!(matches!(
        h.client.send(cached).await,
        Err(Error::BudgetSpent)
    ));
}

#[tokio::test]
async fn concurrent_bodies_reserve_their_declared_length() {
    let h = harness();
    let server = serving(TestResponse {
        delay: Duration::from_millis(50),
        ..TestResponse::ok(vec![1u8; 600])
    })
    .await;
    let budget = ristle_net::BodyBudget::new(1_000);
    let request = |path: &str| Request {
        budget: Some(budget.clone()),
        ..get(&server, path)
    };
    let (a, b) = tokio::join!(h.client.send(request("/a")), h.client.send(request("/b")));
    let kept = [&a, &b].iter().filter(|result| result.is_ok()).count();
    assert_eq!(kept, 1, "{a:?} {b:?}");
    assert!(
        [a, b]
            .into_iter()
            .any(|result| result == Err(Error::BudgetSpent))
    );
}

#[tokio::test]
async fn sockets_reach_public_addresses_or_what_a_grant_confirms() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let h = harness();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = [0u8; 64];
                let read = stream.read(&mut buffer).await.unwrap_or(0);
                let _ = stream.write_all(&buffer[..read]).await;
            });
        }
    });
    assert!(matches!(
        h.client
            .connect_socket("127.0.0.1", port, false, None)
            .await,
        Err(Error::AddressRefused { granted: false, .. })
    ));
    assert!(matches!(
        h.client
            .connect_socket("localhost", port, false, None)
            .await,
        Err(Error::AddressRefused { .. })
    ));
    let origin = GrantedOrigin {
        scheme: ristle_net::Scheme::Http,
        host: "127.0.0.1".into(),
        port,
        public: false,
        confirmed: vec!["127.0.0.1".parse().unwrap()],
    };
    let mut socket = h
        .client
        .connect_socket("127.0.0.1", port, false, Some(&origin))
        .await
        .unwrap();
    socket.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    socket.read_exact(&mut echoed).await.unwrap();
    assert_eq!(&echoed, b"ping");
}
