use super::{BcCamera, Error, Result, RX_TIMEOUT};
use crate::bc::{model::*, xml::*};

impl BcCamera {
    /// Get the [PtzPreset] XML which contains the list of the preset positions known to the camera
    pub fn get_ptz_preset(&self) -> Result<PtzPreset> {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to get time");
        let sub_set = connection.subscribe(MSG_ID_GET_PTZ_PRESET)?;

        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_GET_PTZ_PRESET,
                channel_id: self.channel_id,
                msg_num: self.new_message_num(),
                response_code: 0,
                stream_type: 0,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg {
                extension: Some(Extension {
                    channel_id: Some(self.channel_id),
                    ..Default::default()
                }),
                payload: None
            }),
        };
        sub_set.send(get)?;

        let msg = sub_set.rx.recv_timeout(RX_TIMEOUT)?;

        if let BcBody::ModernMsg(ModernMsg {
             payload: Some(BcPayloads::BcXml(BcXml {
                 ptz_preset: Some(ptz_preset),
                 ..
             })),
             ..
        }) = msg.body {
            Ok(ptz_preset)
        } else {
            Err(Error::UnintelligibleReply {
                reply: msg,
                why: "The camera did not return a valid PtzPreset xml",
            })
        }
    }

    /// Set a PTZ preset. If a [name] is given the current position will be saved as a preset
    /// with the given [preset_id] and [name], otherwise the camera will attempt to move to the
    /// preset with the given ID.
    pub fn set_ptz_preset(&self, preset_id: i8, name: Option<String>) -> Result<()> {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to get time");
        let sub_set = connection.subscribe(MSG_ID_PTZ_CONTROL_PRESET)?;
        let command = if name.is_some() {
            "setPos"
        } else {
            "toPos"
        };
        let preset = Preset {
            id: preset_id,
            name,
            command: Some(command.to_string()),
            ..Default::default()
        };
        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_PTZ_CONTROL_PRESET,
                channel_id: self.channel_id,
                msg_num: self.new_message_num(),
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
                    ptz_preset: Some(PtzPreset {
                        preset_list: Some(PresetList {
                            preset: vec![preset],
                        }),
                        ..Default::default()
                    }),
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
                reply: msg,
                why: "The camera did not accept the PtzPreset xml",
            })
        }
    }

    /// Move the camera with given speed and command. The only known value for speed is 32.
    /// [command] might be `left`, `right`, `up`, `down`, `leftUp`, `leftDown`, `rightUp`,
    /// `rightDown` or `stop`
    pub fn ptz_control(&self, speed: i8, command: String) -> Result<()> {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to get time");
        let sub_set = connection.subscribe(MSG_ID_PTZ_CONTROL)?;

        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_PTZ_CONTROL,
                channel_id: self.channel_id,
                msg_num: self.new_message_num(),
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
                    ptz_control: Some(PtzControl {
                        speed,
                        command,
                        ..Default::default()
                    }),
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
                reply: msg,
                why: "The camera did not accept the PTZ command",
            })
        }
    }
}