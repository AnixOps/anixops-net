//! One message per frame: a four-byte big-endian length, then that many bytes.

use std::io::{self, Read, Write};

/// The longest frame either side accepts. The largest messages are saved capture sessions with their
/// request and response bodies; a length above this is a broken or hostile peer, refused before anything is
/// allocated for it.
pub const MAX_LENGTH: usize = 64 * 1024 * 1024;

/// Writes `payload` as one frame and flushes it. The length and the bytes go out in one write, so a side
/// that closes the channel while a frame is being sent (a window quitting while its answer goes out) does
/// not leave the other side a length with nothing after it: the frame is either all sent or not begun.
pub fn write(writer: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_LENGTH {
        return Err(too_long(payload.len()));
    }
    let length = u32::try_from(payload.len()).map_err(|_| too_long(payload.len()))?;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(payload);
    writer.write_all(&frame)?;
    writer.flush()
}

/// Reads one frame. `None` when the channel closed before a new frame began; closing inside a frame is an
/// error.
pub fn read(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut prefix = [0_u8; 4];
    let mut filled = 0;
    while filled < prefix.len() {
        match reader.read(&mut prefix[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => return Err(cut_short()),
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    let length = u32::from_be_bytes(prefix) as usize;
    if length > MAX_LENGTH {
        return Err(too_long(length));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            cut_short()
        } else {
            error
        }
    })?;
    Ok(Some(payload))
}

fn too_long(length: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("a frame of {length} bytes is over the limit of {MAX_LENGTH}"),
    )
}

fn cut_short() -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "the channel closed inside a frame",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Hands out one byte per read, as a pipe may.
    struct Trickle(Cursor<Vec<u8>>);

    impl Read for Trickle {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let end = buffer.len().min(1);
            self.0.read(&mut buffer[..end])
        }
    }

    #[test]
    fn a_frame_is_its_length_then_its_bytes() {
        let mut bytes = Vec::new();
        write(&mut bytes, b"{}").unwrap();
        assert_eq!(bytes, [0, 0, 0, 2, b'{', b'}']);
    }

    /// Counts the writes it is handed.
    #[derive(Default)]
    struct Writes(Vec<Vec<u8>>);

    impl Write for Writes {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.0.push(buffer.to_vec());
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_frame_goes_out_in_one_write() {
        // Two writes let a close in between cut the frame after its length (the other side then reads
        // "the channel closed inside a frame").
        let mut writes = Writes::default();
        write(&mut writes, b"answer").unwrap();
        assert_eq!(writes.0, [b"\0\0\0\x06answer".to_vec()]);
    }

    #[test]
    fn frames_are_read_back_one_at_a_time() {
        let mut bytes = Vec::new();
        write(&mut bytes, b"first").unwrap();
        write(&mut bytes, b"").unwrap();
        write(&mut bytes, b"third").unwrap();
        let mut reader = Cursor::new(bytes);
        assert_eq!(read(&mut reader).unwrap().as_deref(), Some(&b"first"[..]));
        assert_eq!(read(&mut reader).unwrap().as_deref(), Some(&b""[..]));
        assert_eq!(read(&mut reader).unwrap().as_deref(), Some(&b"third"[..]));
        assert_eq!(read(&mut reader).unwrap(), None);
    }

    #[test]
    fn bytes_that_arrive_one_by_one_make_the_same_frame() {
        let mut bytes = Vec::new();
        write(&mut bytes, b"slow").unwrap();
        let mut reader = Trickle(Cursor::new(bytes));
        assert_eq!(read(&mut reader).unwrap().as_deref(), Some(&b"slow"[..]));
        assert_eq!(read(&mut reader).unwrap(), None);
    }

    #[test]
    fn closing_inside_a_frame_is_an_error() {
        // Inside the length.
        let error = read(&mut Cursor::new(vec![0, 0])).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        // Inside the bytes.
        let error = read(&mut Cursor::new(vec![0, 0, 0, 5, b'a'])).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn a_length_over_the_limit_is_refused_before_reading_on() {
        let length = u32::try_from(MAX_LENGTH + 1).unwrap();
        let error = read(&mut Cursor::new(length.to_be_bytes().to_vec())).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let error = read(&mut Cursor::new(vec![0xff; 4])).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
