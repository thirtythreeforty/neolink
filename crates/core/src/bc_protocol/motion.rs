use super::{BcCamera, Error, Result, RX_TIMEOUT};
use crate::bc::{model::*, xml::*};

/// Motion Status that the callback can send
pub enum MotionStatus {
    /// Sent when motion is first detected
    Start,
    /// Sent when motion stops
    Stop,
    /// Sent when an Alarm about something other than motion was received
    NoChange,
}

/// This is a conveince type for the error of the MotionOutput callback
pub type MotionOutputError = Result<bool>;

/// Trait used as part of [`listen_on_motion`] to send motion messages
pub trait MotionOutput {
    /// This is the callback used when motion is received
    ///
    /// If result is `Ok(true)` more messages will be sent
    ///
    /// If result if `Ok(false)` then message will be stopped
    ///
    /// If result is `Err(E)` then motion messages be stopped
    /// and an error will be thrown
    fn motion_recv(&mut self, motion_status: MotionStatus) -> MotionOutputError;
}

impl BcCamera {
    /// This message tells the camera to send the motion events to us
    /// Which are the recieved on msgid 33
    fn start_motion_query(&self) -> Result<u16> {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        let msg_num = self.new_message_num();
        let sub = connection.subscribe(msg_num)?;
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

        sub.send(msg)?;

        let msg = sub.rx.recv_timeout(RX_TIMEOUT)?;

        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
            Ok(msg_num)
        } else {
            Err(Error::UnintelligibleReply {
                reply: Box::new(msg),
                why: "The camera did not accept the request to start motion",
            })
        }
    }

    /// This requests that motion messages be listen to and sent to the
    /// output struct.
    ///
    /// The output structure must implement the [`MotionCallback`] trait
    pub fn listen_on_motion<T: MotionOutput>(&self, data_out: &mut T) -> Result<()> {
        let msg_num = self.start_motion_query()?;

        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to listen to messages");

        // After start_motion_query (MSG_ID 31) the camera sends motion messages
        // when whenever motion is detected.
        let sub = connection.subscribe(msg_num)?;

        loop {
            // Mostly ignore when timout is reached because these messages are only
            // sent when motion is detected which might means hours between messages
            // being received
            let msg = sub.rx.recv_timeout(RX_TIMEOUT);
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
                        let mut result = MotionStatus::NoChange;
                        for alarm_event in &alarm_event_list.alarm_events {
                            if alarm_event.channel_id == self.channel_id {
                                if alarm_event.status == "MD" {
                                    result = MotionStatus::Start;
                                    break;
                                } else if alarm_event.status == "none" {
                                    result = MotionStatus::Stop;
                                    break;
                                }
                            }
                        }
                        Ok(result)
                    } else {
                        Ok(MotionStatus::NoChange)
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(MotionStatus::NoChange),
                // On connection drop we stop
                Err(e @ std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(e),
            }?;

            match data_out.motion_recv(status) {
                Ok(true) => {}
                Ok(false) => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    }
}
