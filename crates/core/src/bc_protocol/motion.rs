use super::{BcCamera, Error, Result};
use crate::bc::{model::*, xml::*};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{channel, error::TryRecvError, Receiver};
use tokio::task::{self, JoinHandle};

/// Motion Status that the callback can send
#[derive(Clone, Copy, Debug)]
pub enum MotionStatus {
    /// Sent when motion is first detected
    Start(Instant),
    /// Sent when motion stops
    Stop(Instant),
    /// Sent when an Alarm about something other than motion was received
    NoChange(Instant),
}

/// A handle on current motion related events comming from the camera
///
/// When this object is dropped the motion events are stopped
pub struct MotionData {
    handle: Option<JoinHandle<Result<()>>>,
    rx: Receiver<MotionStatus>,
    last_update: MotionStatus,
}

impl MotionData {
    /// Get if motion has been detected. Returns None if
    /// no motion data has yet been recieved from the camera
    ///
    /// An error is raised if the motion connection to the camera is dropped
    pub fn motion_detected(&mut self) -> Result<Option<bool>> {
        self.consume_motion_events()?;
        Ok(match &self.last_update {
            MotionStatus::Start(_) => Some(true),
            MotionStatus::Stop(_) => Some(false),
            MotionStatus::NoChange(_) => None,
        })
    }

    /// Get if motion has been detected within given duration. Returns None if
    /// no motion data has yet been recieved from the camera
    ///
    /// An error is raised if the motion connection to the camera is dropped
    pub fn motion_detected_within(&mut self, duration: Duration) -> Result<Option<bool>> {
        self.consume_motion_events()?;
        Ok(match &self.last_update {
            MotionStatus::Start(time) => Some((Instant::now() - *time) < duration),
            MotionStatus::Stop(time) => Some((Instant::now() - *time) < duration),
            MotionStatus::NoChange(_) => None,
        })
    }

    /// Consume the motion events diretly
    ///
    /// An error is raised if the motion connection to the camera is dropped
    pub fn consume_motion_events(&mut self) -> Result<Vec<MotionStatus>> {
        let mut results: Vec<MotionStatus> = vec![];
        loop {
            match self.rx.try_recv() {
                Ok(motion) => results.push(motion),
                Err(TryRecvError::Empty) => break,
                Err(e) => return Err(Error::from(e)),
            }
        }
        if let Some(last) = results.last() {
            self.last_update = *last;
        }
        Ok(results)
    }
}

impl Drop for MotionData {
    fn drop(&mut self) {
        if let Some(h) = self.handle.take() {
            let backoff = crossbeam::utils::Backoff::new();
            h.abort();
            while !h.is_finished() {
                backoff.snooze();
            }
        }
    }
}

impl BcCamera {
    /// This message tells the camera to send the motion events to us
    /// Which are the recieved on msgid 33
    async fn start_motion_query(&self) -> Result<u16> {
        let connection = self.get_connection();

        let msg_num = self.new_message_num();
        let mut sub = connection.subscribe(msg_num).await?;
        let msg = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_MOTION_REQUEST,
                channel_id: self.channel_id,
                msg_num,
                stream_type: 0,
                response_code: 0,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg {
                ..Default::default()
            }),
        };

        sub.send(msg).await?;

        let msg = sub.recv().await?;

        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
            Ok(msg_num)
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "The camera did not accept the request to start motion",
            })
        }
    }

    /// This returns a data structure which can be used to
    /// query motion events
    pub async fn listen_on_motion(&self) -> Result<MotionData> {
        let msg_num = self.start_motion_query().await?;

        let connection = self.get_connection();

        // After start_motion_query (MSG_ID 31) the camera sends motion messages
        // when whenever motion is detected.
        let (tx, rx) = channel(20);

        let channel_id = self.channel_id;
        let handle = task::spawn(async move {
            let mut sub = connection.subscribe(msg_num).await?;

            loop {
                let msg = sub.recv().await;
                let status = match msg {
                    Ok(motion_msg) => {
                        if let BcBody::ModernMsg(ModernMsg {
                            payload:
                                Some(BcPayloads::BcXml(BcXml {
                                    alarm_event_list: Some(alarm_event_list),
                                    ..
                                })),
                            ..
                        }) = motion_msg.body
                        {
                            let mut result = MotionStatus::NoChange(Instant::now());
                            for alarm_event in &alarm_event_list.alarm_events {
                                if alarm_event.channel_id == channel_id {
                                    if alarm_event.status == "MD" {
                                        result = MotionStatus::Start(Instant::now());
                                        break;
                                    } else if alarm_event.status == "none" {
                                        result = MotionStatus::Stop(Instant::now());
                                        break;
                                    }
                                }
                            }
                            Ok(result)
                        } else {
                            Ok(MotionStatus::NoChange(Instant::now()))
                        }
                    }
                    // On connection drop we stop
                    Err(e) => Err(e),
                }?;

                if tx.send(status).await.is_err() {
                    // Motion reciever has been dropped
                    break;
                }
            }
            Ok(())
        });

        Ok(MotionData {
            handle: Some(handle),
            rx,
            last_update: MotionStatus::NoChange(Instant::now()),
        })
    }
}
