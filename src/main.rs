use std::{collections::VecDeque, convert::Infallible};

use embed_serial_protocol::{Decode, Encode, Frame, FrameTxRx, MAX_FRAME_SIZE};
use embedded_hal_nb::serial::{ErrorType, Read, Write};

fn main() {
    // println!("Hello");
    // let mut test_write = TxBuffer::new();
    // // let data = [4, 9, 9, 9, 4, 8, 8, 8];
    let data = "ASCII is the standardisation of a seven-bit teleprinter code".as_bytes();
    let mut buf = [0; MAX_FRAME_SIZE];
    let r = data.encode(&mut buf);
    println!("{:?}", r);
    println!("buf = {:?}", buf);
    let f = Frame::decode(&buf);
    println!("{:?}", f);

    let rx = ReadBuffer(VecDeque::from(buf));
    println!("Rx Source: {:?}", rx);
    let tx = TxBuffer::new();
    let mut frame_txrx = FrameTxRx::new(tx, rx);
    let rec = frame_txrx.recv();
    println!("Receiving!");
    println!("{:?}", rec);
    println!("Sending!");
    let send = frame_txrx.send("hello world".as_bytes());
    println!("sent: {:?}", send);
    let (tx, rx) = frame_txrx.split();
    println!("{:?}", tx);
    println!("{:?}", rx.buf.as_slices());
    
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
