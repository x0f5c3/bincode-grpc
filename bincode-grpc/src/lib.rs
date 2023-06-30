extern crate self as bincode_grpc;
pub extern crate tonic;
pub extern crate tracing;

#[derive(Copy, Clone)]
struct BinCodec<T: Encode, D: Decode> {
    enc: BinCoder<T>,
    dec: BinDec<D>,
}

impl<T: Encode, D: Decode> BinCodec<T, D> {
    pub fn enc(&self) -> &BinCoder<T> {
        &self.enc
    }
    pub fn dec(&self) -> &BinDec<D> {
        &self.dec
    }
}

impl<T: Encode + Send + 'static + Clone, D: Decode + Send + 'static + Clone> Codec
    for BinCodec<T, D>
{
    type Encode = T;
    type Decode = D;
    type Encoder = BinCoder<T>;
    type Decoder = BinDec<D>;

    fn encoder(&mut self) -> Self::Encoder {
        self.enc().clone()
    }

    fn decoder(&mut self) -> Self::Decoder {
        self.dec().clone()
    }
}

#[derive(Clone, Copy)]
struct BinCoder<T: Encode> {
    _t: PhantomData<T>,
}

struct ReadAdapter<T: Buf> {
    inner: bytes::buf::Reader<T>,
}

impl<T: Buf> bincode::de::read::Reader for ReadAdapter<T> {
    fn read(&mut self, bytes: &mut [u8]) -> Result<(), DecodeError> {
        let req_n = bytes.len();
        let n = self
            .inner
            .read(bytes)
            .map_err(|e| DecodeError::OtherString(e.to_string()))?;
        if req_n != n {
            return Err(DecodeError::ArrayLengthMismatch {
                found: n,
                required: req_n,
            });
        }
        Ok(())
    }
}

impl<T: Buf> ReadAdapter<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner: inner.reader(),
        }
    }
}

impl<T: Buf> AsRef<T> for ReadAdapter<T> {
    fn as_ref(&self) -> &T {
        self.inner.get_ref()
    }
}

// impl<T: Buf> Into<T> for ReadAdapter<T> {
//     fn into(self) -> T {
//         self.inner.into_inner()
//     }
// }

struct WriteAdapter<T: Buf> {
    inner: bytes::buf::Writer<T>,
}

impl<T: Buf + BufMut> bincode::enc::write::Writer for WriteAdapter<T> {
    fn write(&mut self, bytes: &[u8]) -> Result<(), EncodeError> {
        let req_len = bytes.len();
        let n = self
            .inner
            .write(bytes)
            .map_err(|e| EncodeError::OtherString(e.to_string()))?;
        if req_len != n {
            return Err(EncodeError::Other("Wanted {req_len} got {n}"));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EncoderError {
    #[error(transparent)]
    EncodeError(#[from] bincode::error::EncodeError),
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    DecodeError(#[from] bincode::error::DecodeError),
    #[error("Status: {0}")]
    Status(#[from] tonic::Status),
}

impl EncoderError {
    fn into_status(self) -> Status {
        Status::internal(self.to_string())
    }
}

impl<T: Encode> Encoder for BinCoder<T> {
    type Item = T;
    type Error = Status;
    fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        let mut buf = Vec::new();
        bincode::encode_into_slice(item, &mut buf, bincode::config::standard())
            .map_err(|e| EncoderError::from(e))
            .map_err(|e| e.into_status())?;
        dst.put(buf.as_slice());
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct BinDec<D: Decode> {
    _d: PhantomData<D>,
}

impl<D: Decode> Decoder for BinDec<D> {
    type Item = D;
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Self::Error> {
        bincode::decode_from_reader(ReadAdapter::new(src), bincode::config::standard())
            .map_err(EncoderError::from)
            .map_err(|e| e.into_status())
    }
}
//
// pub mod bi_codec {
//     use bincode::{Decode, Encode};
//     use grpcio::MessageReader;
//     use grpcio::Result;
//     use serde::de::DeserializeOwned;
//     use std::io::{BufReader, Read};
//     use std::time::Instant;
//     use tonic::codec::{Codec, DecodeBuf, Decoder, Encoder};
//     pub fn ser<M: Encode>(msg: &M, buf: &mut Vec<u8>) {
//         let span = tracing::span!(tracing::Level::DEBUG, "serialize");
//         let _guard = span.enter();
//         let start_time = Instant::now();
//         let serialized = bincode::serialize(msg).expect("serialize message failed");
//         assert_eq!(std::mem::replace(buf, serialized).len(), 0);
//         tracing::debug!(
//             "serialize {:?} time cost {:?} on thread {:?}",
//             std::any::type_name_of_val(&msg),
//             start_time.elapsed(),
//             std::thread::current().id()
//         );
//     }
//
//     pub fn de<M: Decode>(mut reader: MessageReader) -> Result<M> {
//         let span = tracing::span!(tracing::Level::DEBUG, "deserialize");
//         let _guard = span.enter();
//         let start_time = Instant::now();
//         let mut buf = Vec::with_capacity(reader.len());
//         reader
//             .read_to_end(&mut buf)
//             .expect("Reading message from buffer failed");
//         let result =
//             bincode::deserialize(&mut buf).expect("Deserializing message from buffer failed");
//         tracing::debug!(
//             "deserialize {:?} time cost {:?} on thread {:?}",
//             std::any::type_name_of_val(&result),
//             start_time.elapsed(),
//             std::thread::current().id()
//         );
//         Ok(result)
//     }
// }

use bincode::error::{DecodeError, EncodeError};
pub use bincode::{Decode, Encode};
use bytes::{Buf, BufMut};
use std::error::Error;
use std::io::{Read, Write};
use std::marker::PhantomData;
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};
use tonic::Status;
