use bytes::{Bytes, BytesMut};
use futures_util::{StreamExt, stream};
use http::HeaderMap;
use http_body_util::{BodyExt, BodyStream, Full, StreamBody};
use hyper::body::Frame;

use super::{Body, BoxError};

pub struct Buffered {
    pub data: Bytes,
    pub trailers: Option<HeaderMap>,
    rest: Option<Body>,
}

impl Buffered {
    pub fn is_complete(&self) -> bool {
        self.rest.is_none()
    }

    pub fn into_body(self) -> Body {
        replay(self.data, self.trailers, self.rest)
    }
}

pub async fn collect(mut body: Body, limit: usize) -> Result<Buffered, BoxError> {
    let mut data = BytesMut::new();
    let mut trailers = None;

    while let Some(frame) = body.frame().await {
        match frame?.into_data() {
            Ok(chunk) => data.extend_from_slice(&chunk),
            Err(frame) => trailers = frame.into_trailers().ok().or(trailers),
        }

        if data.len() > limit {
            return Ok(Buffered {
                data: data.freeze(),
                trailers,
                rest: Some(body),
            });
        }
    }

    Ok(Buffered {
        data: data.freeze(),
        trailers,
        rest: None,
    })
}

pub fn replay(data: Bytes, trailers: Option<HeaderMap>, rest: Option<Body>) -> Body {
    if trailers.is_none() && rest.is_none() {
        return Full::new(data)
            .map_err(|never| match never {})
            .boxed_unsync();
    }

    let head = [
        (!data.is_empty()).then(|| Frame::data(data)),
        trailers.map(Frame::trailers),
    ]
    .into_iter()
    .flatten()
    .map(Ok);

    let tail = stream::iter(rest).flat_map(BodyStream::new);

    StreamBody::new(stream::iter(head).chain(tail)).boxed_unsync()
}
