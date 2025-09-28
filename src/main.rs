use std::{collections::VecDeque, convert::Infallible};

use embed_serial_protocol::{read_transaction, write_transaction, Decode, Encode, Frame, Kind, Packet, SerialConnection, TransactionState};
use embedded_io::{ErrorType, Read, Write};

fn main() {
    // println!("Hello");
    // let mut test_write = TxBuffer::new();
    // // let data = [4, 9, 9, 9, 4, 8, 8, 8];
    let data = "ASCII is the standardisation of a seven-bit teleprinter code developed in part from earlier telegraph codes. Work on the ASCII standard began in May 1961, when IBM engineer Bob Bemer submitted a proposal to the American Standards Association's (ASA) (now the American National Standards Institute or ANSI) X3.2 subcommittee.[7] The first edition of the standard was published in 1963,[8] contemporaneously with the introduction of the Teletype Model 33. It later underwent a major revision in 1967,[9][10] and several further revisions until 1986.".as_bytes();
    // write_transaction(0x4224, 0x01, &data, &mut test_write);
    // // println!("{:02x?}", &test_write.0.as_slice());
    // println!("len {}", test_write.0.as_slice().len());
    // println!("{:?}", &test_write.0.as_slice());
    // let data_bytes = test_write.0.as_slice();

    // println!("Read now");
    // let data = [170, 85, 66, 36, 255, 1, 0, 68, 111, 110, 97, 108, 100, 32, 84, 114, 117, 109, 112, 32, 97, 115, 107, 101, 100, 32, 84, 101, 120, 97, 115, 32, 116, 111, 32, 114, 101, 100, 114, 97, 119, 32, 116, 104, 101, 105, 114, 32, 67, 111, 110, 103, 114, 101, 115, 115, 105, 111, 110, 97, 108, 32, 109, 97, 112, 115, 32, 116, 111, 32, 102, 105, 110, 100, 32, 104, 105, 109, 32, 102, 105, 118, 101, 32, 115, 101, 97, 116, 115, 46, 32, 73, 116, 39, 115, 32, 97, 32, 100, 101, 101, 112, 108, 121, 32, 117, 110, 102, 97, 105, 114, 32, 97, 116, 116, 101, 109, 112, 116, 32, 116, 111, 32, 114, 105, 103, 32, 116, 104, 101, 32, 50, 48, 50, 54, 32, 72, 111, 117, 115, 101, 32, 101, 108, 101, 99, 116, 105, 111, 110, 32, 105, 110, 32, 104, 105, 115, 32, 102, 97, 118, 111, 114, 46, 10, 67, 97, 108, 105, 102, 111, 114, 110, 105, 97, 32, 119, 105, 108, 108, 32, 114, 101, 115, 112, 111, 110, 100, 32, 98, 121, 32, 114, 101, 100, 114, 97, 119, 105, 110, 103, 32, 111, 117, 114, 32, 109, 97, 112, 115, 44, 32, 98, 117, 116, 32, 111, 110, 108, 121, 32, 105, 102, 32, 118, 111, 116, 101, 114, 115, 32, 108, 105, 107, 101, 32, 121, 111, 117, 32, 97, 112, 112, 114, 111, 118, 101, 32, 116, 104, 101, 32, 109, 101, 97, 115, 117, 114, 101, 32, 105, 110, 196, 170, 85, 66, 36, 38, 1, 3, 32, 97, 110, 32, 101, 108, 101, 99, 116, 105, 111, 110, 32, 99, 111, 109, 105, 110, 103, 32, 117, 112, 32, 105, 110, 32, 97, 32, 102, 101, 119, 32, 119, 101, 101, 107, 115, 46, 219];
    // let mut read_buf = ReadBuffer::from_iter(data_bytes.iter().copied());
    // let read = read_transaction(&mut read_buf, String::new(), |mut accum, next| {
    //     let s = str::from_utf8(next).unwrap();
    //     accum.push_str(s);
    //     accum
    // });
    // println!("{:?}", read);
    let data = "ASCII is the standardisation of a seven-bit teleprinter".as_bytes();
    let frame = Frame::new(data);
    println!("frame: {:?}", frame);
    let mut buffer = [0; 255];
    frame.encode(&mut buffer);
    println!("{:?}", buffer);
    let x = Frame::decode(&buffer);
    println!("{:?}", x);

    println!("Packet!");

    let p = Packet::new(Kind::Data, 0x01, data);
    let mut buffer = [0; 255];
    p.encode(&mut buffer);
    println!("Packet: {:?}", &buffer[0..p.size()]);
    let x = Packet::decode(&buffer);
    println!("{:?}", x);

    println!("Serial Connection!");
    let tx = TxBuffer::new();
    let p = Packet::new(Kind::Ack, 0x01, &[]);
    let mut pbuf = [0; 3];
    p.encode(&mut pbuf);
    let f = Frame::new(&pbuf);
    let mut fbuf = [0; 10];
    f.encode(&mut fbuf);
    let rx = ReadBuffer(VecDeque::from(fbuf));
    let mut serial = SerialConnection::new(tx, rx);
    serial.send(&[9, 9, 9], 0x1).unwrap();

    println!("Serial: {:?}", serial);


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
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        for x in buf {
            self.0.push(*x);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Debug)]
struct ReadBuffer(VecDeque<u8>);

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
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.len() >= 1 {
            match self.0.pop_front() {
                Some(x) => {
                    buf[0] = x;
                    Ok(1)
                }
                None => Ok(0),
            }
        } else {
            Ok(0)
        }
    }
}
