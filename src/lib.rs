#![no_std]

mod custom;
mod slip;


pub trait Encode {
    type Error;

    fn encode(&self, buffer: &mut [u8]) -> Result<(), Self::Error>;
}

pub trait Decode<'a> where Self: Sized {
    type Error;

    fn decode(data: &'a [u8]) -> Result<Self, Self::Error>;
}

pub use slip::{Frame, FrameHeader, FramingError, Packet, PacketHeader, Kind, PacketError, SerialConnection};

