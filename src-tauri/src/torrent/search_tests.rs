use super::*;
use serde_json::json;

const HASH: &str = "1111111111111111111111111111111111111111";
const OTHER_HASH: &str = "2222222222222222222222222222222222222222";

#[test]
fn structured_identity_distinguishes_numeric_titles_artists_and_versions() {
    for (artist, title, candidate, matches) in [
        ("The Beatles", "1", "The Beatles - 1 [FLAC]", true),
        ("The Beatles", "1", "The Beatles - 1967–1970", false),
        ("The Beatles", "1", "The Beatles - 1 Love", false),
        ("X", "Los Angeles", "X - Los Angeles (1980) [FLAC]", true),
        ("X", "Los Angeles", "X Japan - Los Angeles", false),
        ("Björk", "Vespertine", "bjork.vespertine.2001.flac", true),
        ("宇多田ヒカル", "光", "宇多田ヒカル - 光 [FLAC]", true),
        ("Artist", "Song", "Artist - Song (Live)", false),
        ("Artist", "Song (Live)", "Artist - Song (Live) [FLAC]", true),
        ("Artist", "Song (Live)", "Artist - Song [FLAC]", false),
        ("Artist", "Song", "Artist - Song (Champion Remix)", false),
        (
            "Black Rebel Motorcycle Club",
            "B.R.M.C.",
            "Black Rebel Motorcycle Club - B.R.M.C. [2001] [FLAC]",
            true,
        ),
    ] {
        let query = Query::new(artist, title).unwrap();
        assert_eq!(
            query.identity(candidate, &[]).is_some(),
            matches,
            "{artist} / {title} against {candidate}"
        );
    }
}

#[test]
fn archive_search_escapes_lucene_and_http_parameters_once() {
    let query = Query::new("A+B", "One (Live) & Two").unwrap();
    let client = reqwest::Client::new();
    let request = providers::archive::search_request(&client, &query)
        .build()
        .unwrap();
    let parameters = request.url().query_pairs().collect::<Vec<_>>();
    assert!(parameters.iter().any(|(key, value)| {
        key == "q"
            && value == r#"mediatype:audio AND ((creator:"A\+B" AND title:"One \(Live\) & Two") OR title:"A\+B One \(Live\) & Two")"#
    }));
    assert!(parameters
        .iter()
        .any(|(key, value)| key == "fl[]" && value == "identifier"));
}

#[test]
fn archive_verifies_the_exact_torrent_and_keeps_popularity_separate() {
    let rows = json!({"response": {"docs": [
        {"identifier":"../bad", "creator":"X", "title":"Los Angeles"},
        {"identifier":"x-los-angeles", "creator":[null, "X"], "title":["Los Angeles"], "downloads":"99", "item_size":12345},
        {"identifier":"x-los-angeles-other", "creator":"X", "title":"Los Angeles"}
    ]}});
    let query = Query::new("X", "Los Angeles").unwrap();
    let hits = providers::archive::parse_search(&rows, &query).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].title, "X – Los Angeles");
    assert_eq!(hits[0].seeders, None);
    assert_eq!(hits[0].popularity, Some(99));
    assert_eq!(hits[0].size_bytes, Some(12345));
    assert!(providers::archive::has_torrent(
        &json!({"files":[{"name":"x-los-angeles_archive.torrent"}]}),
        "x-los-angeles"
    )
    .unwrap());
    assert!(!providers::archive::has_torrent(
        &json!({"files":[{"name":"other_archive.torrent"}]}),
        "x-los-angeles"
    )
    .unwrap());
    assert!(!providers::archive::has_torrent(&json!([]), "x-los-angeles").unwrap());
    assert!(matches!(
        providers::archive::has_torrent(&json!({"error":"some upstream failure"}), "x-los-angeles"),
        Err(ProviderError::Unavailable)
    ));
}

#[test]
fn knaben_accepts_magnet_only_and_hash_only_but_rejects_conflicting_identity() {
    let magnet = format!("magnet:?xt=urn:btih:{HASH}");
    let rows = json!({"hits": [
        {"title":"Artist - Song [FLAC]", "magnetUrl":magnet, "bytes":"152", "seeders":0},
        {"title":"Artist - Song [MP3]", "infoHash":OTHER_HASH, "seeders":"14"},
        {"title":"Artist - Song", "magnetUrl":magnet, "hash":OTHER_HASH},
        {"title":"Artist - Song", "hash":HASH, "infoHash":OTHER_HASH},
        {"title":"Artist - Song", "hash":"garbage"},
        {"name":"wrong property"}
    ]});
    let hits = providers::knaben::parse(&rows, &Query::new("Artist", "Song").unwrap()).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, format!("btih:{OTHER_HASH}"));
    assert_eq!(hits[0].seeders, Some(14));
    assert_eq!(hits[1].id, format!("btih:{HASH}"));
    assert_eq!(hits[1].seeders, Some(0));
    assert_eq!(hits[1].size_bytes, Some(152));
}

#[test]
fn knaben_keeps_a_document_source_without_passing_along_tracker_parameters() {
    let query = Query::new("Artist", "Song").unwrap();
    let magnet = format!("magnet:?xt=urn:btih:{HASH}&tr=https%3A%2F%2Fexample.org%2Fprivate&xs=https%3A%2F%2Fexample.org%2Fsong.torrent&xs=https%3A%2F%2Fexample.org%2Fsong.torrent%3Ftoken%3Dsecret");
    let hits = providers::knaben::parse(
        &json!({"hits":[
            {"title":"Artist - Song", "magnetUrl":magnet}
        ]}),
        &query,
    )
    .unwrap();
    let output = url::Url::parse(&hits[0].input).unwrap();
    assert_eq!(
        output.query_pairs().into_owned().collect::<Vec<_>>(),
        [
            ("xt".to_string(), format!("urn:btih:{HASH}")),
            ("xs".to_string(), "https://example.org/song.torrent".into()),
        ]
    );
}

#[test]
fn knaben_uses_a_validated_download_link_when_the_index_has_no_hash() {
    let query = Query::new("Artist", "Song").unwrap();
    let link = "https://knaben.eu/live/dl/indexer/?path=opaque-catalog-key&file=Song";
    let hits = providers::knaben::parse(&json!({"hits":[
        {"title":"Artist - Song", "hash":null, "magnetUrl":null, "link":link},
        {"title":"Artist - Song", "link":"https://other.example/live/dl/indexer/?path=unknown"},
        {"title":"Artist - Song", "link":"https://user@knaben.eu/live/dl/indexer/?path=unknown"},
        {"title":"Artist - Song", "link":"https://knaben.eu/live/dl/indexer/?path=opaque-catalog-key&file=Song"},
    ]}), &query).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].input, link);
    assert!(hits[0].id.starts_with("knaben:"));
    assert!(!hits[0].id.contains("opaque-catalog-key"));
    assert!(hits[0].info_hash.is_none());
}

#[test]
fn apibay_distinguishes_a_sentinel_from_an_invalid_response() {
    let zero_hash = "0".repeat(40);
    let query = Query::new("Artist", "Song").unwrap();
    let empty = json!([{"id":"0", "name":"No results returned", "info_hash": zero_hash}]);
    assert!(providers::apibay::parse(&empty, &query).unwrap().is_empty());
    let rows = json!([
        {"id":"123", "name":"Artist - Song", "info_hash":HASH, "size":42, "seeders":"0"},
        {"id":124, "name":"Artist - Song", "info_hash":OTHER_HASH, "size":"43"}
    ]);
    let hits = providers::apibay::parse(&rows, &query).unwrap();
    assert_eq!(hits[0].size_bytes, Some(42));
    assert_eq!(hits[0].seeders, Some(0));
    assert_eq!(hits[1].size_bytes, Some(43));
    assert_eq!(hits[1].seeders, None);
    let invalid = json!([{"id":"123", "info_hash":false}]);
    assert!(matches!(
        providers::apibay::parse(&invalid, &query),
        Err(ProviderError::Malformed)
    ));
}

#[tokio::test]
async fn progress_is_immediate_and_final_deduplication_ignores_completion_order() {
    use std::sync::Mutex;
    use tokio::sync::oneshot;

    async fn collect_order(first: SearchHit, second: SearchHit) -> SearchHit {
        let provider = first.provider.clone();
        let (release_second, wait_for_first) = oneshot::channel();
        let release_second = Mutex::new(Some(release_second));
        let mut jobs = JoinSet::new();
        jobs.spawn(async move { ProviderReport::from_hits(first.provider.clone(), vec![first]) });
        jobs.spawn(async move {
            wait_for_first.await.unwrap();
            ProviderReport::from_hits(second.provider.clone(), vec![second])
        });
        let report = tokio::time::timeout(
            Duration::from_secs(2),
            collect(jobs, |event| {
                if let Some(send) = release_second.lock().unwrap().take() {
                    assert_eq!(event.provider, provider);
                    assert_eq!(event.hits[0].id, format!("btih:{HASH}"));
                    send.send(()).unwrap();
                }
            }),
        )
        .await
        .expect("the first result must arrive before the second job can finish");
        assert_eq!(report.providers.len(), 2);
        assert_eq!(report.hits.len(), 1);
        report.hits.into_iter().next().unwrap()
    }

    let query = Query::new("Artist", "Song").unwrap();
    let knaben = providers::knaben::parse(
        &json!({"hits":[
            {"title":"Artist - Song [FLAC]", "hash":HASH, "seeders":3}
        ]}),
        &query,
    )
    .unwrap()
    .remove(0);
    let apibay = providers::apibay::parse(
        &json!([
            {"id":"123", "name":"Artist - Song", "info_hash":HASH, "seeders":"8"}
        ]),
        &query,
    )
    .unwrap()
    .remove(0);

    let later_apibay = collect_order(knaben.clone(), apibay.clone()).await;
    let later_knaben = collect_order(apibay, knaben).await;
    for hit in [later_apibay, later_knaben] {
        assert_eq!(hit.provider, "ApiBay");
        assert_eq!(hit.providers, ["ApiBay", "Knaben"]);
        assert_eq!(hit.seeders, Some(8));
    }
}

fn serve_http(responses: Vec<String>) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/search", listener.local_addr().unwrap());
    let task = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        for response in responses {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "missing local HTTP request"
                        );
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("could not accept local HTTP request: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = BufReader::new(&stream);
            loop {
                let mut line = String::new();
                assert_ne!(request.read_line(&mut line).unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
            }
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (url, task)
}

fn wire_response(status: &str, extra_headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n{extra_headers}\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn transport_respects_retry_after_within_the_request_budget() {
    let client = Client::builder().no_proxy().build().unwrap();
    let gate = providers::Gate::new();
    let date = httpdate::fmt_http_date(std::time::SystemTime::now() - Duration::from_secs(1));
    let (url, server) = serve_http(vec![
        wire_response(
            "429 Too Many Requests",
            &format!("Retry-After: {date}\r\n"),
            "",
        ),
        wire_response("200 OK", "", r#"{"hits":[]}"#),
    ]);
    let response = gate
        .json(
            client.get(&url),
            tokio::time::Instant::now() + Duration::from_secs(3),
        )
        .await
        .unwrap();
    assert_eq!(response, json!({"hits":[]}));
    server.join().unwrap();

    let gate = providers::Gate::new();
    let (url, server) = serve_http(vec![wire_response(
        "429 Too Many Requests",
        "Retry-After: 60\r\n",
        "",
    )]);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    assert_eq!(
        gate.json(client.get(&url), deadline).await,
        Err(ProviderError::RateLimited)
    );
    server.join().unwrap();
    assert_eq!(
        gate.json(client.get(&url), deadline).await,
        Err(ProviderError::RateLimited)
    );
}

#[tokio::test]
async fn transport_reports_http_failures_and_rejects_malformed_or_oversized_bodies() {
    let client = Client::builder().no_proxy().build().unwrap();
    for (response, expected) in [
        (
            wire_response("503 Service Unavailable", "", ""),
            ProviderError::Unavailable,
        ),
        (
            wire_response("200 OK", "", "<!doctype html>"),
            ProviderError::Malformed,
        ),
        (
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 40\r\n\r\n{}".to_string(),
            ProviderError::Malformed,
        ),
        (
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 5000000\r\n\r\n".to_string(),
            ProviderError::ResponseTooLarge,
        ),
    ] {
        let (url, server) = serve_http(vec![response]);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        assert_eq!(
            providers::Gate::new()
                .json(client.get(&url), deadline)
                .await,
            Err(expected)
        );
        server.join().unwrap();
    }
}
