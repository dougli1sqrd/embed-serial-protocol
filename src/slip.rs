use core::{marker::PhantomData, ops::Range};

use bilge::prelude::*;
use crc::Crc;
use embedded_io::{Read, Write};

use crate::{Decode, Encode};

pub trait Bytes<T>
where
    T: AsRef<[u8]>,
{
    fn bytes(&self) -> T;
}

const START: u8 = 0x55;

#[derive(Debug)]
struct Start;

impl Start {
    fn value(&self) -> u8 {
        START
    }
}

#[derive(Debug)]
pub struct FrameHeader {
    length: u8,
}

impl FrameHeader {
    const fn size() -> usize {
        1
    }

    fn bytes(&self) -> [u8; 1] {
        [self.length]
    }
}

impl Bytes<[u8; 1]> for FrameHeader {
    fn bytes(&self) -> [u8; 1] {
        [self.length]
    }
}

const MAX_FRAME_SIZE: usize = 1 + FrameHeader::size() + u8::MAX as usize + 1;

#[derive(Debug)]
pub struct Frame<'a> {
    start: Start,
    header: FrameHeader,
    data: &'a [u8],
    crc: u8,
}

impl<'a> Frame<'a> {
    pub fn new(data: &'a [u8]) -> Frame<'a> {
        let c = Crc::<u8>::new(&crc::CRC_8_MAXIM_DOW);
        let mut d = c.digest();
        let header = FrameHeader {
            length: data.len() as u8,
        };
        d.update(&header.bytes());
        d.update(data);
        let crc = d.finalize();
        Frame {
            start: Start,
            header,
            data,
            crc,
        }
    }

    fn size(&self) -> usize {
        1 + FrameHeader::size() + self.header.length as usize + 1
    }

    fn crc_indices(&self) -> Range<usize> {
        1..(self.data.len() + 1)
    }
}

#[derive(Debug)]
pub enum FramingError {
    MissingStart,
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
}

impl<'a> Encode for Frame<'a> {
    type Error = FramingError;

    fn encode(&self, buffer: &mut [u8]) -> Result<(), Self::Error> {
        if buffer.len() < self.size() {
            return Err(FramingError::EncodeBufferTooSmall {
                expected: self.size(),
                found: buffer.len(),
            });
        }
        // START: 1, HEADER: 1, DATA: length, CRC: 1
        //          |--------DIGEST----------|
        buffer[0] = self.start.value();
        buffer[1] = self.header.length;
        let crc_portion = &mut buffer[1..self.header.length as usize + 2];
        if crc_portion.len() != self.header.length as usize + 1 {
            panic!(
                "crc measured length is {}, and found constructed length {}",
                crc_portion.len(),
                self.header.length as usize + 1
            );
        }
        let data_portion = &mut crc_portion[1..self.header.length as usize + 1];
        data_portion.copy_from_slice(self.data);

        let c = Crc::<u8>::new(&crc::CRC_8_MAXIM_DOW);
        let mut d = c.digest();
        // digest everything we placed into the buffer up to the last byte
        d.update(&crc_portion);
        buffer[self.size() - 1] = d.finalize();
        Ok(())
    }
}

impl<'a> Decode<'a> for Frame<'a> {
    type Error = FramingError;

    fn decode(data: &'a [u8]) -> Result<Self, Self::Error> {
        if data[0] != START {
            return Err(FramingError::MissingStart);
        }
        let header = FrameHeader { length: data[1] };
        if data[2..].len() < header.length as usize {
            return Err(FramingError::DecodeBufferTooSmall {
                expected_at_least: header.length as usize,
                found: data[2..].len(),
            });
        }
        let payload = &data[2..header.length as usize + 2];
        let c = Crc::<u8>::new(&crc::CRC_8_MAXIM_DOW);
        let mut d = c.digest();
        d.update(&[header.length]);
        d.update(payload);
        let calc_crc = d.finalize();
        // The next byte after the end of actual payload
        let found_crc = data[1 + header.length as usize + 1];
        if found_crc != calc_crc {
            return Err(FramingError::CrcMismatch {
                calculated: calc_crc,
                found: found_crc,
            });
        }
        Ok(Frame {
            start: Start,
            header,
            data: payload,
            crc: found_crc,
        })
    }
}

const MAX_PACKET_SIZE: usize = u8::MAX as usize;
const MAX_PACKET_DATA_SIZE: usize = MAX_PACKET_SIZE - PacketHeader::size();

#[bitsize(24)]
#[derive(DebugBits, Clone, Copy, FromBits)]
pub struct PacketHeader {
    kind: Kind,
    _reserved: u5,
    convo_id: u8,
    pub length: u8,
}

impl PacketHeader {
    const fn size() -> usize {
        3
    }
}

impl Bytes<[u8; 3]> for PacketHeader {
    fn bytes(&self) -> [u8; 3] {
        [self.kind() as u8, self.convo_id(), self.length()]
    }
}

#[bitsize(3)]
#[derive(Debug, Clone, Copy, FromBits)]
pub enum Kind {
    Data = 0b000,
    DataContinue = 0b001,
    DataLast = 0b010,
    Ack = 0b100,
    Error = 0b101,
    #[fallback]
    Reserved = 0b110,
}

#[derive(Debug)]
pub struct Packet<'a> {
    pub header: PacketHeader,
    data: &'a [u8],
}

impl<'a> Packet<'a> {
    pub fn new(kind: Kind, convo_id: u8, data: &'a [u8]) -> Packet<'a> {
        let header = PacketHeader::new(kind, convo_id, data.len() as u8);
        Packet { header, data }
    }

    pub fn size(&self) -> usize {
        PacketHeader::size() + self.data.len()
    }

    pub fn owned(&self) -> PacketOwned {
        PacketOwned::new(self.header.kind(), self.header.convo_id(), self.data)
    }
}

impl<'a> Bytes<heapless::Vec<u8, MAX_PACKET_SIZE>> for Packet<'a> {
    fn bytes(&self) -> heapless::Vec<u8, MAX_PACKET_SIZE> {
        let mut v = heapless::Vec::<u8, MAX_PACKET_SIZE>::new();
        v.extend_from_slice(&self.header.bytes());
        v.extend_from_slice(self.data);
        v
    }
}

pub struct PacketOwned {
    pub header: PacketHeader,
    data: [u8; MAX_PACKET_DATA_SIZE],
}

impl PacketOwned {
    pub fn new(kind: Kind, convo_id: u8, data: &[u8]) -> PacketOwned {
        let header = PacketHeader::new(kind, convo_id, data.len() as u8);
        let mut buf = [0; MAX_PACKET_DATA_SIZE];
        let buf_data = &mut buf[0..data.len()];
        buf_data.copy_from_slice(data);
        PacketOwned { header, data: buf }
    }

    pub fn size(&self) -> usize {
        PacketHeader::size() + self.data.len()
    }

    pub fn borrowed(&self) -> Packet {
        Packet {
            header: self.header.clone(),
            data: &self.data[0..self.header.length() as usize],
        }
    }
}

#[derive(Debug)]
pub enum PacketError {
    EncodeBufferTooSmall {
        expected_at_least: usize,
        found: usize,
    },
    DecodeBufferTooSmall {
        expected_at_least: usize,
        found: usize,
    },
    NotEnoughDataForHeader {
        expected: usize,
        found: usize,
    },
}

impl<'a> Encode for Packet<'a> {
    type Error = PacketError;

    fn encode(&self, buffer: &mut [u8]) -> Result<(), Self::Error> {
        if buffer.len() < self.size() {
            return Err(PacketError::EncodeBufferTooSmall {
                expected_at_least: self.size(),
                found: buffer.len(),
            });
        }
        let header_buf = &mut buffer[0..PacketHeader::size()];
        header_buf.copy_from_slice(&self.header.bytes());

        let data_buf =
            &mut buffer[PacketHeader::size()..self.header.length() as usize + PacketHeader::size()];
        data_buf.copy_from_slice(self.data);
        Ok(())
    }
}

impl<'a> Decode<'a> for Packet<'a> {
    type Error = PacketError;

    fn decode(data: &'a [u8]) -> Result<Self, Self::Error> {
        if data.len() < PacketHeader::size() {
            return Err(PacketError::NotEnoughDataForHeader {
                expected: PacketHeader::size(),
                found: data.len(),
            });
        }
        let k = Kind::from(u3::from_u8(data[0]));
        let convo = data[1];
        let l = data[2];

        let header = PacketHeader::new(k, convo, l);
        if data.len() < PacketHeader::size() + header.length() as usize {
            return Err(PacketError::DecodeBufferTooSmall {
                expected_at_least: PacketHeader::size() + header.length() as usize,
                found: data.len(),
            });
        }
        let data = &data[PacketHeader::size()..PacketHeader::size() + header.length() as usize];

        Ok(Packet { header, data })
    }
}

#[derive(Debug)]
pub enum ConnectionError {
    PacketError(PacketError),
    FrameError(FramingError),
    NoAckResponse,
    UnexpectedConvoId { expected: u8, found: u8 },
}

impl From<PacketError> for ConnectionError {
    fn from(value: PacketError) -> Self {
        ConnectionError::PacketError(value)
    }
}

impl From<FramingError> for ConnectionError {
    fn from(value: FramingError) -> Self {
        ConnectionError::FrameError(value)
    }
}

#[derive(Debug)]
pub struct SerialConnection<Tx: Write, Rx: Read> {
    tx: Tx,
    rx: Rx,
}

impl<Tx, Rx> SerialConnection<Tx, Rx>
where
    Tx: Write,
    Rx: Read,
{
    pub fn new(tx: Tx, rx: Rx) -> SerialConnection<Tx, Rx> {
        SerialConnection { tx, rx }
    }

    pub fn send(&mut self, payload: &[u8], id: u8) -> Result<(), ConnectionError> {
        let chunks = payload.len() / MAX_PACKET_DATA_SIZE;
        for (i, payl) in payload.chunks(MAX_PACKET_DATA_SIZE).enumerate() {
            let k = if i == chunks {
                Kind::DataLast
            } else if i == 0 {
                Kind::Data
            } else {
                Kind::DataContinue
            };
            let p = Packet::new(k, id, payl);
            let mut buf = [0; MAX_PACKET_SIZE];
            if let Err(e) = p.encode(&mut buf[0..p.size()]) {
                return Err(ConnectionError::PacketError(e));
            }
            let f = Frame::new(&buf[0..p.size()]);
            let mut buf = [0; MAX_FRAME_SIZE];
            if let Err(e) = f.encode(&mut buf[0..f.size()]) {
                return Err(ConnectionError::FrameError(e));
            }
            let _ = self.tx.write_all(&buf[0..f.size()]);

            // Now listen for Ack on the same id
            let r = self.recv_packet()?;
            match r.header.kind() {
                Kind::Ack => {
                    if r.header.convo_id() != id {
                        return Err(ConnectionError::UnexpectedConvoId {
                            expected: id,
                            found: r.header.convo_id(),
                        });
                    }
                }
                _ => return Err(ConnectionError::NoAckResponse),
            }
            // Once we successfully get an Ack we can continue to the next chunk
        }
        Ok(())
    }

    pub fn recv_packet(&mut self) -> Result<PacketOwned, ConnectionError> {
        let mut v = heapless::Vec::<_, MAX_PACKET_SIZE>::new();
        let mut start = [0; 1];
        // Read until we find the start byte
        loop {
            self.rx.read_exact(&mut start);
            if start[0] == START {
                break;
            }
        }
        v.push(Start.value());
        let mut h = [0; FrameHeader::size()];
        self.rx.read_exact(&mut h);
        let size = h[0];
        v.push(size);
        let mut b = [0; 1];
        // Data length + crc
        for _ in 0..size + 1 {
            self.rx.read_exact(&mut b);
            v.push(b[0]);
        }
        let frame = Frame::decode(&v).map_err(ConnectionError::from)?;
        let packet = Packet::decode(frame.data).map_err(PacketError::from)?;
        Ok(packet.owned())
    }
}
