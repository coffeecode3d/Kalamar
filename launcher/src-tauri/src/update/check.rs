//! Is there a newer launcher? One request to GitHub's release API (`/releases/latest` on the stable channel, the last
//! ten releases on the beta channel), answered from the ETag when nothing changed; then, only for a release newer than
//! this launcher, its manifest and signature, checked before anything is offered. Of the API's answer only the tag
//! name and the pre-release and draft flags are read: the release notes shown are the signed manifest's.

use super::manifest::{self, Channel, Expect, Manifest};
use super::net::{self, ApiReply, NetError, Source};
use super::version::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// A release the API lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub tag: String,
    pub prerelease: bool,
}

impl Candidate {
    pub fn version(&self) -> Option<Version> {
        Version::parse_tag(&self.tag)
    }
}

/// A tag usable in a download URL: the version spelling, with or without `v`.
pub fn valid_tag(tag: &str) -> bool {
    tag.len() <= 64 && tag.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')) && Version::parse_tag(tag).is_some()
}

fn candidate(v: &Value) -> Option<Candidate> {
    let tag = v.get("tag_name")?.as_str()?;
    if v.get("draft").and_then(Value::as_bool).unwrap_or(false) || !valid_tag(tag) {
        return None;
    }
    Some(Candidate { tag: tag.to_string(), prerelease: v.get("prerelease").and_then(Value::as_bool).unwrap_or(false) })
}

/// The releases of an API answer (one object for `latest`, an array for the list); drafts and odd tags are left out.
pub fn parse_releases(body: &Value) -> Vec<Candidate> {
    match body {
        Value::Array(items) => items.iter().take(50).filter_map(candidate).collect(),
        Value::Object(_) => candidate(body).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// The newest release above `current` that the channel takes (stable: no pre-release flag, no pre-release version).
pub fn pick(cands: &[Candidate], channel: Channel, current: &Version) -> Option<Candidate> {
    cands
        .iter()
        .filter_map(|c| c.version().map(|v| (v, c)))
        .filter(|(v, c)| channel == Channel::Beta || (!c.prerelease && !v.is_prerelease()))
        .filter(|(v, _)| v > current)
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, c)| c.clone())
}

/// What the check remembers between runs (`<data>\update\state.json`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CheckState {
    /// Unix seconds of the last check that reached GitHub.
    pub last_check: u64,
    /// No request before this time (GitHub's rate limit said so).
    pub not_before: u64,
    /// Per API URL: the ETag and the releases of that answer.
    pub etags: BTreeMap<String, (String, Vec<Candidate>)>,
    /// The last check's outcome code (`up_to_date`, `available`, `network`, `rate_limited`, `untrusted`).
    pub last_result: String,
}

/// A newer release whose manifest is signed and valid.
#[derive(Debug, Clone)]
pub struct Found {
    pub tag: String,
    pub manifest: Manifest,
    pub manifest_bytes: Vec<u8>,
    pub signature: Vec<u8>,
}

#[derive(Debug)]
pub enum Outcome {
    UpToDate,
    Available(Box<Found>),
    Failed(&'static str, String),
}

/// One check. `known` is a manifest already verified for some tag: not downloaded again for that tag.
pub async fn run(src: &Source, current: &Version, channel: Channel, state: &mut CheckState, known: Option<&Found>, now: u64) -> Outcome {
    if now < state.not_before {
        return Outcome::Failed("rate_limited", "GitHub's rate limit".into());
    }
    let url = match channel {
        Channel::Stable => src.latest_url(),
        Channel::Beta => src.list_url(),
    };
    let etag = state.etags.get(&url).map(|(e, _)| e.clone());
    let releases = match net::get_api(src, &url, etag.as_deref()).await {
        Ok(ApiReply::NotModified) => state.etags.get(&url).map(|(_, c)| c.clone()).unwrap_or_default(),
        Ok(ApiReply::Fresh { body, etag }) => {
            let list = parse_releases(&body);
            match etag {
                Some(e) => {
                    state.etags.insert(url.clone(), (e, list.clone()));
                }
                None => {
                    state.etags.remove(&url);
                }
            }
            list
        }
        // `latest` with no stable release yet
        Err(NetError::Status(404)) => Vec::new(),
        Err(NetError::RateLimited(reset)) => {
            state.not_before = reset.unwrap_or(now + 3600).max(now + 60);
            return Outcome::Failed("rate_limited", "GitHub's rate limit".into());
        }
        Err(e) => return Outcome::Failed(e.code(), e.to_string()),
    };
    state.last_check = now;
    let Some(cand) = pick(&releases, channel, current) else { return Outcome::UpToDate };
    if let Some(k) = known.filter(|k| k.tag == cand.tag) {
        return Outcome::Available(Box::new(k.clone()));
    }
    let tag_version = cand.version();
    let (m_url, s_url) = (src.asset_url(&cand.tag, manifest::MANIFEST_NAME), src.asset_url(&cand.tag, manifest::SIGNATURE_NAME));
    let manifest_bytes = match net::get_small(src, &m_url, manifest::MAX_MANIFEST).await {
        Ok(b) => b,
        // a release without a manifest is not an update for this launcher
        Err(NetError::Status(404)) => return Outcome::Failed("untrusted", format!("{} has no {}", cand.tag, manifest::MANIFEST_NAME)),
        Err(e) => return Outcome::Failed(e.code(), e.to_string()),
    };
    let signature = match net::get_small(src, &s_url, manifest::MAX_SIGNATURE).await {
        Ok(b) => b,
        Err(NetError::Status(404)) => return Outcome::Failed("untrusted", format!("{} has no signature", cand.tag)),
        Err(e) => return Outcome::Failed(e.code(), e.to_string()),
    };
    let expect = Expect { repo: &src.repo, tag_version: tag_version.as_ref(), current, channel };
    match manifest::verify(&manifest_bytes, &signature, &src.keys, &expect) {
        Ok(manifest) => Outcome::Available(Box::new(Found { tag: cand.tag, manifest, manifest_bytes, signature })),
        Err(r) => Outcome::Failed("untrusted", format!("{}: {r}", cand.tag)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn the_api_answer_gives_tags_and_flags_only() {
        let latest = json!({"tag_name": "v0.2.0", "prerelease": false, "draft": false, "body": "<script>x</script>", "assets": [{"browser_download_url": "https://evil.example/x.exe"}]});
        assert_eq!(parse_releases(&latest), vec![Candidate { tag: "v0.2.0".into(), prerelease: false }]);
        let list = json!([
            {"tag_name": "v0.3.0-beta.1", "prerelease": true},
            {"tag_name": "v0.2.1", "draft": true},
            {"tag_name": "v0.2.0"},
            {"tag_name": "../../etc"},
            {"tag_name": "v0.2.0/../x"},
            {"tag_name": 7},
            {"name": "no tag"},
        ]);
        let got = parse_releases(&list);
        assert_eq!(got.iter().map(|c| c.tag.as_str()).collect::<Vec<_>>(), ["v0.3.0-beta.1", "v0.2.0"]);
        assert!(parse_releases(&json!("x")).is_empty());
        assert!(!valid_tag("v1.2.3?x=1") && !valid_tag("v1.2.3#") && !valid_tag("") && valid_tag("0.2.0"));
    }

    #[test]
    fn the_channel_picks_the_newest_release_above_the_running_one() {
        let c = |t: &str, p: bool| Candidate { tag: t.into(), prerelease: p };
        let list = [c("v0.3.0-beta.2", true), c("v0.2.1", false), c("v0.2.0", false), c("v0.4.0-rc.1", false), c("v0.1.0", false)];
        assert_eq!(pick(&list, Channel::Stable, &v("0.1.0")).unwrap().tag, "v0.2.1", "a pre-release version is not stable even without the flag");
        assert_eq!(pick(&list, Channel::Beta, &v("0.1.0")).unwrap().tag, "v0.4.0-rc.1");
        assert_eq!(pick(&list, Channel::Stable, &v("0.2.1")), None);
        assert_eq!(pick(&list, Channel::Stable, &v("0.9.0")), None, "never an older version");
        // running a beta: stable offers nothing older, beta offers the next one
        assert_eq!(pick(&list, Channel::Stable, &v("0.3.0-beta.2")), None);
        assert_eq!(pick(&list, Channel::Beta, &v("0.3.0-beta.2")).unwrap().tag, "v0.4.0-rc.1");
    }
}
