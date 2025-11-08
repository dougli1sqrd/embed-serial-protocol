extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;
use crc::Crc;
use embedded_hal_nb::serial::{ErrorType, Read, Write};
use embedded_io::Write as IoWrite;

use crate::{
    Decode, Encode,
    serial::{BufferedRx, BufferedTx, ErrorShim},
};

/// size field is a u8, so max amount of data is u8::MAX (255)
pub const MAX_DATA_SIZE: usize = u8::MAX as usize;
/// Start: 1, Size: 1, Data: MAX_DATA_SIZE, CRC: 1, End: 1
pub const MAX_FRAME_SIZE: usize = MAX_DATA_SIZE + 4;
/// Start and End byte of a Frame
pub const DELIMITER: u8 = 0x55;

/// Frames consist of a Start Delimiter, Size byte,
/// the packaged data, CRC byte, and End Delimiter.
#[derive(Debug)]
pub struct Frame {
    pub size: u8,
    pub data: Vec<u8>,
    pub crc: u8,
}

impl Frame {
    /// Length in a slice this frame occupies including start and end Delimiters
    pub fn len(&self) -> usize {
        self.data.len() + 4
    }
}

/// Byte Slice
pub type FrameDataSlice<'a> = &'a [u8];

/// Error type for encoding and decoding Frames
#[derive(Debug)]
pub enum FrameError {
    MissingStartDelim,
    MissingEndDelim { index: usize, found: u8 },
    EncodeBufferTooSmall {
        expected: usize,
        found: usize,
    },
    DecodeBufferTooSmall {
        expected_at_least: usize,
        found: usize,
    },
    CrcMismatch {
        calculated: u8,
        found: u8,
    },
    Debug(String),
}

impl<'a> Encode for FrameDataSlice<'a> {
    type Error = FrameError;

    fn encode(&self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        // Frame start
        buffer[0] = DELIMITER;
        // size byte. If Self is too large then we'll just
        // grab the first MAX_DATA_SIZE bytes *shrug*
        let size = MAX_DATA_SIZE.min(self.len());
        buffer[1] = size as u8;

        // Check buffer length
        // Required length is 1 from start Delim, 1 from size byte,
        // size from data, 1 from crc, 1 from end Delim
        if buffer.len() < size + 4 {
            return Err(FrameError::EncodeBufferTooSmall {
                expected: size + 4,
                found: buffer.len(),
            });
        }

        // Copy data from Self to buffer
        let data = &self[0..size];
        let data_buf = &mut buffer[2..size + 2];
        data_buf.copy_from_slice(data);

        // CRC
        let c = Crc::<u8>::new(&crc::CRC_8_MAXIM_DOW);
        let mut d = c.digest();
        d.update(&[size as u8]);
        d.update(data);
        let crc = d.finalize();
        buffer[size + 2] = crc;

        // End delim
        buffer[size + 3] = DELIMITER;

        Ok(size + 4)
    }
}

impl<'a> Decode<'_> for Frame {
    type Error = FrameError;

    fn decode(data: &'_ [u8]) -> Result<Self, Self::Error> {
        // return Err(FrameError::Debug(alloc::format!("{:?}", data)));
        // Check data non zero size
        if data.len() < 4 {
            return Err(FrameError::DecodeBufferTooSmall {
                expected_at_least: 4,
                found: data.len(),
            });
        }
        // Check start delimiter
        if data[0] != DELIMITER {
            return Err(FrameError::MissingStartDelim);
        }
        // Grab size byte
        let size = data[1] as usize;

        // Check data size
        if data.len() < size + 4 {
            return Err(FrameError::DecodeBufferTooSmall {
                expected_at_least: size + 4,
                found: data.len(),
            });
        }

        // Grab data vec
        let p = data[2..size + 2].to_vec();

        // CRC
        let c = Crc::<u8>::new(&crc::CRC_8_MAXIM_DOW);
        let mut d = c.digest();
        d.update(&[size as u8]);
        d.update(p.as_ref());
        let calc_crc = d.finalize();
        let crc = data[size + 2];
        if crc != calc_crc {
            return Err(FrameError::CrcMismatch {
                calculated: calc_crc,
                found: crc,
            });
        }

        // Check end delimiter
        if data[size + 3] != DELIMITER {
            return Err(FrameError::MissingEndDelim { index: size + 3, found: data[size+3] });
        }

        Ok(Frame {
            size: size as u8,
            data: p,
            crc,
        })
    }
}

#[derive(Debug)]
pub enum FrameIOError<WriteError, ReadError> {
    Frame(FrameError),
    Write(WriteError),
    Read(ReadError),
}

impl<Ew, Er> From<FrameError> for FrameIOError<Ew, Er> {
    fn from(value: FrameError) -> Self {
        FrameIOError::Frame(value)
    }
}

impl<Ew: ErrorType, Er> From<Ew> for FrameIOError<Ew, Er> {
    fn from(value: Ew) -> Self {
        FrameIOError::Write(value)
    }
}

pub struct FrameTxRx<Tx: Write, Rx: Read> {
    tx: BufferedTx<Tx>,
    rx: BufferedRx<Rx>,
}

impl<Tx: Write, Rx: Read> FrameTxRx<Tx, Rx> {
    pub fn new(tx: Tx, rx: Rx) -> FrameTxRx<Tx, Rx> {
        FrameTxRx {
            tx: BufferedTx::new(tx),
            rx: BufferedRx::new(rx),
        }
    }

    pub fn send(&mut self, data: &[u8]) -> Result<(), FrameIOError<Tx::Error, Rx::Error>> {
        // We can send the whole encoded frame into Tx. Tx is Buffered so
        // Even if it doesn't all go down the wire right away it'll at least
        // be in the buffer and will eventually flush.
        let mut buf = [0; MAX_DATA_SIZE + 4];
        let size = data.encode(&mut buf).map_err(FrameIOError::from)?;
        match IoWrite::write(&mut self.tx, &buf[0..size]) {
            Ok(_) => Ok(()),
            Err(ErrorShim(e)) => Err(FrameIOError::Write(e)),
        }
    }

    pub fn recv(&mut self) -> nb::Result<Frame, FrameIOError<Tx::Error, Rx::Error>> {
        // Throw away any garbage that isn't the Delimiter
        // If rx.read() WouldBlock, then the buffer is empty and we WouldBlock also
        self.rx.buffer();
        loop {
            if let Some(DELIMITER) = self.rx.peek() {
                break;
            }
            // If we block or error, then return
            if let Err(e) = self.rx.read() {
                match e {
                    nb::Error::WouldBlock => return Err(nb::Error::WouldBlock),
                    nb::Error::Other(ee) => return Err(nb::Error::Other(FrameIOError::Read(ee))),
                }
            }
        }
        // We need to check if the underlying Rx buffer contains a frame
        // We'll only return if we have a potential Frame to decode
        let x = self.rx.front_back_find(DELIMITER);
        // return Err(nb::Error::Other(FrameIOError::Frame(FrameError::Debug(alloc::format!("{:?}", x)))));
        if x.is_some() {
            // We've found two different DELIMITERs, so try to make a frame
            let buf = self.rx.slice();
            let f = Frame::decode(buf).map_err(FrameIOError::from)?;
            // Deque all the elements in the slice we just made into a Frame
            self.rx.drain(f.len());
            return Ok(f)
        } else {
            return Err(nb::Error::WouldBlock);
        }
        
    }

    pub fn split(self) -> (BufferedTx<Tx>, BufferedRx<Rx>) {
        (self.tx, self.rx)
    }
}
