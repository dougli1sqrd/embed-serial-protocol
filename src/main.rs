use std::{collections::VecDeque, convert::Infallible};

use embed_serial_protocol::{Decode, Encode, Frame, FrameTxRx, MAX_FRAME_SIZE, packet::FrameRx};
use embedded_hal_nb::serial::{ErrorType, Read, Write};

fn main() {

}

#[derive(Debug)]
struct TxBuffer(Vec<u8>);

impl TxBuffer {
    fn new() -> TxBuffer {
        TxBuffer(Vec::new())
    }
}

impl ErrorType for TxBuffer {
    type Error = Infallible;
}

impl Write for TxBuffer {
    fn write(&mut self, c: u8) -> nb::Result<(), Self::Error> {
        self.0.push(c);
        Ok(())
    }

    fn flush(&mut self) -> nb::Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Debug)]
struct ReadBuffer(pub VecDeque<u8>);

impl ReadBuffer {
    fn from_iter(data: impl Iterator<Item = u8>) -> ReadBuffer {
        let q = VecDeque::from_iter(data);
        ReadBuffer(q)
    }
}

impl ErrorType for ReadBuffer {
    type Error = Infallible;
}

impl Read for ReadBuffer {
    fn read(&mut self) -> nb::Result<u8, Self::Error> {
        self.0.pop_front().ok_or(nb::Error::WouldBlock)
    }
}
