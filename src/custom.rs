use embedded_io::{Read, Write};
use bilge::prelude::*;

const PREAMBLE: [u8; 2] = [0xAA, 0x55];
const HEADER_LENGTH: usize = 5;
const PREAMBLE_HEADER_LENGTH: usize = HEADER_LENGTH + PREAMBLE.len();
const PAYLOAD_LENGTH: usize = u8::MAX as usize;
const DATAGRAM_LENGTH: usize = PAYLOAD_LENGTH + HEADER_LENGTH + 1;

#[bitsize(3)]
#[derive(FromBits, Debug, Clone, Copy)]
pub enum TransactionState {
    Start = 0,
    Single = 1,
    Continue = 2,
    End = 3,
    #[fallback]
    Reserved,
}

#[bitsize(1)]
#[derive(FromBits, Debug, Clone, Copy)]
enum Kind {
    Init = 0,
    Reply,
}

#[bitsize(8)]
#[derive(DebugBits, Clone, Copy, FromBits)]
struct Info {
    transaction_state: TransactionState,
    kind: Kind,
    _reserved: u4,
}

impl TryFrom<u8> for TransactionState {
    type Error = HeaderError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            x if x == TransactionState::Start as u8 => Ok(TransactionState::Start),
            x if x == TransactionState::Continue as u8 => Ok(TransactionState::Continue),
            x if x == TransactionState::End as u8 => Ok(TransactionState::End),
            // x if x == TransactionState::Reply as u8 => Ok(TransactionState::Reply),
            _ => Err(HeaderError::BadTransactionState)
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct DatagramHeader {
    source: u16,
    length: u8,
    transaction_id: u8,
    info: Info,
}

impl DatagramHeader {
    fn bytes(&self) -> [u8; 5] {
        let mut out = [0; 5];
        let s = self.source.to_be_bytes();
        out[0] = s[0];
        out[1] = s[1];
        out[2] = self.length;
        out[3] = self.transaction_id;
        out[4] = self.info.value;

        out
    }

    fn decode(data: &[u8]) -> Result<DatagramHeader, HeaderError> {
        if data.len() < HEADER_LENGTH {
            return Err(HeaderError::NotEnoughBytesToParseHeader)
        }

        let info = Info::from(data[4]);

        let header = DatagramHeader {
            source: ((data[0] as u16) << 8) + data[1] as u16,
            length: data[2],
            transaction_id: data[3],
            info,
        };
        Ok(header)
    }
}

type Crc = u8;

#[derive(Debug)]
pub enum HeaderError {
    NotEnoughBytesToParseHeader,
    BadTransactionState,
    CrcMismatch{data: [u8; DATAGRAM_LENGTH], found: u8, expected: u8},
    PreambleMismatch,
    TransactionStateBad,
}


fn encode<'a>(source: u16, id: u8, state: TransactionState, data: &'a [u8]) -> (DatagramHeader, &'a [u8], Crc) {
    let crc_algo = crc::Crc::<u8>::new(&crc::CRC_8_MAXIM_DOW);
    let mut d = crc_algo.digest();

    let length = data.len().min(u8::MAX as usize);

    let info = Info::new(state, Kind::Init);

    let header = DatagramHeader {
        source,
        length: length as u8,
        transaction_id: id,
        info,
    };

    let header_bytes = header.bytes();
    d.update(&header_bytes);

    d.update(&data);

    let crc = d.finalize();
    (header, &data[0..length], crc)
}

fn decode(data: &[u8]) -> Result<(DatagramHeader, &[u8]), HeaderError> {
    let crc_algo = crc::Crc::<u8>::new(&crc::CRC_8_MAXIM_DOW);
    let mut d = crc_algo.digest();
    d.update(&data[0..data.len() - 1]);
    let crc = d.finalize();
    if *data.last().unwrap() != crc {
        return Err(HeaderError::CrcMismatch{ data: [0; DATAGRAM_LENGTH], found: *data.last().unwrap(), expected: crc });
    }

    let header = DatagramHeader::decode(&data[0..HEADER_LENGTH])?;
    Ok((header, &data[HEADER_LENGTH..data.len() - 1]))
}

pub fn read_transaction<R: Read, Fold, T>(rx: &mut R, init: T, fold: Fold) -> Result<T, HeaderError> where Fold: Fn(T, &[u8]) -> T {

    let crc_algo = crc::Crc::<u8>::new(&crc::CRC_8_MAXIM_DOW);
    let mut d = crc_algo.digest();
    let mut accum = init;

    let mut preamble_buffer = [0; PREAMBLE.len()];
    rx.read_exact(&mut preamble_buffer);
    if &preamble_buffer != &PREAMBLE {
        return Err(HeaderError::PreambleMismatch)
    }

    let mut header_buffer = [0; HEADER_LENGTH];
    rx.read_exact(&mut header_buffer);

    // decode header and update crc
    d.update(&header_buffer);
    let header = DatagramHeader::decode(&header_buffer)?;
    let mut transaction_state = match header.info.transaction_state() {
        TransactionState::Start => TransactionState::Start,
        TransactionState::Single => TransactionState::Single,
        _ => {
            return Err(HeaderError::BadTransactionState)
        }
    };

    // Load data for the gram
    let mut data_buffer = [0; PAYLOAD_LENGTH];
    // Slice only over the length given in the header
    let mut data_buffer = &mut data_buffer[0..header.length as usize];
    rx.read_exact(&mut data_buffer);
    d.update(&data_buffer);
    let crc = d.finalize();

    // Load crc byte and compare with computed crc
    let mut crc_buf = [0; 1];
    rx.read_exact(&mut crc_buf);
    if crc != crc_buf[0] {
        let mut edata = heapless::Vec::<u8, DATAGRAM_LENGTH>::new();
        edata.extend_from_slice(&header_buffer);
        edata.extend_from_slice(&data_buffer);
        edata.extend_from_slice(&crc_buf);
        let mut a = [0; DATAGRAM_LENGTH];
        a[0..edata.len()].copy_from_slice(edata.as_slice());
        return Err(HeaderError::CrcMismatch { data: a, found: crc_buf[0], expected: crc});
    }

    accum = fold(accum, &data_buffer);

    match transaction_state {
        TransactionState::Single => {
            return Ok(accum)
        },
        _ => {}
    }

    loop {
        let mut d = crc_algo.digest();

        let mut preamble_buffer = [0; PREAMBLE.len()];
        rx.read_exact(&mut preamble_buffer);
        if &preamble_buffer != &PREAMBLE {
            return Err(HeaderError::PreambleMismatch)
        }

        let mut header_buffer = [0; HEADER_LENGTH];
        rx.read_exact(&mut header_buffer);

        // decode header and update crc
        d.update(&header_buffer);
        let header = DatagramHeader::decode(&header_buffer)?;
        transaction_state = match header.info.transaction_state() {
            TransactionState::Continue => TransactionState::Continue,
            TransactionState::End => TransactionState::End,
            _ => {
                return Err(HeaderError::BadTransactionState)
            }
        };

        // Load data for the gram
        let mut data_buffer = [0; PAYLOAD_LENGTH];
        // Slice only over the length given in the header
        let mut data_buffer = &mut data_buffer[0..header.length as usize];
        rx.read_exact(&mut data_buffer);
        d.update(&data_buffer);
        let crc = d.finalize();

        // Load crc byte and compare with computed crc
        let mut crc_buf = [0; 1];
        rx.read_exact(&mut crc_buf);
        if crc != crc_buf[0] {
            let mut edata = heapless::Vec::<u8, DATAGRAM_LENGTH>::new();
        edata.extend_from_slice(&header_buffer);
        edata.extend_from_slice(&data_buffer);
        edata.extend_from_slice(&crc_buf);
        let mut a = [0; DATAGRAM_LENGTH];
        a[0..edata.len()].copy_from_slice(edata.as_slice());
            return Err(HeaderError::CrcMismatch { data: a, found: crc_buf[0], expected: crc});
        }

        accum = fold(accum, &data_buffer);

        match transaction_state {
            TransactionState::End => {
                return Ok(accum)
            },
            _ => {}
        }
    }

}

pub fn write_transaction<'a, T: Write>(source: u16, id: u8, data: &'a [u8], tx: &mut T) {
    let crc_algo = crc::Crc::<u8>::new(&crc::CRC_8_MAXIM_DOW);

    let mut buffer = heapless::Vec::<u8, DATAGRAM_LENGTH>::new();
    let start_state = if data.len() > PAYLOAD_LENGTH {
        TransactionState::Start
    } else {
        TransactionState::Single
    };

    for (i, chunk) in data.chunks(PAYLOAD_LENGTH).enumerate() {
        buffer.clear();
        let state = match i {
            0 => start_state,
            _ if i == data.len() / PAYLOAD_LENGTH => TransactionState::End,
            _ => TransactionState::Continue,
        };
        let length = chunk.len();
        let header = DatagramHeader {
            source,
            length: length as u8,
            transaction_id: id,
            // TODO deal with Kind
            info: Info::new(state, Kind::Init),
        };
        let hb = header.bytes();
        let mut d = crc_algo.digest_with_initial(0);
        d.update(&hb);
        d.update(chunk);
        let crc = d.finalize();

        let _ = buffer.extend_from_slice(&hb);
        let _ = buffer.extend_from_slice(chunk);
        let _ = buffer.push(crc);
        
        let _ = tx.write(&PREAMBLE);
        let _ = tx.write(&buffer);
    }
}



pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use core::convert::Infallible;

    use embedded_io::ErrorType;

    use super::*;

    extern crate alloc;
    

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }

    // #[test]
    // fn blah() {
    //     let mut test_write = TxBuffer::new();
    //     let data = [4, 3, 2, 1];
    //     write_transaction(0x4224, 0x01, TransactionState::Single, &data, &mut test_write);
    //     assert_eq!(&[0], &test_write.0.as_slice());
    // }
}
