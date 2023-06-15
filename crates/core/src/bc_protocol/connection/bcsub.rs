use super::BcConnection;
use crate::bcmedia::codex::BcMediaCodex;
use crate::{bc::model::*, Error, Result};
use futures::stream::{IntoAsyncRead, Stream, StreamExt, TryStreamExt};
use std::io::{Error as IoError, ErrorKind, Result as IoResult};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc::Receiver;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::codec::FramedRead;
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt};

pub struct BcSubscription<'a> {
    rx: ReceiverStream<Result<Bc>>,
    msg_num: Option<u32>,
    conn: &'a BcConnection,
}

pub struct BcStream<'a> {
    rx: &'a mut ReceiverStream<Result<Bc>>,
}

impl<'a> Unpin for BcStream<'a> {}

impl<'a> Stream for BcStream<'a> {
    type Item = Result<Bc>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Bc>>> {
        let mut this = self.as_mut();
        match Pin::new(&mut this.rx).poll_next(cx) {
            Poll::Ready(Some(bc)) => Poll::Ready(Some(bc)),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct BcPayloadStream<'a> {
    rx: &'a mut ReceiverStream<Result<Bc>>,
}

impl<'a> Unpin for BcPayloadStream<'a> {}

impl<'a> Stream for BcPayloadStream<'a> {
    type Item = IoResult<Vec<u8>>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<IoResult<Vec<u8>>>> {
        // log::debug!("PayloadStream: Poll");
        match Pin::new(&mut self.rx).poll_next(cx) {
            Poll::Ready(Some(Ok(Bc {
                body:
                    BcBody::ModernMsg(ModernMsg {
                        payload: Some(BcPayloads::Binary(data)),
                        ..
                    }),
                ..
            }))) => {
                // log::debug!("PayloadStream: Data");
                return Poll::Ready(Some(Ok(data)));
            }
            Poll::Ready(Some(Ok(_bc))) => {
                // trace!("Got other BC in payload stream");
                // log::debug!("PayloadStream: Other Data!");
            }
            Poll::Ready(Some(Err(e))) => {
                // log::debug!("PayloadStream: Err");
                return Poll::Ready(Some(Err(IoError::new(ErrorKind::Other, e))));
            }
            Poll::Ready(None) => {
                // log::debug!("PayloadStream: None");
                return Poll::Ready(None);
            }
            Poll::Pending => {
                // log::debug!("PayloadStream: Pend");
                return Poll::Pending;
            }
        }
        // log::debug!("PayloadStream: Default Pend and Wake");
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

pub type BcMediaStream<'b> = FramedRead<Compat<IntoAsyncRead<BcPayloadStream<'b>>>, BcMediaCodex>;

impl<'a> BcSubscription<'a> {
    pub fn new(
        rx: Receiver<Result<Bc>>,
        msg_num: Option<u32>,
        conn: &'a BcConnection,
    ) -> BcSubscription<'a> {
        BcSubscription {
            rx: ReceiverStream::new(rx),
            msg_num,
            conn,
        }
    }

    pub async fn send(&self, bc: Bc) -> Result<()> {
        if let Some(msg_num) = self.msg_num {
            assert!(bc.meta.msg_num as u32 == msg_num);
        } else {
            log::debug!("Sending message before msg_num has been aquired");
        }
        self.conn.send(bc).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<Bc> {
        let bc = self.rx.next().await.ok_or(Error::DroppedSubscriber)?;
        if let Ok(bc) = &bc {
            if let Some(msg_num) = self.msg_num {
                assert!(bc.meta.msg_num as u32 == msg_num);
            } else {
                // Leaning number now
                self.msg_num = Some(bc.meta.msg_num as u32);
            }
        }
        bc
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
    pub fn bcmedia_stream(&'_ mut self, strict: bool) -> BcMediaStream<'_> {
        let async_read = BcPayloadStream { rx: &mut self.rx }
            .into_async_read()
            .compat();
        FramedRead::new(async_read, BcMediaCodex::new(strict))
    }
}
