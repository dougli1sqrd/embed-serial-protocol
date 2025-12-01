extern crate alloc;

use core::convert::Infallible;

use alloc::string::String;
use alloc::vec::Vec;
use crc::Crc;
use embedded_hal_nb::serial::{ErrorType, Read, Write};
use embedded_io::Write as IoWrite;

use crate::{
    Decode, Encode,
    serial::{BufferedRx, BufferedRxTx, BufferedTx, ErrorShim, ReadAmt},
};

/// size field is a u8, so max amount of data is u8::MAX (255)
pub const MAX_DATA_SIZE: usize = u8::MAX as usize;
/// Start: 1, Size: 1, Data: MAX_DATA_SIZE, CRC: 1, End: 1
pub const MAX_FRAME_SIZE: usize = MAX_DATA_SIZE + 4;
/// Start and End byte of a Frame
pub const DELIMITER: u8 = 0x55;
pub const END_DELIM: u8 = 0xAA;

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
    MissingEndDelim {
        index: usize,
        found: u8,
    },
    EarlyEndDelim {
        found_at: usize,
        expected: usize,
    },
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
        buf: Vec<u8>
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
        // size from data, 1 from crc, 1 from end delim
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
        buffer[size + 3] = END_DELIM;

        Ok(size + 4)
    }
}

impl<'a> Decode<'_> for Frame {
    type Error = FrameError;

    fn decode(data: &'_ [u8]) -> Result<Self, Self::Error> {

        // Check data has at least a zero length data frame
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
        // Delimiter: 1, Size: 1, crc: 1, End Delim: 1
        if data.len() < size + 4 {
            return Err(FrameError::DecodeBufferTooSmall {
                expected_at_least: size + 4,
                found: data.len(),
            });
        }

        // Grab data vec
        let p = data[2..size + 2].to_vec();

        // Check size of the frame by going to the end delimiter position and
        // walking backwards until we find it
        if data[size + 3] != END_DELIM {
            for i in (0..=size + 3).rev() {
                if data[i] == END_DELIM {
                    return Err(FrameError::EarlyEndDelim { found_at: i, expected: size+3 })
                }
            }
            // If we get here then the end delimiter is totally missing.
            // We should still check CRC
        }

        // CRC
        let c = Crc::<u8>::new(&crc::CRC_8_MAXIM_DOW);
        let mut d = c.digest();
        d.update(&[size as u8]);
        d.update(p.as_ref());
        let calc_crc = d.finalize();
        let crc = data[size + 2];
        if crc != calc_crc {
            // Now a CRC check only fails if a decoded frame is known to be the same size
            // based on the position of the end delimiter.
            return Err(FrameError::CrcMismatch {
                calculated: calc_crc,
                found: crc,
                buf: Vec::from(data)
            });
        }

        // If data is good, double check End Delim
        if data[size + 3] != END_DELIM {
            return Err(FrameError::MissingEndDelim { index: size+3, found: data[size+3] })
        }

        Ok(Frame {
            size: size as u8,
            data: p,
            crc,
        })
    }
}

pub fn recv_frame<Rx: Read>(rx: &mut BufferedRx<Rx>) -> nb::Result<Frame, FrameError> {
    // Cycle through bytes until we get to the delimiter
    let sl = rx.slice();
    let delim = sl.iter().enumerate().find(|(_, x)| **x == DELIMITER);
    match delim {
        Some((i, _)) => {
            let _ = rx.read_amt(i);
        },
        None => {
            // If we don't find the delimiter drain everything
            let _ = rx.read_amt(sl.len());
        }
    }
    match Frame::decode(rx.slice()) {
        Ok(f) => {
            // drain the bytes we took
            let _ = rx.read_amt(f.len());
            Ok(f)
        },
        Err(e) => {
            match e {
                FrameError::DecodeBufferTooSmall { expected_at_least: _, found: _ } => Err(nb::Error::WouldBlock),
                e => Err(nb::Error::Other(e))
            }
        }
    }
}

pub fn send_frame<Tx: Write>(tx: &mut BufferedTx<Tx>, data: &[u8]) -> Result<(), FrameError> {
    let mut buf = [0; MAX_FRAME_SIZE];
    let size = data.encode(&mut buf)?;
    for b in &buf[0..size] {
        let _ = embedded_hal_nb::serial::Write::write(tx, *b);
    }
    Ok(())
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
    ftx: FrameTx<Tx>,
    pub frx: FrameRx<Rx>,
}

impl<Tx: Write, Rx: Read> FrameTxRx<Tx, Rx> {
    pub fn new(tx: Tx, rx: Rx) -> FrameTxRx<Tx, Rx> {
        FrameTxRx {
            ftx: FrameTx::new(tx),
            frx: FrameRx::new(rx),
        }
    }

    pub fn split(self) -> (BufferedTx<Tx>, BufferedRx<Rx>) {
        (self.ftx.tx, self.frx.rx)
    }
}

impl<Tx: Write, Rx: Read> FrameSend<Tx> for FrameTxRx<Tx, Rx> {
    fn flush(&mut self) -> nb::Result<(), <Tx>::Error> {
        self.ftx.flush()
    }

    fn send(&mut self, data: &[u8]) -> Result<(), FrameIOError<<Tx>::Error, Infallible>> {
        self.ftx.send(data)
    }
}

impl<Tx: Write, Rx: Read> FrameRecv<Rx> for FrameTxRx<Tx, Rx> {
    fn buffer(&mut self) -> nb::Result<(), <Rx>::Error> {
        self.frx.buffer()
    }

    fn recv(&mut self) -> nb::Result<Frame, FrameIOError<Infallible, <Rx>::Error>> {
        self.frx.recv()
    }
}

pub trait FrameRecv<Rx: Read> {
    fn buffer(&mut self) -> nb::Result<(), Rx::Error>;

    fn recv(&mut self) -> nb::Result<Frame, FrameIOError<Infallible, Rx::Error>>;
}

pub trait FrameSend<Tx: Write> {
    fn flush(&mut self) -> nb::Result<(), Tx::Error>;

    fn send(&mut self, data: &[u8]) -> Result<(), FrameIOError<Tx::Error, Infallible>>;
}

pub struct FrameRx<Rx: Read> {
    pub rx: BufferedRx<Rx>,
}

impl<Rx: Read> FrameRx<Rx> {
    pub fn new(rx: Rx) -> FrameRx<Rx> {
        FrameRx { rx: BufferedRx::new(rx) }
    }
}
    

impl<Rx: Read> FrameRecv<Rx> for FrameRx<Rx> {

    /// Read as much as we can out of the underlying Read until
    /// we WouldBlock or Error
    /// TODO no accounting for just a flood of input on Rx and
    /// we end up blocking by accident anyway.
    fn buffer(&mut self) -> nb::Result<(), <Rx>::Error> {
        self.rx.buffer()
    }

    fn recv(&mut self) -> nb::Result<Frame, FrameIOError<Infallible, <Rx>::Error>> {
        
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
        // At this point we just have to try and make a frame from the buffer.
        // If we can make a frame, then we have one.
        let buf = self.rx.slice();
        match Frame::decode(buf) {
            Ok(f) => {
                self.rx.drain(f.len());
                return Ok(f)
            },
            Err(FrameError::EarlyEndDelim { found_at, expected }) => {
                // If we think we're on a frame, then the current next read will be the
                // Frame delimiter. We should pop this so the next time we `recv` we'll
                // toss the bytes until the next Delimiter
                let _ = self.rx.rx.read();
                return Err(nb::Error::Other(FrameIOError::Frame(FrameError::EarlyEndDelim { found_at, expected })))
            },
            Err(FrameError::CrcMismatch { calculated, found, buf }) => {
                // If we think we're on a frame, then the current next read will be the
                // Frame delimiter. We should pop this so the next time we `recv` we'll
                // toss the bytes until the next Delimiter
                let _ = self.rx.rx.read();
                return Err(nb::Error::Other(FrameIOError::Frame(FrameError::CrcMismatch { calculated, found, buf })))
            },
            Err(FrameError::MissingEndDelim { index, found }) => {
                // If we think we're on a frame, then the current next read will be the
                // Frame delimiter. We should pop this so the next time we `recv` we'll
                // toss the bytes until the next Delimiter
                let _ = self.rx.rx.read();
                return Err(nb::Error::Other(FrameIOError::Frame(FrameError::MissingEndDelim { index, found })))
            },
            Err(FrameError::DecodeBufferTooSmall { expected_at_least: _, found: _ }) => {
                return Err(nb::Error::WouldBlock)
            },
            Err(e) => {
                // Other errors are essentially due to not having enough data
                return Err(nb::Error::Other(FrameIOError::Frame(e)))
            },
        }
    }
}

pub struct FrameTx<Tx: Write> {
    pub tx: BufferedTx<Tx>
}

impl<Tx: Write> FrameTx<Tx> {
    pub fn new(tx: Tx) -> FrameTx<Tx> {
        FrameTx { tx: BufferedTx::new(tx) }
    }
}

impl<Tx: Write> FrameSend<Tx> for FrameTx<Tx> {
    fn flush(&mut self) -> nb::Result<(), <Tx>::Error> {
        embedded_hal_nb::serial::Write::flush(&mut self.tx)
    }

    fn send(&mut self, data: &[u8]) -> Result<(), FrameIOError<Tx::Error, Infallible>> {
        // We can send the whole encoded frame into Tx. Tx is Buffered so
        // Even if it doesn't all go down the wire right away it'll at least
        // be in the buffer and will eventually flush.
        let mut buf = [0; MAX_FRAME_SIZE];
        let size = data.encode(&mut buf).map_err(FrameIOError::from)?;
        match IoWrite::write(&mut self.tx, &buf[0..size]) {
            Ok(_) => Ok(()),
            Err(ErrorShim(e)) => Err(FrameIOError::Write(e)),
        }
    }
}
