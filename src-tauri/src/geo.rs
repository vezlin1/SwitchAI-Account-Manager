use std::time::{Duration, Instant};

use reqwest::Client;

pub const UNSUPPORTED_OPENAI_COUNTRIES: &[&str] = &["RU", "BY", "CN", "IR", "KP", "CU", "SY", "VE"];

pub const CLOUDFLARE_TRACE_URLS: &[&str] = &[
    "https://cloudflare.com/cdn-cgi/trace",
    "https://1.1.1.1/cdn-cgi/trace",
];

pub const CHATGPT_PROBE_URL: &str = "https://chatgpt.com/cdn-cgi/trace";

const TRACE_PROBE_TIMEOUT: Duration = Duration::from_millis(2500);
const CHATGPT_PROBE_TIMEOUT: Duration = Duration::from_millis(3000);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatGptReachability {
    Available,
    UnsupportedRegion { country_code: String },
    BlockedOrUnreachable { reason: String },
}

impl ChatGptReachability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    pub fn user_summary(&self) -> String {
        match self {
            Self::Available => "ChatGPT is reachable".to_string(),
            Self::UnsupportedRegion { country_code } => {
                format!("Unsupported region ({country_code}) for ChatGPT. Connect via VPN.")
            }
            Self::BlockedOrUnreachable { reason } => {
                format!("ChatGPT is unreachable ({reason}). Connect via VPN.")
            }
        }
    }
}

pub fn parse_cloudflare_trace_loc(trace_body: &str) -> Option<String> {
    for line in trace_body.lines() {
        let trimmed = line.trim();
        if let Some(loc) = trimmed.strip_prefix("loc=") {
            let country = loc.trim();
            if !country.is_empty() {
                return Some(country.to_ascii_uppercase());
            }
        }
    }
    None
}

pub async fn probe_chatgpt_reachability(client: &Client) -> ChatGptReachability {
    // 1. Fast probe to Cloudflare trace to check country location without waiting for blocked domain timeout
    for trace_url in CLOUDFLARE_TRACE_URLS {
        if let Ok(response) = client
            .get(*trace_url)
            .timeout(TRACE_PROBE_TIMEOUT)
            .send()
            .await
            && response.status().is_success()
            && let Ok(body) = response.text().await
            && let Some(loc) = parse_cloudflare_trace_loc(&body)
        {
            if UNSUPPORTED_OPENAI_COUNTRIES.contains(&loc.as_str()) {
                return ChatGptReachability::UnsupportedRegion { country_code: loc };
            }
            // The second trace is only a fallback for a failed first trace.
            break;
        }
    }

    // 2. Probe chatgpt.com directly
    match client
        .get(CHATGPT_PROBE_URL)
        .header("User-Agent", "codex-cli")
        .timeout(CHATGPT_PROBE_TIMEOUT)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                if let Ok(body) = resp.text().await
                    && let Some(loc) = parse_cloudflare_trace_loc(&body)
                    && UNSUPPORTED_OPENAI_COUNTRIES.contains(&loc.as_str())
                {
                    return ChatGptReachability::UnsupportedRegion { country_code: loc };
                }
                ChatGptReachability::Available
            } else if status.as_u16() == 403 || status.as_u16() == 451 || status.as_u16() == 1020 {
                ChatGptReachability::UnsupportedRegion {
                    country_code: "Geo-blocked".to_string(),
                }
            } else {
                ChatGptReachability::Available
            }
        }
        Err(err) => {
            let reason = if err.is_timeout() {
                "Connection timed out".to_string()
            } else if err.is_connect() {
                "Connection failed/blocked".to_string()
            } else {
                err.to_string()
            };
            ChatGptReachability::BlockedOrUnreachable { reason }
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct ReachabilityCache {
    cached: Option<(Instant, ChatGptReachability)>,
    generation: u64,
}

impl ReachabilityCache {
    pub fn get_cached(
        &self,
        retry_unavailable: bool,
        requested_at: Instant,
    ) -> Option<ChatGptReachability> {
        if let Some((checked_at, ref reachability)) = self.cached
            && checked_at.elapsed()
                < Duration::from_secs(if reachability.is_available() { 600 } else { 30 })
            && (reachability.is_available() || !retry_unavailable || checked_at >= requested_at)
        {
            Some(reachability.clone())
        } else {
            None
        }
    }

    pub fn set(&mut self, reachability: ChatGptReachability) {
        self.cached = Some((Instant::now(), reachability));
    }

    pub fn invalidate(&mut self) {
        self.cached = None;
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_reuses_success_and_coalesces_failed_probes_but_allows_manual_retry() {
        let requested = Instant::now();
        let mut cache = ReachabilityCache::default();
        cache.set(ChatGptReachability::Available);
        assert!(
            cache
                .get_cached(true, Instant::now())
                .unwrap()
                .is_available()
        );
        cache.cached.as_mut().unwrap().0 = Instant::now() - Duration::from_secs(601);
        assert!(cache.get_cached(false, requested).is_none());
        cache.set(ChatGptReachability::BlockedOrUnreachable {
            reason: "offline".into(),
        });
        assert!(cache.get_cached(true, requested).is_some()); // Same in-flight probe.
        let after = cache.cached.as_ref().unwrap().0 + Duration::from_nanos(1);
        assert!(cache.get_cached(true, after).is_none()); // New explicit retry.
        assert!(cache.get_cached(false, after).is_some());
        cache.cached.as_mut().unwrap().0 = Instant::now() - Duration::from_secs(31);
        assert!(cache.get_cached(false, requested).is_none());
        cache.set(ChatGptReachability::Available);
        let generation = cache.generation();
        cache.invalidate();
        assert_ne!(cache.generation(), generation);
        assert!(cache.get_cached(false, requested).is_none());
    }

    #[test]
    fn parses_loc_from_trace() {
        let trace = "fl=1166f27\nh=cloudflare.com\nip=92.255.206.132\nloc=RU\ntls=TLSv1.3\n";
        assert_eq!(parse_cloudflare_trace_loc(trace), Some("RU".to_string()));

        let trace_us = "fl=1166f27\nloc=US\n";
        assert_eq!(parse_cloudflare_trace_loc(trace_us), Some("US".to_string()));

        let trace_empty = "fl=1166f27\n";
        assert_eq!(parse_cloudflare_trace_loc(trace_empty), None);
    }

    #[test]
    fn detects_unsupported_countries() {
        assert!(UNSUPPORTED_OPENAI_COUNTRIES.contains(&"RU"));
        assert!(UNSUPPORTED_OPENAI_COUNTRIES.contains(&"BY"));
        assert!(UNSUPPORTED_OPENAI_COUNTRIES.contains(&"CN"));
        assert!(UNSUPPORTED_OPENAI_COUNTRIES.contains(&"IR"));
        assert!(!UNSUPPORTED_OPENAI_COUNTRIES.contains(&"US"));
        assert!(!UNSUPPORTED_OPENAI_COUNTRIES.contains(&"DE"));
        assert!(!UNSUPPORTED_OPENAI_COUNTRIES.contains(&"FI"));
    }

    #[test]
    fn reachability_user_summary_format() {
        let available = ChatGptReachability::Available;
        assert!(available.is_available());
        assert_eq!(available.user_summary(), "ChatGPT is reachable");

        let unsupported = ChatGptReachability::UnsupportedRegion {
            country_code: "RU".to_string(),
        };
        assert!(!unsupported.is_available());
        assert!(
            unsupported
                .user_summary()
                .contains("Unsupported region (RU)")
        );

        let unreachable = ChatGptReachability::BlockedOrUnreachable {
            reason: "Connection timed out".to_string(),
        };
        assert!(!unreachable.is_available());
        assert!(
            unreachable
                .user_summary()
                .contains("ChatGPT is unreachable")
        );
    }
}
