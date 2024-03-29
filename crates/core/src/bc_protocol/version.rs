use super::{BcCamera, Error, Result, RX_TIMEOUT};
use crate::bc::{model::*, xml::*};

impl BcCamera {
    /// Request the [VersionInfo] xml
    pub fn version(&self) -> Result<VersionInfo> {
        let connection = self
            .connection
            .as_ref()
            .expect("Must be connected to get version info");
        let sub_version = connection.subscribe(MSG_ID_VERSION)?;

        let version = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_VERSION,
                channel_id: self.channel_id,
                msg_num: self.new_message_num(),
                stream_type: 0,
                response_code: 0,
                class: 0x6414, // IDK why
            },
            body: BcBody::ModernMsg(ModernMsg {
                ..Default::default()
            }),
        };

        sub_version.send(version)?;

        let modern_reply = sub_version.rx.recv_timeout(RX_TIMEOUT)?;
        let version_info;
        match modern_reply.body {
            BcBody::ModernMsg(ModernMsg {
                payload:
                    Some(BcPayloads::BcXml(BcXml {
                        version_info: Some(info),
                        ..
                    })),
                ..
            }) => {
                version_info = info;
            }
            _ => {
                return Err(Error::UnintelligibleReply {
                    reply: modern_reply,
                    why: "Expected a VersionInfo message",
                })
            }
        }

        Ok(version_info)
    }
}
