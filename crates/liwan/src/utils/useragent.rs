use quick_cache::sync::Cache;
use std::{io::Cursor, sync::LazyLock};
use ua_parser::Extractor;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserAgent {
    pub device_family: Option<String>,
    pub os_family: Option<String>,
    pub ua_family: Option<String>,
}

static PARSER: LazyLock<Extractor<'static>> = LazyLock::new(|| {
    let data =
        zstd::decode_all(Cursor::new(include_bytes!("../../../../data/ua_regexes.json.zstd"))).expect("valid data");
    let regexes: ua_parser::Regexes = serde_json::from_slice(&data).expect("valid data");

    ua_parser::Extractor::try_from(regexes).expect("valid data")
});

#[derive(Clone, Debug, PartialEq, Eq)]
enum Classification {
    Crawler,
    Client(UserAgent),
}

impl Classification {
    fn client(self) -> Option<UserAgent> {
        match self {
            Self::Crawler => None,
            Self::Client(client) => Some(client),
        }
    }
}

static UAP_CACHE: LazyLock<Cache<String, Classification>> = LazyLock::new(|| Cache::new(1024));
static CRAWLER_TOKENS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    include_str!("../../../../data/crawlers.txt")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
});

/// Parse a user agent and screen crawlers and bots in one cached call.
///
/// `None` means the event should be dropped. The verdict and the parsed client
/// share one cache entry, so a repeated header costs one lookup instead of a
/// crawler scan plus a parser lookup.
pub fn classify(header: &str) -> Option<UserAgent> {
    if let Some(entry) = UAP_CACHE.get(header) {
        return entry.client();
    }

    let entry = if is_crawler_header(header) {
        Classification::Crawler
    } else {
        let client = parse(header);
        if client.is_bot() { Classification::Crawler } else { Classification::Client(client) }
    };

    UAP_CACHE.insert(header.to_string(), entry.clone());
    entry.client()
}

fn parse(header: &str) -> UserAgent {
    let (ua, os, device) = PARSER.extract(header);
    UserAgent {
        device_family: device.map(|d| d.device.to_string()),
        os_family: os.map(|os| os.os.replace("Mac OS X", "macOS").replace("Other", "Unknown")),
        ua_family: ua.map(|ua| ua.family.to_string()),
    }
}

fn is_crawler_header(header: &str) -> bool {
    let header = header.to_ascii_lowercase();
    CRAWLER_TOKENS.iter().any(|crawler| header.contains(crawler))
}

impl UserAgent {
    pub fn is_bot(&self) -> bool {
        self.device_family == Some("Spider".into()) || self.ua_family == Some("HeadlessChrome".into())
    }

    pub fn is_mobile(&self) -> bool {
        [Some("iOS"), Some("Android")].contains(&self.os_family.as_deref()) // good enough for 99% of cases
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_ua_parser() {
        let user_agent = "Mozilla/5.0 (iPhone; CPU iPhone OS 13_2_3 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/13.0.3 Mobile/15E148 Safari/604.1";
        let client = classify(user_agent).expect("a normal browser is not a crawler");
        assert_eq!(client.os_family, Some("iOS".into()), "Expected OS family to be iOS");
        assert_eq!(client.device_family, Some("iPhone".into()), "Expected device family to be iPhone");
        assert!(client.is_mobile(), "Expected device to be mobile");
        assert!(!client.is_bot(), "Expected device to not be a bot");
    }

    #[test]
    fn crawler_header_matches_long_browser_like_user_agents() {
        let applebot = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1 Applebot/0.1";
        let bytespider = "Mozilla/5.0 (Linux; Android 10; Pixel 4 XL) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36 Bytespider";
        let google_other = "Mozilla/5.0 (Linux; Android 6.0.1; Nexus 5X Build/MMB29P) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36 (compatible; GoogleOther)";
        let yisou = "Mozilla/5.0 (Linux; Android 11; Mobile) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/96.0.4664.45 Mobile Safari/537.36 YisouSpider";

        assert!(is_crawler_header(applebot));
        assert!(is_crawler_header(bytespider));
        assert!(is_crawler_header(google_other));
        assert!(is_crawler_header(yisou));
    }

    #[test]
    fn crawler_header_matching_is_case_insensitive() {
        assert!(is_crawler_header("Mozilla/5.0 APPLEBOT/0.1"));
        assert!(is_crawler_header("Mozilla/5.0 ByteSpider"));
    }

    #[test]
    fn crawler_header_ignores_normal_browsers() {
        let user_agent = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
        assert!(!is_crawler_header(user_agent));
    }

    #[test]
    fn classify_drops_crawlers_and_headless_browsers() {
        let applebot = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1 Applebot/0.1";
        let headless = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/120.0.0.0 Safari/537.36";

        assert_eq!(classify(applebot), None, "a crawler token must drop the header");
        assert_eq!(classify(headless), None, "a parsed bot must drop the header");
    }

    #[test]
    fn classify_returns_the_same_verdict_when_cached() {
        let user_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
        let first = classify(user_agent);
        assert!(first.is_some());
        assert_eq!(first, classify(user_agent));
    }
}
