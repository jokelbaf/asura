use std::path::Path;

use http::header::HOST;
use http::{HeaderMap, HeaderName, HeaderValue, request, response};
use hyper::ext::ReasonPhrase;
use session::{Body, Headers, Request, Response, Scheme, Version};

use crate::observe::Scope;

pub fn headers(map: &HeaderMap) -> Headers {
    let mut headers = Headers::default();

    for (name, value) in map {
        headers.push(name.as_str(), value.as_bytes());
    }

    headers
}

pub fn map(headers: &Headers) -> HeaderMap {
    let mut map = HeaderMap::with_capacity(headers.len());

    for (name, value) in headers.iter() {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_bytes(value),
        ) {
            map.append(name, value);
        }
    }

    map
}

pub fn version(version: http::Version) -> Version {
    match version {
        http::Version::HTTP_10 | http::Version::HTTP_09 => Version::Http10,
        http::Version::HTTP_2 => Version::Http2,
        http::Version::HTTP_3 => Version::Http3,
        _ => Version::Http11,
    }
}

pub fn request(scope: &Scope, parts: &request::Parts) -> Request {
    let authority = parts
        .uri
        .authority()
        .map(ToString::to_string)
        .or_else(|| {
            parts
                .headers
                .get(HOST)
                .and_then(|host| host.to_str().ok())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| scope.connection.server.to_string());

    let scheme = match parts.uri.scheme_str() {
        Some("http") => Scheme::Http,
        Some("https") => Scheme::Https,
        _ => scope.scheme,
    };

    let target = match parts.version {
        http::Version::HTTP_2 => parts
            .uri
            .path_and_query()
            .map_or("/", |path| path.as_str())
            .to_owned(),
        _ => parts.uri.to_string(),
    };

    Request {
        method: parts.method.to_string(),
        scheme,
        authority,
        target,
        version: version(parts.version),
        headers: headers(&parts.headers),
        trailers: Headers::default(),
        body: Body::default(),
    }
}

pub fn response(parts: &response::Parts) -> Response {
    let reason = match (parts.extensions.get::<ReasonPhrase>(), parts.version) {
        (Some(reason), _) => String::from_utf8_lossy(reason.as_bytes()).into_owned(),
        (None, http::Version::HTTP_2) => String::new(),
        (None, _) => parts
            .status
            .canonical_reason()
            .unwrap_or_default()
            .to_owned(),
    };

    Response {
        status: parts.status.as_u16(),
        reason,
        version: version(parts.version),
        headers: headers(&parts.headers),
        trailers: Headers::default(),
        body: Body::default(),
    }
}

pub fn filled(bytes: &[u8], spool: &Path) -> Body {
    let mut body = Body::default();
    let _ = body.append(bytes, spool);
    body.finish();
    body
}
