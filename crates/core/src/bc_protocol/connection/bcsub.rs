use super::BcConnection;
use crate::bcmedia::codex::BcMediaCodex;
use crate::{bc::model::*, Error, Result};
use futures::stream::{IntoAsyncRead, Stream, StreamExt, TryStreamExt};
use std::io::Result as IoResult;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc::Receiver;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::codec::FramedRead;
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt};

pub struct BcSubscription<'a> {
    rx: ReceiverStream<Bc>,
    msg_num: u16,
    conn: &'a BcConnection,
}

pub struct BcStream<'a> {
    rx: &'a mut ReceiverStream<Bc>,
}

impl<'a> Unpin for BcStream<'a> {}

impl<'a> Stream for BcStream<'a> {
    type Item = Bc;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Bc>> {
        let mut this = self.as_mut();
        match Pin::new(&mut this.rx).poll_next(cx) {
            Poll::Ready(Some(bc)) => Poll::Ready(Some(bc)),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct BcPayloadStream<'a> {
    rx: &'a mut ReceiverStream<Bc>,
}

impl<'a> Unpin for BcPayloadStream<'a> {}

impl<'a> Stream for BcPayloadStream<'a> {
    type Item = IoResult<Vec<u8>>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<IoResult<Vec<u8>>>> {
        match Pin::new(&mut self.rx).poll_next(cx) {
            Poll::Ready(Some(bc)) => {
                match bc {
                    Bc {
                        body:
                            BcBody::ModernMsg(ModernMsg {
                                payload: Some(BcPayloads::Binary(data)),
                                ..
                            }),
                        ..
                    } => Poll::Ready(Some(Ok(data))),
                    _ => {
                        cx.waker().wake_by_ref(); // Make it wake in next frame for another attempt at rx.poll_next(cx)
                        Poll::Pending
                    }
                }
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub type BcMediaStream<'b> = FramedRead<Compat<IntoAsyncRead<BcPayloadStream<'b>>>, BcMediaCodex>;

impl<'a> BcSubscription<'a> {
    pub fn new(rx: Receiver<Bc>, msg_num: u16, conn: &'a BcConnection) -> BcSubscription<'a> {
        BcSubscription {
            rx: ReceiverStream::new(rx),
            msg_num,
            conn,
        }
    }

    pub async fn send(&self, bc: Bc) -> Result<()> {
        assert!(bc.meta.msg_num == self.msg_num);
        self.conn.send(bc).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<Bc> {
        let bc = self.rx.next().await.ok_or(Error::DroppedSubscriber)?;
        assert!(bc.meta.msg_num == self.msg_num);
        Ok(bc)
    }

    #[allow(unused)]
    pub fn bc_stream(&'_ mut self) -> BcStream<'_> {
        BcStream { rx: &mut self.rx }
    }

    #[allow(unused)]
    pub fn payload_stream(&'_ mut self) -> BcPayloadStream<'_> {
        BcPayloadStream { rx: &mut self.rx }
    }

    #[allow(unused)]
    pub fn bcmedia_stream(&'_ mut self) -> BcMediaStream<'_> {
        let async_read = BcPayloadStream { rx: &mut self.rx }
            .into_async_read()
            .compat();
        FramedRead::new(async_read, BcMediaCodex::new())
    }
}

/// Makes it difficult to avoid unsubscribing when you're finished
impl<'a> Drop for BcSubscription<'a> {
    fn drop(&mut self) {
        // It's fine if we can't unsubscribe as that means we already have
        let _ = self.conn.unsubscribe(self.msg_num);
    }
}
