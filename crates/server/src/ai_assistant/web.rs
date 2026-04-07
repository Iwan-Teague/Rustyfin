use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use futures::StreamExt;
use reqwest::header;
use reqwest::{Response, Url, redirect};
use scraper::{Html, Selector};
use serde::Serialize;
use tokio::net::lookup_host;

pub const AI_PUBLIC_WEB_ENABLE_ENV: &str = "RUSTFIN_AI_PUBLIC_WEB_ENABLED";

const PUBLIC_WEB_USER_AGENT: &str = concat!("Rustyfin-AI-WebFetch/", env!("CARGO_PKG_VERSION"));
const PUBLIC_WEB_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const PUBLIC_WEB_REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const PUBLIC_WEB_MAX_REDIRECTS: usize = 4;
const PUBLIC_WEB_MAX_BODY_BYTES: usize = 64 * 1024;
const PUBLIC_WEB_MAX_TEXT_CHARS: usize = 1_200;
const PUBLIC_WEB_MAX_QUERY_CHARS: usize = 160;
const PUBLIC_WEB_SEARCH_RESULT_LIMIT: usize = 5;
const DUCKDUCKGO_HTML_SEARCH_URL: &str = "https://html.duckduckgo.com/html/";

#[derive(Debug, Clone, Serialize)]
pub struct PublicWebSearchResult {
    pub title: String,
    pub url: String,
    pub source_host: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicWebPageSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub requested_url: String,
    pub final_url: String,
    pub source_host: String,
    pub page_title: Option<String>,
    pub summary: String,
    pub content_type: String,
}

struct FetchedPublicText {
    requested_url: String,
    final_url: String,
    source_host: String,
    content_type: String,
    body: String,
}

pub fn public_web_tools_enabled() -> bool {
    std::env::var(AI_PUBLIC_WEB_ENABLE_ENV)
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

pub async fn search_public_web(
    query: &str,
    limit: Option<usize>,
) -> Result<Vec<PublicWebSearchResult>, String> {
    let query = normalize_search_query(query)?;
    let limit = limit
        .unwrap_or(PUBLIC_WEB_SEARCH_RESULT_LIMIT)
        .clamp(1, PUBLIC_WEB_SEARCH_RESULT_LIMIT);
    let search_url = Url::parse_with_params(DUCKDUCKGO_HTML_SEARCH_URL, &[("q", query.as_str())])
        .map_err(|error| format!("failed to build public web search URL: {error}"))?;
    let fetched = fetch_public_text_url(search_url.as_str()).await?;
    let results = parse_duckduckgo_results(&fetched.body, limit);
    if results.is_empty() {
        return Err("public web search returned no parseable results".to_string());
    }
    Ok(results)
}

pub async fn fetch_public_page_summary(raw_url: &str) -> Result<PublicWebPageSummary, String> {
    let fetched = fetch_public_text_url(raw_url).await?;
    let page_title = extract_page_title(&fetched.body);
    let summary = summarize_page_body(&fetched.body, &fetched.content_type);
    if summary.is_empty() {
        return Err("public page did not contain extractable text".to_string());
    }
    Ok(PublicWebPageSummary {
        category: None,
        requested_url: fetched.requested_url,
        final_url: fetched.final_url,
        source_host: fetched.source_host,
        page_title,
        summary,
        content_type: fetched.content_type,
    })
}

async fn fetch_public_text_url(raw_url: &str) -> Result<FetchedPublicText, String> {
    let client = reqwest::Client::builder()
        .redirect(redirect::Policy::none())
        .connect_timeout(PUBLIC_WEB_CONNECT_TIMEOUT)
        .timeout(PUBLIC_WEB_REQUEST_TIMEOUT)
        .user_agent(PUBLIC_WEB_USER_AGENT)
        .build()
        .map_err(|error| format!("failed to build public web client: {error}"))?;

    let mut current = normalize_public_url(raw_url)?;
    let requested_url = current.to_string();

    for _ in 0..PUBLIC_WEB_MAX_REDIRECTS {
        validate_public_target(&current).await?;
        let response = client
            .get(current.clone())
            .header(
                header::ACCEPT,
                "text/html,application/xhtml+xml,text/plain;q=0.9",
            )
            .header(header::ACCEPT_LANGUAGE, "en-US,en;q=0.8")
            .send()
            .await
            .map_err(|error| format!("public web request failed: {error}"))?;

        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| "public web redirect missing location header".to_string())?;
            current = current
                .join(location)
                .map_err(|error| format!("invalid public web redirect location: {error}"))?;
            continue;
        }

        if !response.status().is_success() {
            return Err(format!(
                "public web request failed with status {}",
                response.status()
            ));
        }
        return finalize_public_text_response(requested_url.clone(), response).await;
    }

    Err("public web request exceeded redirect limit".to_string())
}

async fn finalize_public_text_response(
    requested_url: String,
    response: Response,
) -> Result<FetchedPublicText, String> {
    let final_url = response.url().clone();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    if !is_supported_text_content_type(&content_type) {
        return Err(format!(
            "public web response content type is not supported: {content_type}"
        ));
    }
    let body = read_limited_text_body(response).await?;
    let source_host = final_url
        .host_str()
        .unwrap_or("unknown")
        .trim()
        .to_ascii_lowercase();
    Ok(FetchedPublicText {
        requested_url,
        final_url: final_url.to_string(),
        source_host,
        content_type,
        body,
    })
}

pub(crate) fn normalize_public_url(raw_url: &str) -> Result<Url, String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return Err("public web URL is required".to_string());
    }
    let url = Url::parse(trimmed).map_err(|error| format!("invalid public web URL: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("public web tools only allow http and https URLs".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("public web tools do not allow credentialed URLs".to_string());
    }
    if let Some(port) = url.port() {
        if port != 80 && port != 443 {
            return Err("public web tools only allow default public web ports".to_string());
        }
    }
    Ok(url)
}

async fn validate_public_target(url: &Url) -> Result<(), String> {
    let host = url
        .host_str()
        .ok_or_else(|| "public web URL is missing a host".to_string())?;
    let host_lower = host.trim().to_ascii_lowercase();
    if is_blocked_hostname(&host_lower) {
        return Err("public web target is not allowed".to_string());
    }

    let port = url.port_or_known_default().unwrap_or(443);
    let addresses = resolve_host_addresses(&host_lower, port).await?;
    if addresses.is_empty() {
        return Err("public web target did not resolve to an address".to_string());
    }
    if addresses.iter().any(|ip| !is_public_ip(*ip)) {
        return Err("public web target resolved to a private or reserved address".to_string());
    }
    Ok(())
}

async fn resolve_host_addresses(host: &str, port: u16) -> Result<Vec<IpAddr>, String> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }
    let addresses = lookup_host((host, port))
        .await
        .map_err(|error| format!("failed to resolve public web host: {error}"))?
        .map(|addr| addr.ip())
        .collect::<Vec<_>>();
    Ok(addresses)
}

fn is_blocked_hostname(host: &str) -> bool {
    host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host == "host.docker.internal"
        || host == "metadata.google.internal"
        || host == "169.254.169.254"
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => is_public_ipv4(ipv4),
        IpAddr::V6(ipv6) => is_public_ipv6(ipv6),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    !ip.is_private()
        && !ip.is_loopback()
        && !ip.is_link_local()
        && !ip.is_multicast()
        && !ip.is_broadcast()
        && !ip.is_documentation()
        && !ip.is_unspecified()
        && !is_ipv4_shared(ip)
}

fn is_ipv4_shared(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !ip.is_unique_local()
        && !ip.is_unicast_link_local()
        && !is_ipv6_documentation(ip)
}

fn is_ipv6_documentation(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    segments[0] == 0x2001 && segments[1] == 0x0db8
}

fn is_supported_text_content_type(content_type: &str) -> bool {
    let lower = content_type.to_ascii_lowercase();
    lower.starts_with("text/html")
        || lower.starts_with("application/xhtml+xml")
        || lower.starts_with("text/plain")
}

async fn read_limited_text_body(response: Response) -> Result<String, String> {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|error| format!("failed to read public web response: {error}"))?;
        if body.len() + chunk.len() > PUBLIC_WEB_MAX_BODY_BYTES {
            return Err("public web response exceeded size limit".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&body).to_string())
}

fn normalize_search_query(raw_query: &str) -> Result<String, String> {
    let query = normalize_text(raw_query);
    if query.len() < 3 {
        return Err("public web search query must be at least 3 characters".to_string());
    }
    if query.chars().count() > PUBLIC_WEB_MAX_QUERY_CHARS {
        return Err(format!(
            "public web search query must be <= {PUBLIC_WEB_MAX_QUERY_CHARS} characters"
        ));
    }
    Ok(query)
}

fn parse_duckduckgo_results(search_html: &str, limit: usize) -> Vec<PublicWebSearchResult> {
    let document = Html::parse_document(search_html);
    let result_selector = Selector::parse(".result").ok();
    let title_selector = Selector::parse("a.result__a").ok();
    let snippet_selector = Selector::parse(".result__snippet").ok();

    let mut results = Vec::new();
    if let (Some(result_selector), Some(title_selector)) = (&result_selector, &title_selector) {
        for result in document.select(result_selector).take(limit) {
            let Some(title_link) = result.select(title_selector).next() else {
                continue;
            };
            let Some(href) = title_link.value().attr("href") else {
                continue;
            };
            let Some(target_url) = normalize_search_result_url(href) else {
                continue;
            };
            let Some(source_host) = source_host_for_url(&target_url) else {
                continue;
            };
            let title = normalize_text(&title_link.text().collect::<Vec<_>>().join(" "));
            let snippet = snippet_selector
                .as_ref()
                .and_then(|selector| result.select(selector).next())
                .map(|node| normalize_text(&node.text().collect::<Vec<_>>().join(" ")))
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            results.push(PublicWebSearchResult {
                title,
                url: target_url,
                source_host,
                snippet: truncate_chars(&snippet, 240),
            });
        }
    }

    results
}

fn normalize_search_result_url(raw_href: &str) -> Option<String> {
    let href = raw_href.trim();
    if href.is_empty() {
        return None;
    }

    let candidate = if href.starts_with("//") {
        format!("https:{href}")
    } else if href.starts_with('/') {
        format!("https://html.duckduckgo.com{href}")
    } else {
        href.to_string()
    };

    let parsed = Url::parse(&candidate).ok()?;
    if parsed
        .host_str()
        .map(|host| host.contains("duckduckgo.com"))
        .unwrap_or(false)
    {
        if let Some((_, decoded)) = parsed.query_pairs().find(|(key, _)| key == "uddg") {
            return Some(decoded.into_owned());
        }
    }

    Some(parsed.to_string())
}

fn source_host_for_url(url: &str) -> Option<String> {
    Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(|host| host.to_ascii_lowercase()))
}

fn extract_page_title(body: &str) -> Option<String> {
    let document = Html::parse_document(body);
    let selector = Selector::parse("title").ok()?;
    let title = document.select(&selector).next()?;
    let normalized = normalize_text(&title.text().collect::<Vec<_>>().join(" "));
    if normalized.is_empty() {
        None
    } else {
        Some(truncate_chars(&normalized, 180))
    }
}

fn summarize_page_body(body: &str, content_type: &str) -> String {
    if content_type.to_ascii_lowercase().starts_with("text/plain") {
        return truncate_chars(&normalize_text(body), PUBLIC_WEB_MAX_TEXT_CHARS);
    }

    let document = Html::parse_document(body);
    let selectors = ["main", "article", "p", "li", "h1", "h2", "h3"];
    let mut segments = Vec::new();
    for selector in selectors {
        let Ok(selector) = Selector::parse(selector) else {
            continue;
        };
        for node in document.select(&selector) {
            let text = normalize_text(&node.text().collect::<Vec<_>>().join(" "));
            if text.len() >= 24 {
                segments.push(text);
            }
            if segments.len() >= 12 {
                break;
            }
        }
        if segments.len() >= 12 {
            break;
        }
    }

    if segments.is_empty() {
        if let Some(title) = extract_page_title(body) {
            return title;
        }
        return String::new();
    }

    truncate_chars(&segments.join(" "), PUBLIC_WEB_MAX_TEXT_CHARS)
}

fn normalize_text(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(raw: &str, max_chars: usize) -> String {
    if raw.chars().count() <= max_chars {
        return raw.to_string();
    }
    raw.chars().take(max_chars).collect::<String>() + "..."
}

#[cfg(test)]
mod tests {
    use super::{
        extract_page_title, finalize_public_text_response, is_public_ip, normalize_public_url,
        normalize_search_result_url, parse_duckduckgo_results, summarize_page_body,
        validate_public_target,
    };
    use axum::{Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
    use reqwest::Client;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    async fn spawn_web_test_server(
        status: StatusCode,
        content_type: &'static str,
        body: String,
    ) -> (String, tokio::task::JoinHandle<()>) {
        async fn handler(
            State((status, content_type, body)): State<(StatusCode, &'static str, String)>,
        ) -> impl IntoResponse {
            (
                status,
                [(axum::http::header::CONTENT_TYPE, content_type)],
                body,
            )
        }
        let app = Router::new()
            .route("/", get(handler))
            .with_state((status, content_type, body));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}/"), handle)
    }

    #[test]
    fn private_ipv4_is_not_public() {
        assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5))));
        assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
    }

    #[test]
    fn public_ipv4_is_public() {
        assert!(is_public_ip(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
    }

    #[test]
    fn unique_local_ipv6_is_not_public() {
        assert!(!is_public_ip(IpAddr::V6(
            "fd00::1".parse::<Ipv6Addr>().expect("valid ipv6")
        )));
    }

    #[test]
    fn duckduckgo_redirect_url_is_unwrapped() {
        let normalized = normalize_search_result_url(
            "https://duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fweather",
        )
        .expect("expected normalized url");
        assert_eq!(normalized, "https://example.com/weather");
    }

    #[test]
    fn search_results_are_parsed() {
        let html = r#"
        <html><body>
          <div class="result">
            <a class="result__a" href="https://example.com/weather">Dublin Weather</a>
            <a class="result__snippet">Current weather and forecast for Dublin.</a>
          </div>
        </body></html>
        "#;
        let results = parse_duckduckgo_results(html, 3);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Dublin Weather");
        assert_eq!(results[0].source_host, "example.com");
    }

    #[test]
    fn html_summary_extracts_visible_text() {
        let html = r#"
        <html><head><title>Example Page</title></head>
        <body><main><p>This is a useful summary paragraph for testing extraction.</p></main></body></html>
        "#;
        assert_eq!(extract_page_title(html).as_deref(), Some("Example Page"));
        assert!(summarize_page_body(html, "text/html").contains("useful summary paragraph"));
    }

    #[test]
    fn normalize_public_url_rejects_credentialed_and_custom_port_urls() {
        let credentialed = normalize_public_url("https://user:pass@example.com/");
        assert!(
            credentialed
                .expect_err("credentialed URL should fail")
                .contains("credentialed")
        );

        let custom_port = normalize_public_url("https://example.com:8443/");
        assert!(
            custom_port
                .expect_err("custom port URL should fail")
                .contains("default public web ports")
        );
    }

    #[tokio::test]
    async fn validate_public_target_rejects_localhost_hosts() {
        let url = normalize_public_url("http://localhost/").unwrap();
        let error = validate_public_target(&url)
            .await
            .expect_err("localhost should be blocked");
        assert!(error.contains("not allowed"));
    }

    #[tokio::test]
    async fn finalize_public_text_response_rejects_binary_content_types() {
        let (url, handle) = spawn_web_test_server(
            StatusCode::OK,
            "application/octet-stream",
            "binary".to_string(),
        )
        .await;
        let response = Client::new().get(&url).send().await.unwrap();
        let error = match finalize_public_text_response(url.clone(), response).await {
            Ok(_) => panic!("binary content type should be rejected"),
            Err(error) => error,
        };
        assert!(error.contains("content type is not supported"));
        handle.abort();
    }

    #[tokio::test]
    async fn finalize_public_text_response_rejects_oversized_bodies() {
        let oversized = "a".repeat(70 * 1024);
        let (url, handle) = spawn_web_test_server(StatusCode::OK, "text/plain", oversized).await;
        let response = Client::new().get(&url).send().await.unwrap();
        let error = match finalize_public_text_response(url.clone(), response).await {
            Ok(_) => panic!("oversized body should be rejected"),
            Err(error) => error,
        };
        assert!(error.contains("exceeded size limit"));
        handle.abort();
    }
}
