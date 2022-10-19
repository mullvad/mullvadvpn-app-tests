use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures::{SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use std::io;
use tarpc::{ClientMessage, Response};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};

use crate::{ServiceRequest, ServiceResponse};

const FRAME_TYPE_SIZE: usize = std::mem::size_of::<FrameType>();
const DAEMON_CHANNEL_BUF_SIZE: usize = 16 * 1024;

pub enum Frame {
    TestRunner(Bytes),
    DaemonRpc(Bytes),
}

#[repr(u8)]
enum FrameType {
    TestRunner,
    DaemonRpc,
}

impl TryFrom<u8> for FrameType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            i if i == FrameType::TestRunner as u8 => Ok(FrameType::TestRunner),
            i if i == FrameType::DaemonRpc as u8 => Ok(FrameType::DaemonRpc),
            _ => Err(()),
        }
    }
}

pub type GrpcForwarder = tokio::io::DuplexStream;
pub type CompletionHandle = tokio::task::JoinHandle<()>;

pub fn create_server_transports(
    serial_stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
) -> (
    tarpc::transport::channel::UnboundedChannel<
        ClientMessage<ServiceRequest>,
        Response<ServiceResponse>,
    >,
    GrpcForwarder,
    CompletionHandle,
) {
    let (runner_forwarder_1, runner_forwarder_2) = tarpc::transport::channel::unbounded();

    let (daemon_rx, mullvad_daemon_forwarder) = tokio::io::duplex(DAEMON_CHANNEL_BUF_SIZE);

    let completion_handle = tokio::spawn(async move {
        if let Err(error) =
            forward_messages(serial_stream, runner_forwarder_2, mullvad_daemon_forwarder).await
        {
            log::error!("forward_messages stopped due an error: {}", error);
        } else {
            log::trace!("forward_messages stopped");
        }
    });

    (runner_forwarder_1, daemon_rx, completion_handle)
}

pub fn create_client_transports(
    serial_stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
) -> (
    tarpc::transport::channel::UnboundedChannel<
        Response<ServiceResponse>,
        ClientMessage<ServiceRequest>,
    >,
    GrpcForwarder,
    CompletionHandle,
) {
    let (runner_forwarder_1, runner_forwarder_2) = tarpc::transport::channel::unbounded();

    let (daemon_rx, mullvad_daemon_forwarder) = tokio::io::duplex(DAEMON_CHANNEL_BUF_SIZE);

    let completion_handle = tokio::spawn(async move {
        if let Err(error) =
            forward_messages(serial_stream, runner_forwarder_1, mullvad_daemon_forwarder).await
        {
            log::error!("forward_messages stopped due an error: {}", error);
        } else {
            log::trace!("forward_messages stopped");
        }
    });
    (runner_forwarder_2, daemon_rx, completion_handle)
}

#[derive(err_derive::Error, Debug)]
#[error(no_from)]
enum ForwardError {
    #[error(display = "Failed to deserialize JSON data")]
    DeserializeFailed(#[error(source)] serde_json::Error),

    #[error(display = "Failed to serialize JSON data")]
    SerializeFailed(#[error(source)] serde_json::Error),

    #[error(display = "Serial connection error")]
    SerialConnection(#[error(source)] io::Error),

    #[error(display = "Test runner channel error")]
    TestRunnerChannel(#[error(source)] tarpc::transport::channel::ChannelError),

    #[error(display = "Daemon channel error")]
    DaemonChannel(#[error(source)] io::Error),
}

async fn forward_messages<
    T: Serialize + Unpin + Send + 'static,
    S: DeserializeOwned + Unpin + Send + 'static,
>(
    serial_stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
    mut runner_forwarder: tarpc::transport::channel::UnboundedChannel<T, S>,
    mullvad_daemon_forwarder: GrpcForwarder,
) -> Result<(), ForwardError> {
    let codec = MultiplexCodec::default();
    let mut serial_stream = codec.framed(serial_stream);

    // Needs to be framed to allow empty messages.
    let mut mullvad_daemon_forwarder = LengthDelimitedCodec::new().framed(mullvad_daemon_forwarder);

    loop {
        match futures::future::select(
            serial_stream.next(),
            futures::future::select(runner_forwarder.next(), mullvad_daemon_forwarder.next()),
        )
        .await
        {
            futures::future::Either::Left((Some(frame), _)) => {
                let frame = frame.map_err(ForwardError::SerialConnection)?;

                //
                // Deserialize frame and send it to one of the channels
                //

                match frame {
                    Frame::TestRunner(data) => {
                        let message = serde_json::from_slice(&data)
                            .map_err(ForwardError::DeserializeFailed)?;
                        runner_forwarder
                            .send(message)
                            .await
                            .map_err(ForwardError::TestRunnerChannel)?;
                    }
                    Frame::DaemonRpc(data) => {
                        mullvad_daemon_forwarder
                            .send(data)
                            .await
                            .map_err(|error| ForwardError::DaemonChannel(error))?;
                    }
                }
            }
            futures::future::Either::Right((
                futures::future::Either::Left((Some(message), _)),
                _,
            )) => {
                let message = message.map_err(ForwardError::TestRunnerChannel)?;

                //
                // Serialize messages from tarpc channel into frames
                // and send them over the serial connection
                //

                let serialized =
                    serde_json::to_vec(&message).map_err(ForwardError::SerializeFailed)?;
                serial_stream
                    .send(Frame::TestRunner(serialized.into()))
                    .await
                    .map_err(ForwardError::SerialConnection)?;
            }
            futures::future::Either::Right((
                futures::future::Either::Right((Some(data), _)),
                _,
            )) => {
                let data = data.map_err(ForwardError::DaemonChannel)?;

                //
                // Forward whatever the heck this is
                //

                serial_stream
                    .send(Frame::DaemonRpc(data.into()))
                    .await
                    .map_err(ForwardError::SerialConnection)?;
            }
            futures::future::Either::Left((None, _))
            | futures::future::Either::Right((futures::future::Either::Left((None, _)), _)) => {
                break Ok(());
            }
            futures::future::Either::Right((futures::future::Either::Right((None, _)), _)) => {
                //
                // Force management interface socket to close
                //
                let _ = serial_stream.send(Frame::DaemonRpc(Bytes::new())).await;

                break Ok(());
            }
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct MultiplexCodec {
    len_delim_codec: LengthDelimitedCodec,
}

impl MultiplexCodec {
    fn decode_frame(mut frame: BytesMut) -> Result<Frame, io::Error> {
        if frame.len() < FRAME_TYPE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "frame does not contain frame type",
            ));
        }

        let mut type_bytes = frame.split_to(FRAME_TYPE_SIZE);
        let frame_type = FrameType::try_from(type_bytes.get_u8())
            .map_err(|_err| io::Error::new(io::ErrorKind::InvalidInput, "invalid frame type"))?;

        match frame_type {
            FrameType::TestRunner => Ok(Frame::TestRunner(frame.into())),
            FrameType::DaemonRpc => Ok(Frame::DaemonRpc(frame.into())),
        }
    }
}

impl Decoder for MultiplexCodec {
    type Item = Frame;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let frame = self.len_delim_codec.decode(src)?;
        frame.map(Self::decode_frame).transpose()
    }
}

impl Encoder<Frame> for MultiplexCodec {
    type Error = io::Error;

    fn encode(&mut self, frame: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let (bytes, frame_type) = match frame {
            Frame::TestRunner(bytes) => (bytes, FrameType::TestRunner as u8),
            Frame::DaemonRpc(bytes) => (bytes, FrameType::DaemonRpc as u8),
        };
        // TODO: implement without copying
        let mut buffer = BytesMut::new();
        buffer.reserve(bytes.len() + FRAME_TYPE_SIZE);
        buffer.put_u8(frame_type);
        buffer.put(&bytes[..]);
        self.len_delim_codec.encode(buffer.into(), dst)
    }
}
