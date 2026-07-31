//! Addresses: what the user typed, turned into something loadable.

use crate::LoadError;
use url::Url;

/// The schemes the browser handles, and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressKind {
    Http,
    Https,
    File,
    /// `data:` URLs, decoded without any network access.
    Data,
    /// The browser's own pages: `about:home`, `about:settings`, …
    About,
}

/// A validated, absolute address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    url: Url,
    kind: AddressKind,
}

impl Address {
    /// Parses an absolute URL.
    pub fn parse(input: &str) -> Result<Address, LoadError> {
        let url = Url::parse(input.trim()).map_err(|_| LoadError::BadUrl(input.to_string()))?;
        let kind = match url.scheme() {
            "http" => AddressKind::Http,
            "https" => AddressKind::Https,
            "file" => AddressKind::File,
            "data" => AddressKind::Data,
            "about" => AddressKind::About,
            other => return Err(LoadError::UnsupportedScheme(other.to_string())),
        };
        Ok(Address { url, kind })
    }

    /// Parses `input` relative to `base`.
    pub fn parse_relative(base: &Address, input: &str) -> Result<Address, LoadError> {
        let joined = base
            .url
            .join(input.trim())
            .map_err(|_| LoadError::BadUrl(input.to_string()))?;
        Address::parse(joined.as_str())
    }

    pub fn kind(&self) -> AddressKind {
        self.kind
    }

    pub fn url(&self) -> &str {
        self.url.as_str()
    }

    pub fn scheme(&self) -> &str {
        self.url.scheme()
    }

    pub fn host(&self) -> Option<&str> {
        self.url.host_str()
    }

    pub fn path(&self) -> &str {
        self.url.path()
    }

    pub fn fragment(&self) -> Option<&str> {
        self.url.fragment()
    }

    /// The same address without its fragment, which is what gets fetched.
    pub fn without_fragment(&self) -> Address {
        let mut url = self.url.clone();
        url.set_fragment(None);
        Address {
            url,
            kind: self.kind,
        }
    }

    /// Do two addresses refer to the same document, ignoring the fragment?
    pub fn same_document(&self, other: &Address) -> bool {
        self.without_fragment().url == other.without_fragment().url
    }

    /// A short label for the omnibox: the host, or something sensible for
    /// schemes that have no host.
    pub fn display_host(&self) -> String {
        match self.kind {
            AddressKind::About => format!("about:{}", self.url.path()),
            AddressKind::Data => "data:".to_string(),
            AddressKind::File => "file".to_string(),
            _ => self
                .host()
                .map(|host| host.trim_start_matches("www.").to_string())
                .unwrap_or_else(|| self.url.scheme().to_string()),
        }
    }

    /// Is this address secure, for the omnibox indicator?
    pub fn is_secure(&self) -> bool {
        matches!(self.kind, AddressKind::Https | AddressKind::About)
            || (self.kind == AddressKind::Http
                && matches!(self.host(), Some("localhost") | Some("127.0.0.1")))
    }

    /// The `about:` page name, if this is an about address.
    pub fn about_page(&self) -> Option<&str> {
        (self.kind == AddressKind::About).then(|| self.url.path())
    }

    /// Local path for a `file:` address.
    pub fn file_path(&self) -> Option<std::path::PathBuf> {
        (self.kind == AddressKind::File)
            .then(|| self.url.to_file_path().ok())
            .flatten()
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.url.as_str())
    }
}

/// Turns whatever the user typed into an address.
///
/// A string that looks like a host or a path becomes a URL; anything else
/// becomes a search on `search_template`, where `{}` marks the query slot.
pub fn normalize_input(input: &str, search_template: &str) -> Result<Address, LoadError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Address::parse("about:home");
    }

    // An explicit scheme is taken at face value.
    if let Some((scheme, rest)) = trimmed.split_once(':') {
        let looks_like_scheme = !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
            // `example.com:8080/path` is a host with a port, not a scheme.
            && !rest.chars().next().is_some_and(|c| c.is_ascii_digit());
        if looks_like_scheme {
            return Address::parse(trimmed);
        }
    }

    // Absolute and home-relative file paths.
    if trimmed.starts_with('/') || trimmed.starts_with("~/") || trimmed.starts_with("./") {
        let expanded = if let Some(rest) = trimmed.strip_prefix("~/") {
            match std::env::var("HOME") {
                Ok(home) => format!("{home}/{rest}"),
                Err(_) => trimmed.to_string(),
            }
        } else {
            trimmed.to_string()
        };
        if let Ok(canonical) = std::fs::canonicalize(&expanded) {
            if let Ok(url) = Url::from_file_path(&canonical) {
                return Address::parse(url.as_str());
            }
        }
    }

    if looks_like_host(trimmed) {
        // Default to HTTPS; a server that only speaks HTTP will be retried by
        // the caller if it wants to.
        return Address::parse(&format!("https://{trimmed}"));
    }

    let query = percent_encode(trimmed);
    let target = if search_template.contains("{}") {
        search_template.replace("{}", &query)
    } else {
        format!("{search_template}{query}")
    };
    Address::parse(&target)
}

/// Does this look like a hostname rather than a search?
fn looks_like_host(input: &str) -> bool {
    if input.contains(' ') {
        return false;
    }
    let authority = input.split(['/', '?', '#']).next().unwrap_or(input);
    let host = authority.split(':').next().unwrap_or(authority);
    if host == "localhost" {
        return true;
    }
    // An IPv4-looking address, or something with a dotted TLD of 2+ letters.
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|label| label.is_empty()) {
        return false;
    }
    let tld = labels[labels.len() - 1];
    let valid_label = |label: &str| {
        label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || !c.is_ascii())
    };
    labels.iter().all(|label| valid_label(label))
        && (tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
            || tld.chars().all(|c| c.is_ascii_digit()))
}

/// Percent-encodes a search query.
fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Resolves `reference` against `base`, returning an absolute URL string.
pub fn resolve(base: Option<&str>, reference: &str) -> Option<String> {
    let reference = reference.trim();
    if reference.is_empty() {
        return None;
    }
    if let Ok(absolute) = Url::parse(reference) {
        return Some(absolute.to_string());
    }
    let base = Url::parse(base?).ok()?;
    base.join(reference).ok().map(|url| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_supported_schemes() {
        assert_eq!(
            Address::parse("https://example.com/").unwrap().kind(),
            AddressKind::Https
        );
        assert_eq!(
            Address::parse("http://example.com/").unwrap().kind(),
            AddressKind::Http
        );
        assert_eq!(
            Address::parse("about:home").unwrap().kind(),
            AddressKind::About
        );
        assert_eq!(
            Address::parse("data:text/plain,hi").unwrap().kind(),
            AddressKind::Data
        );
        assert_eq!(
            Address::parse("file:///tmp/x.html").unwrap().kind(),
            AddressKind::File
        );
    }

    #[test]
    fn rejects_unsupported_and_malformed_input() {
        assert!(matches!(
            Address::parse("ftp://example.com/"),
            Err(LoadError::UnsupportedScheme(_))
        ));
        assert!(matches!(
            Address::parse("not a url"),
            Err(LoadError::BadUrl(_))
        ));
    }

    #[test]
    fn typed_hosts_become_https() {
        let address = normalize_input("example.com", "https://s/?q={}").unwrap();
        assert_eq!(address.url(), "https://example.com/");

        let with_path = normalize_input("example.com/a/b?c=1", "https://s/?q={}").unwrap();
        assert_eq!(with_path.url(), "https://example.com/a/b?c=1");

        let with_port = normalize_input("example.com:8080/x", "https://s/?q={}").unwrap();
        assert_eq!(with_port.url(), "https://example.com:8080/x");

        assert_eq!(
            normalize_input("localhost:3000", "https://s/?q={}")
                .unwrap()
                .url(),
            "https://localhost:3000/"
        );
    }

    #[test]
    fn explicit_schemes_are_respected() {
        assert_eq!(
            normalize_input("http://example.com", "https://s/?q={}")
                .unwrap()
                .url(),
            "http://example.com/"
        );
        assert_eq!(
            normalize_input("about:settings", "https://s/?q={}")
                .unwrap()
                .url(),
            "about:settings"
        );
    }

    #[test]
    fn prose_becomes_a_search() {
        let address = normalize_input("what a browser", "https://search/?q={}").unwrap();
        assert_eq!(address.url(), "https://search/?q=what+a+browser");

        // A single word with no dot is a search, not a host.
        let single = normalize_input("rust", "https://search/?q={}").unwrap();
        assert!(single.url().contains("q=rust"));
    }

    #[test]
    fn search_queries_are_encoded() {
        let address = normalize_input("a&b=c d", "https://search/?q={}").unwrap();
        assert_eq!(address.url(), "https://search/?q=a%26b%3Dc+d");
    }

    #[test]
    fn empty_input_goes_home() {
        assert_eq!(
            normalize_input("   ", "https://s/?q={}").unwrap().url(),
            "about:home"
        );
    }

    #[test]
    fn relative_urls_resolve_against_the_base() {
        let base = Address::parse("https://example.com/dir/page.html").unwrap();
        assert_eq!(
            Address::parse_relative(&base, "other.html").unwrap().url(),
            "https://example.com/dir/other.html"
        );
        assert_eq!(
            Address::parse_relative(&base, "/root.html").unwrap().url(),
            "https://example.com/root.html"
        );
        assert_eq!(
            Address::parse_relative(&base, "//cdn.example/x.js")
                .unwrap()
                .url(),
            "https://cdn.example/x.js"
        );
    }

    #[test]
    fn resolve_handles_absolute_and_relative() {
        assert_eq!(
            resolve(Some("https://a.com/x/"), "y.css").as_deref(),
            Some("https://a.com/x/y.css")
        );
        assert_eq!(
            resolve(Some("https://a.com/x/"), "https://b.com/z.css").as_deref(),
            Some("https://b.com/z.css")
        );
        assert_eq!(resolve(None, "y.css"), None);
        assert_eq!(resolve(Some("https://a.com/"), "  "), None);
    }

    #[test]
    fn fragments_are_separable() {
        let address = Address::parse("https://example.com/page#section").unwrap();
        assert_eq!(address.fragment(), Some("section"));
        assert_eq!(address.without_fragment().url(), "https://example.com/page");

        let other = Address::parse("https://example.com/page#elsewhere").unwrap();
        assert!(address.same_document(&other));

        let different = Address::parse("https://example.com/other").unwrap();
        assert!(!address.same_document(&different));
    }

    #[test]
    fn display_host_drops_www() {
        assert_eq!(
            Address::parse("https://www.example.com/x")
                .unwrap()
                .display_host(),
            "example.com"
        );
        assert_eq!(
            Address::parse("about:home").unwrap().display_host(),
            "about:home"
        );
    }

    #[test]
    fn security_indicator() {
        assert!(Address::parse("https://example.com/").unwrap().is_secure());
        assert!(!Address::parse("http://example.com/").unwrap().is_secure());
        assert!(Address::parse("http://localhost:8080/")
            .unwrap()
            .is_secure());
        assert!(Address::parse("about:home").unwrap().is_secure());
    }

    #[test]
    fn host_detection_edge_cases() {
        assert!(looks_like_host("example.com"));
        assert!(looks_like_host("sub.example.co.uk"));
        assert!(looks_like_host("192.168.0.1"));
        assert!(looks_like_host("localhost"));
        assert!(!looks_like_host("hello world"));
        assert!(!looks_like_host("single"));
        assert!(!looks_like_host("trailing."));
        assert!(!looks_like_host("2 + 2"));
    }
}
