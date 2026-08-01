//! Resource loading: URLs, schemes and a small cache.
//!
//! Supports `http`, `https`, `file`, `data` and the browser's own `about`
//! scheme. Everything is synchronous and explicit — the engine decides when to
//! fetch, and can be handed a different [`Loader`] in tests.

mod address;
mod cache;
mod http;

pub use address::{normalize_input, resolve, Address, AddressKind};
pub use cache::ResourceCache;
pub use http::{HttpLoader, RequestOptions};

use std::fmt;

/// What came back from a load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource {
    /// The URL the content was finally read from, after redirects.
    pub url: String,
    pub status: u16,
    /// Lower-cased MIME type without parameters, e.g. `text/html`.
    pub content_type: String,
    /// Charset from the `Content-Type` header, if it named one.
    pub charset: Option<String>,
    pub body: Vec<u8>,
}

impl Resource {
    pub fn new(url: impl Into<String>, content_type: impl Into<String>, body: Vec<u8>) -> Self {
        Resource {
            url: url.into(),
            status: 200,
            content_type: content_type.into(),
            charset: None,
            body,
        }
    }

    pub fn html(url: impl Into<String>, html: impl Into<String>) -> Self {
        Resource::new(url, "text/html", html.into().into_bytes())
    }

    /// Decodes the body as text. Only UTF-8 and Latin-1 are handled; anything
    /// else is read as UTF-8 with replacement characters.
    pub fn text(&self) -> String {
        let charset = self
            .charset
            .as_deref()
            .unwrap_or("utf-8")
            .to_ascii_lowercase();
        match charset.as_str() {
            "iso-8859-1" | "latin1" | "windows-1252" => {
                self.body.iter().map(|byte| *byte as char).collect()
            }
            _ => String::from_utf8_lossy(&self.body).into_owned(),
        }
    }

    pub fn is_html(&self) -> bool {
        self.content_type == "text/html" || self.content_type == "application/xhtml+xml"
    }

    pub fn is_css(&self) -> bool {
        self.content_type == "text/css"
    }

    pub fn is_image(&self) -> bool {
        self.content_type.starts_with("image/")
    }

    pub fn is_ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Why a load failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The input could not be understood as a URL.
    BadUrl(String),
    /// The scheme is not one this browser loads.
    UnsupportedScheme(String),
    /// The server or file system said no.
    Io(String),
    /// An HTTP status outside 2xx, with the body if there was one.
    Status { url: String, status: u16 },
    /// Redirected too many times.
    TooManyRedirects(String),
    /// Loading was refused by policy, e.g. an offline profile.
    Blocked(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::BadUrl(input) => write!(f, "not a valid address: {input}"),
            LoadError::UnsupportedScheme(scheme) => {
                write!(f, "{scheme} addresses are not supported")
            }
            LoadError::Io(message) => write!(f, "{message}"),
            LoadError::Status { url, status } => {
                write!(f, "{url} returned HTTP {status}")
            }
            LoadError::TooManyRedirects(url) => write!(f, "{url} redirected too many times"),
            LoadError::Blocked(reason) => write!(f, "blocked: {reason}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Anything that can turn an address into bytes.
pub trait Loader {
    fn load(&self, address: &Address) -> Result<Resource, LoadError>;
}

/// A loader that refuses everything, for offline tests.
pub struct OfflineLoader;

impl Loader for OfflineLoader {
    fn load(&self, address: &Address) -> Result<Resource, LoadError> {
        Err(LoadError::Blocked(format!(
            "{} (this profile is offline)",
            address.url()
        )))
    }
}

/// A loader backed by a fixed set of responses, for tests.
#[derive(Default)]
pub struct StaticLoader {
    responses: Vec<(String, Resource)>,
}

impl StaticLoader {
    pub fn new() -> Self {
        StaticLoader::default()
    }

    pub fn with(mut self, url: impl Into<String>, resource: Resource) -> Self {
        self.responses.push((url.into(), resource));
        self
    }

    pub fn with_html(self, url: impl Into<String>, html: &str) -> Self {
        let url = url.into();
        let resource = Resource::html(url.clone(), html);
        self.with(url, resource)
    }
}

impl Loader for StaticLoader {
    fn load(&self, address: &Address) -> Result<Resource, LoadError> {
        let wanted = address.url();
        self.responses
            .iter()
            .find(|(url, _)| url == wanted)
            .map(|(_, resource)| resource.clone())
            .ok_or_else(|| LoadError::Status {
                url: wanted.to_string(),
                status: 404,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_decoding_handles_utf8_and_latin1() {
        let mut resource = Resource::new("http://x/", "text/plain", "café".as_bytes().to_vec());
        assert_eq!(resource.text(), "café");

        resource.charset = Some("iso-8859-1".into());
        resource.body = vec![0x63, 0x61, 0x66, 0xe9];
        assert_eq!(resource.text(), "café");
    }

    #[test]
    fn invalid_utf8_does_not_lose_the_document() {
        let resource = Resource::new("http://x/", "text/html", vec![b'a', 0xff, b'b']);
        let text = resource.text();
        assert!(text.starts_with('a') && text.ends_with('b'));
    }

    #[test]
    fn content_type_predicates() {
        assert!(Resource::new("u", "text/html", vec![]).is_html());
        assert!(Resource::new("u", "text/css", vec![]).is_css());
        assert!(Resource::new("u", "image/png", vec![]).is_image());
        assert!(!Resource::new("u", "text/plain", vec![]).is_html());
    }

    #[test]
    fn status_predicate() {
        let mut resource = Resource::new("u", "text/html", vec![]);
        assert!(resource.is_ok());
        resource.status = 404;
        assert!(!resource.is_ok());
        resource.status = 204;
        assert!(resource.is_ok());
    }

    #[test]
    fn errors_read_as_sentences() {
        assert_eq!(
            LoadError::UnsupportedScheme("ftp".into()).to_string(),
            "ftp addresses are not supported"
        );
        assert!(LoadError::Status {
            url: "http://x/".into(),
            status: 500
        }
        .to_string()
        .contains("500"));
    }

    #[test]
    fn the_static_loader_serves_what_it_was_given() {
        let loader = StaticLoader::new().with_html("http://example.com/", "<p>hi</p>");
        let address = Address::parse("http://example.com/").unwrap();
        let resource = loader.load(&address).expect("served");
        assert!(resource.text().contains("hi"));

        let missing = Address::parse("http://example.com/other").unwrap();
        assert!(matches!(
            loader.load(&missing),
            Err(LoadError::Status { status: 404, .. })
        ));
    }

    #[test]
    fn the_offline_loader_refuses_everything() {
        let address = Address::parse("http://example.com/").unwrap();
        assert!(matches!(
            OfflineLoader.load(&address),
            Err(LoadError::Blocked(_))
        ));
    }
}
