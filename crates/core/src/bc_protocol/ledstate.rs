use super::{BcCamera, Error, Result, RX_TIMEOUT};
use crate::bc::{model::*, xml::*};

impl BcCamera {
    /// Get the [LedState] xml which contains the LED status of the camera
    pub fn get_ledstate(&self) -> Result<LedState> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let sub_get = connection.subscribe(msg_num)?;
        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_GET_LED_STATUS,
                channel_id: self.channel_id,
                msg_num,
                response_code: 0,
                stream_type: 0,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg {
                extension: Some(Extension {
                    channel_id: Some(self.channel_id),
                    ..Default::default()
                }),
                payload: None,
            }),
        };

        sub_get.send(get)?;
        let msg = sub_get.rx.recv_timeout(RX_TIMEOUT)?;
        if msg.meta.response_code != 200 {
            return Err(Error::CameraServiceUnavaliable);
        }

        if let BcBody::ModernMsg(ModernMsg {
            payload:
                Some(BcPayloads::BcXml(BcXml {
                    led_state: Some(ledstate),
                    ..
                })),
            ..
        }) = msg.body
        {
            Ok(ledstate)
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "Expected LEDState xml but it was not recieved",
            })
        }
    }

    /// Set the led lights using the [LedState] xml
    pub fn set_ledstate(&self, mut led_state: LedState) -> Result<()> {
        let connection = self.get_connection();

        let msg_num = self.new_message_num();
        let sub_set = connection.subscribe(msg_num)?;

        // led_version is a field recieved from the camera but not sent
        // we set to None to ensure we don't send it to the camera
        led_state.led_version = None;
        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_SET_LED_STATUS,
                channel_id: self.channel_id,
                msg_num,
                response_code: 0,
                stream_type: 0,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg {
                extension: Some(Extension {
                    channel_id: Some(self.channel_id),
                    ..Default::default()
                }),
                payload: Some(BcPayloads::BcXml(BcXml {
                    led_state: Some(led_state),
                    ..Default::default()
                })),
            }),
        };

        sub_set.send(get)?;
        let msg = sub_set.rx.recv_timeout(RX_TIMEOUT)?;

        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
            Ok(())
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "The camera did not except the LEDState xml",
            })
        }
    }

    /// This is a convience function to control the IR LED lights
    ///
    /// This is for the RED IR lights that can come on automaitcally
    /// during low light.
    pub fn irled_light_set(&self, state: LightState) -> Result<()> {
        let mut led_state = self.get_ledstate()?;
        led_state.state = match state {
            LightState::On => "open".to_string(),
            LightState::Off => "close".to_string(),
            LightState::Auto => "auto".to_string(),
        };
        self.set_ledstate(led_state)?;
        Ok(())
    }

    /// This is a convience function to control the LED light
    /// True is on and false is off
    ///
    /// This is for the little blue on light of some camera
    pub fn led_light_set(&self, state: bool) -> Result<()> {
        let mut led_state = self.get_ledstate()?;
        led_state.light_state = match state {
            true => "open".to_string(),
            false => "close".to_string(),
        };
        self.set_ledstate(led_state)?;
        Ok(())
    }
}

/// This is pased to `irled_light_set` to turn it on, off or set it to light based auto
pub enum LightState {
    /// Turn the light on
    On,
    /// Turn the light off
    Off,
    /// Set the light to light based auto
    Auto,
}
