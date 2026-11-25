//! Bounded reads of downloaded subscriptions (audit 2026-11-25 NET-02).
//!
//! A subscription answer is read in chunks and never past [`MAX_SUBSCRIPTION_BYTES`]: a provider that answers with
//! more (by mistake or on purpose) gets the download refused instead of the client's memory, whether or not it sent
//! a `Content-Length`. The caller keeps the last good subscription, as for any failed download. Errors carry no URL:
//! a subscription URL holds its token.

use std::io::{self, Read};

/// The largest subscription read, from a file or the network: far above any real one.
pub const MAX_SUBSCRIPTION_BYTES: u64 = 16 * 1024 * 1024;

/// Chunk size of [`read_limited`].
const CHUNK: usize = 64 * 1024;

/// Why a bounded read failed.
#[derive(Debug)]
pub enum LimitedReadError {
    /// The answer is larger than the limit (its `Content-Length` said so, or more bytes arrived).
    TooLarge { limit: u64 },
    /// Reading the answer failed; the error is the transport's and may name the URL, so it is shown only by kind.
    Read(io::Error),
}

impl LimitedReadError {
    /// A message without the URL: the size for [`Self::TooLarge`], the kind of failure for [`Self::Read`].
    pub fn describe(&self) -> String {
        match self {
            Self::TooLarge { limit } => format!(
                "the answer is larger than {} MiB, which no subscription is",
                limit / (1024 * 1024)
            ),
            Self::Read(error) if error.kind() == io::ErrorKind::TimedOut => {
                "the download timed out".to_string()
            }
            Self::Read(_) => "the answer could not be read".to_string(),
        }
    }
}

/// Reads `reader` to its end, at most `limit` bytes. A `declared_length` (the `Content-Length`) above the limit is
/// refused before anything is read; an answer without one, or one that sends more than it declared, is cut off at
/// `limit + 1` bytes and refused. Memory use stays below `limit` plus one chunk.
pub fn read_limited(
    mut reader: impl Read,
    declared_length: Option<u64>,
    limit: u64,
) -> Result<Vec<u8>, LimitedReadError> {
    if declared_length.is_some_and(|length| length > limit) {
        return Err(LimitedReadError::TooLarge { limit });
    }
    let capacity = declared_length.unwrap_or(0).min(limit);
    let mut body = Vec::with_capacity(usize::try_from(capacity).unwrap_or(0));
    let mut chunk = vec![0_u8; CHUNK];
    loop {
        let read = match reader.read(&mut chunk) {
            Ok(0) => return Ok(body),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(LimitedReadError::Read(error)),
        };
        if body.len() as u64 + read as u64 > limit {
            return Err(LimitedReadError::TooLarge { limit });
        }
        body.extend_from_slice(&chunk[..read]);
    }
}

/// [`read_limited`] at [`MAX_SUBSCRIPTION_BYTES`], decoded as reqwest's `text()` does without its `charset`
/// feature: UTF-8, invalid sequences replaced.
pub fn read_subscription_text(
    reader: impl Read,
    declared_length: Option<u64>,
) -> Result<String, LimitedReadError> {
    let body = read_limited(reader, declared_length, MAX_SUBSCRIPTION_BYTES)?;
    Ok(match String::from_utf8(body) {
        Ok(text) => text,
        Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Endless bytes that count how many were taken.
    struct Endless {
        served: u64,
    }

    impl Read for Endless {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            buf.fill(b'a');
            self.served += buf.len() as u64;
            Ok(buf.len())
        }
    }

    #[test]
    fn reads_up_to_the_limit_exactly() {
        let limit = 3 * CHUNK as u64 + 17;
        let body = vec![b'x'; usize::try_from(limit).unwrap()];
        assert_eq!(read_limited(&body[..], None, limit).unwrap(), body);
        assert_eq!(read_limited(&body[..], Some(limit), limit).unwrap(), body);
        assert_eq!(read_limited(&b""[..], None, limit).unwrap(), b"");
    }

    #[test]
    fn refuses_one_byte_over_the_limit_without_a_length() {
        let limit = 3 * CHUNK as u64 + 17;
        let body = vec![b'x'; usize::try_from(limit + 1).unwrap()];
        assert!(matches!(
            read_limited(&body[..], None, limit),
            Err(LimitedReadError::TooLarge { .. })
        ));
    }

    #[test]
    fn refuses_a_declared_length_over_the_limit_before_reading() {
        let mut endless = Endless { served: 0 };
        let error = read_limited(
            &mut endless,
            Some(MAX_SUBSCRIPTION_BYTES + 1),
            MAX_SUBSCRIPTION_BYTES,
        )
        .unwrap_err();
        assert!(matches!(error, LimitedReadError::TooLarge { .. }));
        assert_eq!(endless.served, 0);
    }

    #[test]
    fn stops_an_endless_answer_near_the_limit() {
        // No Content-Length, or one that lies: reading stops within a chunk of the limit.
        for declared in [None, Some(10)] {
            let mut endless = Endless { served: 0 };
            let error = read_subscription_text(&mut endless, declared).unwrap_err();
            assert!(matches!(error, LimitedReadError::TooLarge { .. }));
            assert!(endless.served <= MAX_SUBSCRIPTION_BYTES + CHUNK as u64);
            assert_eq!(
                error.describe(),
                "the answer is larger than 16 MiB, which no subscription is"
            );
        }
    }

    #[test]
    fn read_errors_never_quote_the_transport_text() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other(
                    "error reading https://panel.example/sub?token=s3cret",
                ))
            }
        }
        let message = read_limited(Failing, None, 10).unwrap_err().describe();
        assert_eq!(message, "the answer could not be read");
        struct Slow;
        impl Read for Slow {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "https://panel.example/sub?token=s3cret",
                ))
            }
        }
        assert_eq!(
            read_limited(Slow, None, 10).unwrap_err().describe(),
            "the download timed out"
        );
    }

    #[test]
    fn decodes_like_reqwest_text() {
        assert_eq!(
            read_subscription_text(&b"ss://a"[..], None).unwrap(),
            "ss://a"
        );
        assert_eq!(
            read_subscription_text(&b"a\xffb"[..], None).unwrap(),
            "a\u{fffd}b"
        );
    }
}
