use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures::{channel::mpsc, SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use std::{fmt::Write, io, time::Duration};
use tarpc::{ClientMessage, Response};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};

use crate::{Error, ServiceRequest, ServiceResponse};

/// How long to wait for the RPC server to start
const CONNECT_TIMEOUT: Duration = Duration::from_secs(120);
const FRAME_TYPE_SIZE: usize = std::mem::size_of::<FrameType>();
const DAEMON_CHANNEL_BUF_SIZE: usize = 16 * 1024;

pub enum Frame {
    Handshake,
    TestRunner(Bytes),
    DaemonRpc(Bytes),
}

#[repr(u8)]
enum FrameType {
    Handshake,
    TestRunner,
    DaemonRpc,
}

impl TryFrom<u8> for FrameType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            i if i == FrameType::Handshake as u8 => Ok(FrameType::Handshake),
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

    let (handshake_tx, handshake_rx) = mpsc::unbounded();

    let _ = handshake_tx.unbounded_send(());

    let completion_handle = tokio::spawn(async move {
        if let Err(error) = forward_messages(
            serial_stream,
            runner_forwarder_2,
            mullvad_daemon_forwarder,
            (handshake_tx, handshake_rx),
            None,
        )
        .await
        {
            log::error!(
                "forward_messages stopped due an error: {}",
                display_chain(error)
            );
        } else {
            log::trace!("forward_messages stopped");
        }
    });

    (runner_forwarder_1, daemon_rx, completion_handle)
}

pub async fn create_client_transports(
    serial_stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
) -> Result<
    (
        tarpc::transport::channel::UnboundedChannel<
            Response<ServiceResponse>,
            ClientMessage<ServiceRequest>,
        >,
        GrpcForwarder,
        CompletionHandle,
    ),
    Error,
> {
    let (runner_forwarder_1, runner_forwarder_2) = tarpc::transport::channel::unbounded();

    let (daemon_rx, mullvad_daemon_forwarder) = tokio::io::duplex(DAEMON_CHANNEL_BUF_SIZE);

    let (handshake_tx, handshake_rx) = mpsc::unbounded();
    let (handshake_fwd_tx, mut handshake_fwd_rx) = mpsc::unbounded();

    let _ = handshake_tx.unbounded_send(());

    let completion_handle = tokio::spawn(async move {
        if let Err(error) = forward_messages(
            serial_stream,
            runner_forwarder_1,
            mullvad_daemon_forwarder,
            (handshake_tx, handshake_rx),
            Some(handshake_fwd_tx),
        )
        .await
        {
            log::error!(
                "forward_messages stopped due an error: {}",
                display_chain(error)
            );
        } else {
            log::trace!("forward_messages stopped");
        }
    });

    log::info!("Waiting for server");

    match tokio::time::timeout(CONNECT_TIMEOUT, handshake_fwd_rx.next()).await {
        Ok(_) => log::info!("Server responded"),
        _ => {
            log::error!("Connection timed out");
            return Err(Error::TestRunnerTimeout);
        }
    }

    Ok((runner_forwarder_2, daemon_rx, completion_handle))
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

    #[error(display = "Handshake error")]
    HandshakeError(#[error(source)] io::Error),
}

async fn forward_messages<
    T: Serialize + Unpin + Send + 'static,
    S: DeserializeOwned + Unpin + Send + 'static,
>(
    serial_stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
    mut runner_forwarder: tarpc::transport::channel::UnboundedChannel<T, S>,
    mullvad_daemon_forwarder: GrpcForwarder,
    mut handshaker: (mpsc::UnboundedSender<()>, mpsc::UnboundedReceiver<()>),
    handshake_fwd: Option<mpsc::UnboundedSender<()>>,
) -> Result<(), ForwardError> {
    let codec = MultiplexCodec::default();
    let mut serial_stream = codec.framed(serial_stream);

    // Needs to be framed to allow empty messages.
    let mut mullvad_daemon_forwarder = LengthDelimitedCodec::new().framed(mullvad_daemon_forwarder);

    loop {
        match futures::future::select(
            futures::future::select(serial_stream.next(), handshaker.1.next()),
            futures::future::select(runner_forwarder.next(), mullvad_daemon_forwarder.next()),
        )
        .await
        {
            futures::future::Either::Left((futures::future::Either::Left((Some(frame), _)), _)) => {
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
                            .map_err(ForwardError::DaemonChannel)?;
                    }
                    Frame::Handshake => {
                        log::trace!("shake: recv");
                        if let Some(shake_fwd) = handshake_fwd.as_ref() {
                            let _ = shake_fwd.unbounded_send(());
                        } else {
                            let _ = handshaker.0.unbounded_send(());
                        }
                    }
                }
            }
            futures::future::Either::Left((futures::future::Either::Right((Some(()), _)), _)) => {
                log::trace!("shake: send");

                // Ping the other end
                serial_stream
                    .send(Frame::Handshake)
                    .await
                    .map_err(ForwardError::HandshakeError)?;
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
            futures::future::Either::Right((futures::future::Either::Right((None, _)), _)) => {
                //
                // Force management interface socket to close
                //
                let _ = serial_stream.send(Frame::DaemonRpc(Bytes::new())).await;

                break Ok(());
            }
            _ => {
                break Ok(());
            }
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct MultiplexCodec {
    len_delim_codec: LengthDelimitedCodec,
    has_connected: bool,
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
            FrameType::Handshake => Ok(Frame::Handshake),
            FrameType::TestRunner => Ok(Frame::TestRunner(frame.into())),
            FrameType::DaemonRpc => Ok(Frame::DaemonRpc(frame.into())),
        }
    }

    fn encode_frame(
        &mut self,
        frame_type: FrameType,
        bytes: Option<Bytes>,
        dst: &mut BytesMut,
    ) -> Result<(), io::Error> {
        let mut buffer = BytesMut::new();
        if let Some(bytes) = bytes {
            buffer.reserve(bytes.len() + FRAME_TYPE_SIZE);
            buffer.put_u8(frame_type as u8);
            // TODO: implement without copying
            buffer.put(&bytes[..]);
        } else {
            buffer.reserve(FRAME_TYPE_SIZE);
            buffer.put_u8(frame_type as u8);
        }
        self.len_delim_codec.encode(buffer.into(), dst)
    }

    fn decode_inner(&mut self, src: &mut BytesMut) -> Result<Option<Frame>, io::Error> {
        self.skip_control_chars(src);
        let frame = self.len_delim_codec.decode(src)?;
        frame.map(Self::decode_frame).transpose()
    }

    fn skip_control_chars(&mut self, src: &mut BytesMut) {
        // The test runner likes to send ^@ once in while. Unclear why,
        // but it probably occurs (sometimes) when it reconnects to the
        // serial device. Ignoring these control characters is safe.

        // When using OVMF, the serial port is used for console output.
        // \r\n is sent before we take over the COM port.

        while src.len() >= 2 {
            if src[0] == b'^' {
                log::debug!("ignoring control character");
                src.advance(2);
                continue;
            }
            if !self.has_connected {
                for (pos, c) in src.iter().rev().enumerate() {
                    if *c == b'\n' {
                        log::debug!("ignoring newlines");
                        src.advance(src.len() - pos);
                        break;
                    }
                }
            }
            break;
        }
    }
}

impl Decoder for MultiplexCodec {
    type Item = Frame;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let result = self.decode_inner(src);
        match &result {
            Ok(Some(_)) => self.has_connected = true,
            Ok(None) => (),
            Err(_error) => {
                if !self.has_connected {
                    // If the serial port is used for console output before the
                    // OS is running, we need to ignore that data.
                    log::trace!("ignoring unrecognized data: {:?}", src);
                    src.clear();
                    return Ok(None);
                }
            }
        }
        result
    }
}

impl Encoder<Frame> for MultiplexCodec {
    type Error = io::Error;

    fn encode(&mut self, frame: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        match frame {
            Frame::Handshake => self.encode_frame(FrameType::Handshake, None, dst),
            Frame::TestRunner(bytes) => self.encode_frame(FrameType::TestRunner, Some(bytes), dst),
            Frame::DaemonRpc(bytes) => self.encode_frame(FrameType::DaemonRpc, Some(bytes), dst),
        }
    }
}

fn display_chain(error: impl std::error::Error) -> String {
    let mut s = error.to_string();
    let mut error = &error as &dyn std::error::Error;
    while let Some(source) = error.source() {
        write!(&mut s, "\nCaused by: {}", source).unwrap();
        error = source;
    }
    s
}
