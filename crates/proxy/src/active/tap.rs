use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, ready};

use bytes::Bytes;
use hyper::body::{Body, Frame, SizeHint};
use session::{Direction, Entry};
use time::OffsetDateTime;

use super::{BoxError, convert};

pub struct Tap<B: Body> {
    inner: B,
    entry: Arc<Entry>,
    direction: Direction,
    record: bool,
    complete: bool,
    done: bool,
}

impl<B: Body> Tap<B> {
    pub fn recording(inner: B, entry: Arc<Entry>, direction: Direction, complete: bool) -> Self {
        Self {
            inner,
            entry,
            direction,
            record: true,
            complete,
            done: false,
        }
    }

    pub fn completing(inner: B, entry: Arc<Entry>) -> Self {
        Self {
            inner,
            entry,
            direction: Direction::Inbound,
            record: false,
            complete: true,
            done: false,
        }
    }

    fn capture(&self, frame: &Frame<Bytes>) {
        if !self.record {
            return;
        }

        let direction = self.direction;

        if let Some(data) = frame.data_ref().filter(|data| !data.is_empty()) {
            self.entry.update(|record, spool| {
                let body = match direction {
                    Direction::Outbound => record.request.as_mut().map(|request| &mut request.body),
                    Direction::Inbound => {
                        record.response.as_mut().map(|response| &mut response.body)
                    }
                };

                if let Some(body) = body {
                    let _ = body.append(data, spool);
                }
            });
        }

        if let Some(trailers) = frame.trailers_ref() {
            let trailers = convert::headers(trailers);

            self.entry.update(|record, _| match direction {
                Direction::Outbound => {
                    if let Some(request) = &mut record.request {
                        request.trailers = trailers;
                    }
                }
                Direction::Inbound => {
                    if let Some(response) = &mut record.response {
                        response.trailers = trailers;
                    }
                }
            });
        }
    }

    fn finish(&mut self) {
        self.done = true;

        let (direction, recorded, complete) = (self.direction, self.record, self.complete);
        let time = OffsetDateTime::now_utc();

        self.entry.update(|record, _| match direction {
            Direction::Outbound => {
                if let Some(request) = record.request.as_mut().filter(|_| recorded) {
                    request.body.finish();
                }
                record.timing.request_complete = Some(time);
            }
            Direction::Inbound => {
                if let Some(response) = record.response.as_mut().filter(|_| recorded) {
                    response.body.finish();
                }
                record.timing.response_complete = Some(time);

                if complete {
                    record.complete();
                }
            }
        });
    }

    fn fail(&mut self, reason: String) {
        self.done = true;
        self.entry.update(|record, _| record.fail(reason));
    }
}

impl<B> Body for Tap<B>
where
    B: Body<Data = Bytes> + Unpin,
    B::Error: Into<BoxError>,
{
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let this = self.get_mut();

        match ready!(Pin::new(&mut this.inner).poll_frame(context)) {
            Some(Ok(frame)) => {
                this.capture(&frame);

                if this.inner.is_end_stream() {
                    this.finish();
                }

                Poll::Ready(Some(Ok(frame)))
            }
            Some(Err(error)) => {
                let error = error.into();
                this.fail(error.to_string());
                Poll::Ready(Some(Err(error)))
            }
            None => {
                if !this.done {
                    this.finish();
                }

                Poll::Ready(None)
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

impl<B: Body> Drop for Tap<B> {
    fn drop(&mut self) {
        if self.done {
            return;
        }

        if self.inner.is_end_stream() {
            self.finish();
        } else if self.complete {
            self.fail(String::from(
                "the connection closed before the response completed",
            ));
        }
    }
}
