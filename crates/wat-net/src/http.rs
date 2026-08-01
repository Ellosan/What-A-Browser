//! The default loader: HTTP(S), `file:` and `data:`.

use crate::address::{Address, AddressKind};
use crate::{LoadError, Loader, Resource};

/// Tunables for outgoing requests.
#[derive(Clone, Debug)]
pub struct RequestOptions {
    pub user_agent: String,
    pub timeout_seconds: u64,
    pub max_redirects: u32,
    /// Refuse bodies larger than this, to keep a bad server from filling memory.
    pub max_body_bytes: usize,
    /// Send `Accept-Language`.
    pub accept_language: String,
}

impl Default for RequestOptions {
    fn default() -> Self {
        RequestOptions {
            user_agent: format!(
                "Mozilla/5.0 (compatible) WhatABrowser/{} WAT",
                env!("CARGO_PKG_VERSION")
            ),
            timeout_seconds: 20,
            max_redirects: 10,
            max_body_bytes: 32 * 1024 * 1024,
            accept_language: "en".to_string(),
        }
    }
}

/// Loads over the network and from disk.
pub struct HttpLoader {
    options: RequestOptions,
    agent: ureq::Agent,
}

impl Default for HttpLoader {
    fn default() -> Self {
        HttpLoader::new(RequestOptions::default())
    }
}

impl HttpLoader {
    pub fn new(options: RequestOptions) -> Self {
        let mut builder = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(options.timeout_seconds))
            // Redirects are followed by hand so the final URL is knowable.
            .redirects(0);

        // Honour the usual proxy environment variables, as every other network
        // client on the machine does.
        if let Some(proxy) = proxy_from_environment() {
            match ureq::Proxy::new(&proxy) {
                Ok(proxy) => builder = builder.proxy(proxy),
                Err(error) => log::warn!("ignoring invalid proxy `{proxy}`: {error}"),
            }
        }

        HttpLoader {
            options,
            agent: builder.build(),
        }
    }

    pub fn options(&self) -> &RequestOptions {
        &self.options
    }

    fn fetch_http(&self, address: &Address) -> Result<Resource, LoadError> {
        let mut current = address.without_fragment();
        for _ in 0..=self.options.max_redirects {
            let response = self
                .agent
                .get(current.url())
                .set("User-Agent", &self.options.user_agent)
                .set("Accept", "text/html,application/xhtml+xml,*/*;q=0.8")
                .set("Accept-Language", &self.options.accept_language)
                .call();

            let response = match response {
                Ok(response) => response,
                // A non-2xx status still carries a response we may want to show.
                Err(ureq::Error::Status(status, response)) => {
                    if let Some(location) = redirect_target(status, &response) {
                        current = Address::parse_relative(&current, &location)?;
                        continue;
                    }
                    let resource = read_response(&current, response, &self.options)?;
                    // A 4xx/5xx body is still worth rendering; the caller
                    // decides whether to show it or an error page.
                    return Ok(Resource { status, ..resource });
                }
                Err(error) => return Err(LoadError::Io(error.to_string())),
            };

            if let Some(location) = redirect_target(response.status(), &response) {
                current = Address::parse_relative(&current, &location)?;
                continue;
            }
            return read_response(&current, response, &self.options);
        }
        Err(LoadError::TooManyRedirects(address.url().to_string()))
    }

    fn read_file(&self, address: &Address) -> Result<Resource, LoadError> {
        let path = address
            .file_path()
            .ok_or_else(|| LoadError::BadUrl(address.url().to_string()))?;

        if path.is_dir() {
            return Ok(Resource {
                url: address.url().to_string(),
                status: 200,
                content_type: "text/html".to_string(),
                charset: Some("utf-8".to_string()),
                body: directory_listing(&path).into_bytes(),
            });
        }

        let body = std::fs::read(&path)
            .map_err(|error| LoadError::Io(format!("{}: {error}", path.display())))?;
        if body.len() > self.options.max_body_bytes {
            return Err(LoadError::Io(format!(
                "{} is larger than the {} byte limit",
                path.display(),
                self.options.max_body_bytes
            )));
        }
        Ok(Resource {
            url: address.url().to_string(),
            status: 200,
            content_type: content_type_for_path(&path).to_string(),
            charset: None,
            body,
        })
    }
}

impl Loader for HttpLoader {
    fn load(&self, address: &Address) -> Result<Resource, LoadError> {
        match address.kind() {
            AddressKind::Http | AddressKind::Https => self.fetch_http(address),
            AddressKind::File => self.read_file(address),
            AddressKind::Data => decode_data_url(address),
            AddressKind::About => Err(LoadError::UnsupportedScheme("about".to_string())),
        }
    }
}

/// The proxy to use, from the environment.
///
/// `HTTPS_PROXY` wins over `HTTP_PROXY`, and either casing is accepted.
fn proxy_from_environment() -> Option<String> {
    ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

/// The `Location` header for a redirect status, if this is one.
fn redirect_target(status: u16, response: &ureq::Response) -> Option<String> {
    if !matches!(status, 301 | 302 | 303 | 307 | 308) {
        return None;
    }
    response.header("Location").map(str::to_string)
}

fn read_response(
    address: &Address,
    response: ureq::Response,
    options: &RequestOptions,
) -> Result<Resource, LoadError> {
    let status = response.status();
    let url = response.get_url().to_string();
    let header = response
        .header("Content-Type")
        .unwrap_or("application/octet-stream")
        .to_string();
    let (content_type, charset) = split_content_type(&header);

    let mut body = Vec::new();
    response
        .into_reader()
        .take(options.max_body_bytes as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|error| LoadError::Io(error.to_string()))?;
    if body.len() > options.max_body_bytes {
        return Err(LoadError::Io(format!(
            "{} sent more than the {} byte limit",
            address.url(),
            options.max_body_bytes
        )));
    }

    let content_type = if content_type == "application/octet-stream" {
        // Some servers do not label HTML; sniff the obvious case.
        sniff_content_type(&body).unwrap_or(content_type)
    } else {
        content_type
    };

    Ok(Resource {
        url,
        status,
        content_type,
        charset,
        body,
    })
}

use std::io::Read as _;

/// Splits `text/html; charset=utf-8` into its parts.
pub(crate) fn split_content_type(header: &str) -> (String, Option<String>) {
    let mut parts = header.split(';');
    let mime = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let mut charset = None;
    for parameter in parts {
        let parameter = parameter.trim();
        if let Some(value) = parameter
            .strip_prefix("charset=")
            .or_else(|| parameter.strip_prefix("charset ="))
        {
            charset = Some(value.trim().trim_matches('"').to_ascii_lowercase());
        }
    }
    (mime, charset)
}

/// A minimal content sniffer for servers that do not label their responses.
fn sniff_content_type(body: &[u8]) -> Option<String> {
    let head = &body[..body.len().min(512)];
    let text = String::from_utf8_lossy(head).to_ascii_lowercase();
    let trimmed = text.trim_start();
    if trimmed.starts_with("<!doctype html")
        || trimmed.starts_with("<html")
        || trimmed.starts_with("<head")
        || trimmed.starts_with("<body")
    {
        return Some("text/html".to_string());
    }
    if head.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some("image/png".to_string());
    }
    if head.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg".to_string());
    }
    if head.starts_with(b"GIF8") {
        return Some("image/gif".to_string());
    }
    None
}

/// Guesses a MIME type from a file extension.
pub(crate) fn content_type_for_path(path: &std::path::Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "html" | "htm" | "xhtml" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "txt" | "md" | "log" | "toml" | "rs" | "py" => "text/plain",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// Renders a browsable listing for a `file:` directory.
fn directory_listing(path: &std::path::Path) -> String {
    let mut entries: Vec<(String, bool)> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| {
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            (entry.file_name().to_string_lossy().into_owned(), is_dir)
        })
        .collect();
    entries.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });

    let mut html = format!(
        "<!doctype html><title>{0}</title><h1>{0}</h1><ul>",
        escape(&path.display().to_string())
    );
    if path.parent().is_some() {
        html.push_str("<li><a href=\"..\">../</a></li>");
    }
    for (name, is_dir) in entries {
        let slash = if is_dir { "/" } else { "" };
        html.push_str(&format!(
            "<li><a href=\"{0}{1}\">{0}{1}</a></li>",
            escape(&name),
            slash
        ));
    }
    html.push_str("</ul>");
    html
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Decodes a `data:` URL.
pub fn decode_data_url(address: &Address) -> Result<Resource, LoadError> {
    let url = address.url();
    let rest = url
        .strip_prefix("data:")
        .ok_or_else(|| LoadError::BadUrl(url.to_string()))?;
    let (meta, payload) = rest
        .split_once(',')
        .ok_or_else(|| LoadError::BadUrl(url.to_string()))?;

    let base64 = meta.trim_end().ends_with(";base64");
    let meta = meta.trim_end().trim_end_matches(";base64");
    let (content_type, charset) = if meta.trim().is_empty() {
        ("text/plain".to_string(), Some("us-ascii".to_string()))
    } else {
        split_content_type(meta)
    };

    let body = if base64 {
        decode_base64(payload).ok_or_else(|| LoadError::BadUrl(url.to_string()))?
    } else {
        percent_decode(payload)
    };

    Ok(Resource {
        url: url.to_string(),
        status: 200,
        content_type,
        charset,
        body,
    })
}

fn percent_decode(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    out
}

/// Standard base64, tolerating missing padding and embedded whitespace.
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut accumulator: u32 = 0;
    let mut bits = 0u32;
    for byte in input.bytes() {
        if byte.is_ascii_whitespace() || byte == b'=' {
            continue;
        }
        let value = TABLE.iter().position(|candidate| *candidate == byte)? as u32;
        accumulator = (accumulator << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((accumulator >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_type_is_split_from_its_charset() {
        assert_eq!(
            split_content_type("text/html; charset=UTF-8"),
            ("text/html".to_string(), Some("utf-8".to_string()))
        );
        assert_eq!(
            split_content_type("TEXT/CSS"),
            ("text/css".to_string(), None)
        );
        assert_eq!(
            split_content_type("text/plain; charset=\"iso-8859-1\""),
            ("text/plain".to_string(), Some("iso-8859-1".to_string()))
        );
    }

    #[test]
    fn extensions_map_to_mime_types() {
        use std::path::Path;
        assert_eq!(content_type_for_path(Path::new("a/b.html")), "text/html");
        assert_eq!(content_type_for_path(Path::new("a/b.PNG")), "image/png");
        assert_eq!(
            content_type_for_path(Path::new("a/b.unknown")),
            "application/octet-stream"
        );
        assert_eq!(
            content_type_for_path(Path::new("noextension")),
            "application/octet-stream"
        );
    }

    #[test]
    fn sniffing_recognises_html_and_images() {
        assert_eq!(
            sniff_content_type(b"<!DOCTYPE html><p>x").as_deref(),
            Some("text/html")
        );
        assert_eq!(
            sniff_content_type(b"  \n<html>").as_deref(),
            Some("text/html")
        );
        assert_eq!(
            sniff_content_type(&[0x89, b'P', b'N', b'G', 0, 0]).as_deref(),
            Some("image/png")
        );
        assert_eq!(sniff_content_type(b"just text"), None);
    }

    #[test]
    fn plain_data_urls_decode() {
        let address = Address::parse("data:text/html,%3Cp%3Ehi%3C/p%3E").unwrap();
        let resource = decode_data_url(&address).unwrap();
        assert_eq!(resource.content_type, "text/html");
        assert_eq!(resource.text(), "<p>hi</p>");
    }

    #[test]
    fn base64_data_urls_decode() {
        // "Hello" in base64.
        let address = Address::parse("data:text/plain;base64,SGVsbG8=").unwrap();
        let resource = decode_data_url(&address).unwrap();
        assert_eq!(resource.text(), "Hello");
    }

    #[test]
    fn data_urls_without_a_type_default_to_text() {
        let address = Address::parse("data:,hello").unwrap();
        let resource = decode_data_url(&address).unwrap();
        assert_eq!(resource.content_type, "text/plain");
        assert_eq!(resource.text(), "hello");
    }

    #[test]
    fn malformed_data_urls_are_rejected() {
        let address = Address::parse("data:text/plain").unwrap();
        assert!(matches!(
            decode_data_url(&address),
            Err(LoadError::BadUrl(_))
        ));
    }

    #[test]
    fn base64_tolerates_whitespace_and_missing_padding() {
        assert_eq!(decode_base64("SGVs bG8").unwrap(), b"Hello");
        assert_eq!(decode_base64("SGVsbG8=").unwrap(), b"Hello");
        assert!(decode_base64("not*valid").is_none());
    }

    #[test]
    fn percent_decoding_handles_trailing_garbage() {
        assert_eq!(percent_decode("a%20b"), b"a b");
        assert_eq!(percent_decode("a%2"), b"a%2");
        assert_eq!(percent_decode("a%zzb"), b"a%zzb");
    }

    #[test]
    fn files_are_read_from_disk() {
        let directory = std::env::temp_dir().join("wat-net-file-test");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("page.html");
        std::fs::write(&path, "<p>from disk</p>").unwrap();

        let url = url::Url::from_file_path(&path).unwrap();
        let address = Address::parse(url.as_str()).unwrap();
        let resource = HttpLoader::default().load(&address).expect("readable");
        assert_eq!(resource.content_type, "text/html");
        assert_eq!(resource.text(), "<p>from disk</p>");

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn missing_files_report_an_io_error() {
        // Built from a real directory rather than written out by hand: a
        // Unix-shaped path such as `/nope.html` has no Windows equivalent, so
        // there it is a bad URL rather than a missing file, and the test would
        // be asserting the wrong thing.
        let missing = std::env::temp_dir().join("wat-definitely-not-here.html");
        std::fs::remove_file(&missing).ok();
        let url = url::Url::from_file_path(&missing).unwrap();
        let address = Address::parse(url.as_str()).unwrap();

        match HttpLoader::default().load(&address) {
            Err(LoadError::Io(message)) => {
                assert!(message.contains("wat-definitely-not-here"), "{message}")
            }
            other => panic!("expected an IO error, got {other:?}"),
        }
    }

    #[test]
    fn a_file_url_that_names_no_path_on_this_platform_is_a_bad_url() {
        // `to_file_path` refuses a URL it cannot map, and that is a URL problem
        // rather than a missing file. On Unix every absolute path maps, so this
        // uses a host, which never does.
        let address = Address::parse("file://a-host/share/page.html").unwrap();
        match HttpLoader::default().load(&address) {
            Err(LoadError::BadUrl(_)) | Err(LoadError::Io(_)) => {}
            other => panic!("expected the URL to be rejected, got {other:?}"),
        }
    }

    #[test]
    fn directories_render_as_a_listing() {
        let directory = std::env::temp_dir().join("wat-net-listing-test");
        std::fs::create_dir_all(directory.join("sub")).unwrap();
        std::fs::write(directory.join("a.txt"), "x").unwrap();

        let url = url::Url::from_file_path(&directory).unwrap();
        let address = Address::parse(url.as_str()).unwrap();
        let resource = HttpLoader::default().load(&address).expect("listable");
        let html = resource.text();
        assert!(resource.is_html());
        assert!(html.contains("a.txt"));
        assert!(html.contains("sub/"));

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn about_urls_are_not_this_loaders_job() {
        let address = Address::parse("about:home").unwrap();
        assert!(matches!(
            HttpLoader::default().load(&address),
            Err(LoadError::UnsupportedScheme(_))
        ));
    }

    #[test]
    fn the_user_agent_names_the_browser() {
        let options = RequestOptions::default();
        assert!(options.user_agent.contains("WhatABrowser"));
        assert!(options.max_redirects > 0);
    }
}
