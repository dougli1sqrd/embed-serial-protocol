extern crate alloc;

use alloc::collections::VecDeque;
use embedded_hal_nb::serial::{Error, ErrorType, Read, Write};

#[derive(Debug)]
pub struct BufferedRx<Rx: Read> {
    pub rx: Rx,
    pub buf: VecDeque<u8>,
}

impl<Rx: Read> BufferedRx<Rx> {
    pub fn new(rx: Rx) -> BufferedRx<Rx> {
        BufferedRx {
            rx,
            buf: VecDeque::new(),
        }
    }

    /// Load as much as we can from rx into the internal buf. Like flush from Write
    pub fn buffer(&mut self) -> nb::Result<(), Rx::Error> {
        loop {
            let c = self.rx.read()?;
            self.buf.push_back(c);
        }
    }

    pub fn peek(&self) -> Option<u8> {
        self.buf.front().copied()
    }

    /// Removes elements from the front of the VecDeque from 0 to the 
    /// specified amount.
    pub fn drain(&mut self, amount: usize) -> alloc::collections::vec_deque::Drain<'_, u8> {
        self.buf.drain(0..amount)
    }

    pub fn front_back_find(&self, c: u8) -> Option<(usize, usize)> {
        // Find from the front
        let sl = self.slice();
        let x = sl.iter().enumerate().find(|(_, x)| **x == c);
        // Find from the back
        if let Some((p, _)) = x {
            let y = sl.iter().rev().enumerate().find(|(_, x)| **x == c);
            if let Some((q, _)) = y {
                let q = sl.len() - 1 - q;
                return if p != q { Some((p, q)) } else { None }
            }
        }
        None
    }

    pub fn slice(&self) -> &[u8] {
        // We only return the "front" queue because we only ever push
        // from the front and pop from the back. So only the front queue
        // Will have elements.
        self.buf.as_slices().0
    }
}

impl<Rx: Read> ErrorType for BufferedRx<Rx> {
    type Error = Rx::Error;
}

impl<Rx: Read> Read for BufferedRx<Rx> {
    fn read(&mut self) -> nb::Result<u8, Self::Error> {
        // Read from self.rx and place into the buffer
        loop {
            match self.rx.read() {
                Ok(x) => {
                    self.buf.push_back(x);
                }
                Err(nb::Error::Other(e)) => return Err(nb::Error::Other(e)),
                Err(nb::Error::WouldBlock) => {
                    break;
                }
            }
        }
        // Pop from the end
        self.buf.pop_front().ok_or(nb::Error::WouldBlock)
    }
}

#[derive(Debug)]
pub struct BufferedTx<Tx: Write> {
    tx: Tx,
    pub buf: VecDeque<u8>,
}

impl<Tx: Write> BufferedTx<Tx> {
    pub fn new(tx: Tx) -> BufferedTx<Tx> {
        BufferedTx {
            tx,
            buf: VecDeque::new(),
        }
    }

    pub fn write_all(&mut self, data: &[u8]) -> nb::Result<(), Tx::Error> {
        for a in data {
            // Unwrapping here won't panic - I know this is weird atm TODO ?
            // self.write will just add to a vec, so guarenteed to work
            self.write(*a).unwrap();
        }
        self.flush()
    }
}

impl<Tx: Write> ErrorType for BufferedTx<Tx> {
    type Error = Tx::Error;
}

#[derive(Debug)]
pub struct ErrorShim<T: Error>(pub T);

impl<T: Error> embedded_io::Error for ErrorShim<T> {
    fn kind(&self) -> embedded_io::ErrorKind {
        use embedded_hal_nb::serial::ErrorKind::*;
        match self.0.kind() {
            Overrun => embedded_io::ErrorKind::OutOfMemory,
            FrameFormat => embedded_io::ErrorKind::InvalidData,
            Noise => embedded_io::ErrorKind::Other,
            Parity => embedded_io::ErrorKind::InvalidData,
            _ => embedded_io::ErrorKind::Other,
        }
    }
}

impl<T: Error> From<T> for ErrorShim<T> {
    fn from(value: T) -> Self {
        ErrorShim(value)
    }
}

impl<Tx: Write> embedded_io::ErrorType for BufferedTx<Tx> {
    type Error = ErrorShim<Tx::Error>;
}

impl<Tx: Write> Write for BufferedTx<Tx> {
    fn write(&mut self, word: u8) -> nb::Result<(), Self::Error> {
        self.buf.push_back(word);
        Ok(())
    }

    fn flush(&mut self) -> nb::Result<(), Self::Error> {
        loop {
            // Pop, and if we're empty then flush WouldBlock
            let x = self.buf.pop_front().ok_or(nb::Error::WouldBlock)?;
            // Attempt to write, and we'll drop out if write WouldBlock or Err
            self.tx.write(x)?;
        }
    }
}

impl<Tx: Write> embedded_io::Write for BufferedTx<Tx> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        match self.write_all(buf) {
            Ok(()) => Ok(buf.len()),
            Err(e) => match e {
                nb::Error::Other(ee) => Err(ErrorShim(ee)),
                nb::Error::WouldBlock => Ok(buf.len()),
            },
        }
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        match Write::flush(self) {
            Ok(k) => Ok(k),
            Err(e) => match e {
                nb::Error::Other(ee) => Err(ErrorShim(ee)),
                nb::Error::WouldBlock => Ok(()),
            },
        }
    }
}
