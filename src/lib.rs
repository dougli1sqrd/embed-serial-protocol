#![no_std]

pub mod packet;
pub mod serial;

extern crate alloc;

pub use embedded_io as io;

pub trait Encode {
    type Error;

    fn encode(&self, buffer: &mut [u8]) -> Result<usize, Self::Error>;
}

pub trait Decode<'a>
where
    Self: Sized,
{
    type Error;

    fn decode(data: &'a [u8]) -> Result<Self, Self::Error>;
}

pub use packet::{
    DELIMITER, Frame, FrameDataSlice, FrameError, FrameIOError, FrameTxRx, MAX_DATA_SIZE,
    MAX_FRAME_SIZE,
};
pub use serial::{BufferedRx, BufferedTx, ErrorShim};
