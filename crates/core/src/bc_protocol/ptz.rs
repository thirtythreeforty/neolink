use super::{BcCamera, Error, Result};
use crate::bc::{model::*, xml::*};

/// Directions used for Ptz
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Direction {
    /// To move the camera Up
    Up,
    /// To move the camera Down
    Down,
    /// To move the camera Left
    Left,
    /// To move the camera Right
    Right,
    /// To stop currently active PTZ command
    Stop,
}

impl BcCamera {
    /// Send a PTZ message to the camera
    pub async fn send_ptz(&self, direction: Direction, amount: f32) -> Result<()> {
        self.has_ability_rw("control").await?;
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_set = connection.subscribe(MSG_ID_PTZ_CONTROL, msg_num).await?;

        let direction_str = match direction {
            Direction::Up => "up",
            Direction::Down => "down",
            Direction::Left => "left",
            Direction::Right => "right",
            Direction::Stop => "stop",
        }
        .to_string();
        let send = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_PTZ_CONTROL,
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
                    ptz_control: Some(PtzControl {
                        version: xml_ver(),
                        channel_id: self.channel_id,
                        speed: amount,
                        command: direction_str,
                    }),
                    ..Default::default()
                })),
            }),
        };

        sub_set.send(send).await?;
        let msg = sub_set.recv().await?;

        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
            Ok(())
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "The camera did not accept the PtzControl xml",
            })
        }
    }

    /// Get the [PtzPreset] XML which contains the list of the preset positions known to the camera
    pub async fn get_ptz_preset(&self) -> Result<PtzPreset> {
        self.has_ability_rw("control").await?;
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_set = connection.subscribe(MSG_ID_GET_PTZ_PRESET, msg_num).await?;

        let send = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_GET_PTZ_PRESET,
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

        sub_set.send(send).await?;
        let msg = sub_set.recv().await?;

        if let BcBody::ModernMsg(ModernMsg {
            payload:
                Some(BcPayloads::BcXml(BcXml {
                    ptz_preset: Some(ptz_preset),
                    ..
                })),
            ..
        }) = msg.body
        {
            Ok(ptz_preset)
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "The camera did not return a valid PtzPreset xml",
            })
        }
    }

    /// Set a PTZ preset.
    ///
    /// The current position will be saved as a preset with the given [preset_id] and [name]
    pub async fn set_ptz_preset(&self, preset_id: u8, name: String) -> Result<()> {
        self.has_ability_rw("control").await?;
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_set = connection
            .subscribe(MSG_ID_PTZ_CONTROL_PRESET, msg_num)
            .await?;

        let preset = Preset {
            id: preset_id,
            name: Some(name),
            command: "setPos".to_owned(),
        };
        let send = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_PTZ_CONTROL_PRESET,
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
                    ptz_preset: Some(PtzPreset {
                        preset_list: PresetList {
                            preset: vec![preset],
                        },
                        ..Default::default()
                    }),
                    ..Default::default()
                })),
            }),
        };

        sub_set.send(send).await?;
        let msg = sub_set.recv().await?;

        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
            Ok(())
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "The camera did not accept the PtzPreset xml",
            })
        }
    }

    /// The camera will attempt to move to the preset with the given ID.
    pub async fn moveto_ptz_preset(&self, preset_id: u8) -> Result<()> {
        self.has_ability_rw("control").await?;
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_set = connection
            .subscribe(MSG_ID_PTZ_CONTROL_PRESET, msg_num)
            .await?;

        let preset = Preset {
            id: preset_id,
            name: None,
            command: "toPos".to_owned(),
        };
        let send = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_PTZ_CONTROL_PRESET,
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
                    ptz_preset: Some(PtzPreset {
                        preset_list: PresetList {
                            preset: vec![preset],
                        },
                        ..Default::default()
                    }),
                    ..Default::default()
                })),
            }),
        };

        sub_set.send(send).await?;
        let msg = sub_set.recv().await?;

        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
            Ok(())
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "The camera did not accept the PtzPreset xml",
            })
        }
    }

    /// The camera will zoom to a given zoom amount.
    /// Not sure what the units for this are, seems to be 1000 is 1x and 2000 is 2x
    pub async fn zoom_to(&self, in_zoom_pos: u32) -> Result<()> {
        let current = self.get_zoom().await?;
        let zoom_pos;
        if let Some(zoom) = current.zoom {
            zoom_pos = in_zoom_pos.clamp(zoom.minPos, zoom.maxPos);
        } else {
            zoom_pos = in_zoom_pos;
        }

        self.has_ability_rw("control").await?;
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_set = connection.subscribe(MSG_ID_SET_ZOOM_FOCUS, msg_num).await?;
        let send = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_SET_ZOOM_FOCUS,
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
                    start_zoom_focus: Some(StartZoomFocus {
                        version: xml_ver(),
                        channel_id: self.channel_id,
                        command: Some("zoomPos".to_string()),
                        move_pos: Some(zoom_pos),
                        ..Default::default()
                    }),
                    ..Default::default()
                })),
            }),
        };

        sub_set.send(send).await?;

        let msg = sub_set.recv().await?;

        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
            Ok(())
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "The camera did not accept the StartZoomFocus xml",
            })
        }
    }

    /// Get the zoom xml, that has current min and max zoom values
    pub async fn get_zoom(&self) -> Result<StartZoomFocus> {
        self.has_ability_ro("control").await?;
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_get = connection.subscribe(MSG_ID_GET_ZOOM_FOCUS, msg_num).await?;
        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_GET_ZOOM_FOCUS,
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

        sub_get.send(get).await?;
        let msg = sub_get.recv().await?;
        if msg.meta.response_code != 200 {
            return Err(Error::CameraServiceUnavaliable(msg.meta.response_code));
        }

        if let BcBody::ModernMsg(ModernMsg {
            payload:
                Some(BcPayloads::BcXml(BcXml {
                    start_zoom_focus: Some(xml),
                    ..
                })),
            ..
        }) = msg.body
        {
            Ok(xml)
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "Expected StartZoomFocus xml but it was not recieved",
            })
        }
    }
}
