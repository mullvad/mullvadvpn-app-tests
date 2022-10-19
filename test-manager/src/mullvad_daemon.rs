use futures::{pin_mut, SinkExt, StreamExt};
use mullvad_management_interface::{
    types::management_service_client::ManagementServiceClient, Channel,
};
use test_rpc::transport::GrpcForwarder;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::codec::{Decoder, LengthDelimitedCodec};
use tower::Service;

struct DummyService(Option<GrpcForwarder>);

impl<Request> Service<Request> for DummyService {
    type Response = GrpcForwarder;
    type Error = std::io::Error;
    type Future = futures::future::Ready<Result<GrpcForwarder, Self::Error>>;

    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        if self.0.is_some() {
            std::task::Poll::Ready(Ok(()))
        } else {
            std::task::Poll::Pending
        }
    }

    fn call(&mut self, _: Request) -> Self::Future {
        let stream = self.0.take().expect("called more than once");
        futures::future::ok(stream)
    }
}

pub async fn new_rpc_client(
    mullvad_daemon_transport: GrpcForwarder,
) -> ManagementServiceClient<Channel> {
    const CONVERTER_BUF_SIZE: usize = 16 * 1024;

    let mut framed_transport = LengthDelimitedCodec::new().framed(mullvad_daemon_transport);

    let (mut management_channel_in, management_channel_out) = tokio::io::duplex(CONVERTER_BUF_SIZE);
    tokio::spawn(async move {
        let mut read_buf = [0u8; CONVERTER_BUF_SIZE];
        loop {
            let proxy_read = management_channel_in.read(&mut read_buf);
            pin_mut!(proxy_read);

            match futures::future::select(framed_transport.next(), proxy_read).await {
                futures::future::Either::Left((Some(Ok(bytes)), _)) => {
                    if management_channel_in.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
                futures::future::Either::Right((Ok(num_bytes), _)) => {
                    if framed_transport
                        .send(read_buf[..num_bytes].to_vec().into())
                        .await
                        .is_err()
                    {
                        break;
                    }
                    if num_bytes == 0 {
                        log::trace!("Mullvad daemon connection EOF");
                        break;
                    }
                }
                futures::future::Either::Right((Err(_), _)) => {
                    let _ = framed_transport.send(bytes::Bytes::new()).await;
                    break;
                }
                _ => break,
            }
        }
    });

    log::debug!("Mullvad daemon: connecting");
    let channel = tonic::transport::Endpoint::from_static("serial://placeholder")
        .connect_with_connector(DummyService(Some(management_channel_out)))
        .await
        .unwrap();
    log::debug!("Mullvad daemon: connected");

    ManagementServiceClient::new(channel)
}
