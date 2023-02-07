use super::{BcCamera, Error, Result};
use crate::bc::{model::*, xml::*};
use time::{date, Date, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

impl BcCamera {
    ///
    /// Get the time from the camera
    ///
    /// # Returns
    ///
    /// returns either an error or an option with the offsetted date time
    ///
    pub async fn get_time(&self) -> Result<Option<OffsetDateTime>> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_get_general = connection.subscribe(msg_num).await?;
        let get = Bc {
            meta: BcMeta {
                msg_id: MSG_ID_GET_GENERAL,
                channel_id: self.channel_id,
                msg_num,
                response_code: 0,
                stream_type: 0,
                class: 0x6414,
            },
            body: BcBody::ModernMsg(ModernMsg::default()),
        };

        sub_get_general.send(get).await?;
        let msg = sub_get_general.recv().await?;
        if msg.meta.response_code != 200 {
            return Err(Error::CameraServiceUnavaliable);
        }

        if let BcBody::ModernMsg(ModernMsg {
            payload:
                Some(BcPayloads::BcXml(BcXml {
                    system_general:
                        Some(SystemGeneral {
                            time_zone: Some(time_zone),
                            year: Some(year),
                            month: Some(month),
                            day: Some(day),
                            hour: Some(hour),
                            minute: Some(minute),
                            second: Some(second),
                            ..
                        }),
                    ..
                })),
            ..
        }) = msg.body
        {
            let datetime =
                match try_build_timestamp(time_zone, year, month, day, hour, minute, second) {
                    Ok(dt) => dt,
                    Err(_) => {
                        return Err(Error::UnintelligibleReply {
                            reply: std::sync::Arc::new(Box::new(msg)),
                            why: "Could not parse date",
                        })
                    }
                };

            // This code was written in 2020; I'm trying to catch all the possible epochs that
            // cameras might reset themselves to. My B800 resets to Jan 1, 1999, but I can't
            // guarantee that Reolink won't pick some newer date.  Therefore, last year ought
            // to be new enough, yet still distant enough that it won't interfere with anything
            const BOUNDARY: Date = date!(2019 - 01 - 01);

            // detect if no time is actually set, and return Ok(None): that is, operation
            // succeeded, and there is no time set
            if datetime.date() < BOUNDARY {
                Ok(None)
            } else {
                Ok(Some(datetime))
            }
        } else {
            Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "Reply did not contain SystemGeneral with all time fields filled out",
            })
        }
    }

    ///
    /// Sets the time of the camera
    ///
    /// # Parameters
    ///
    /// * `timestamp` - The time to set the camera to
    ///
    /// # Returns
    ///
    /// returns Ok(()) or error
    ///
    pub async fn set_time(&self, timestamp: OffsetDateTime) -> Result<()> {
        let connection = self.get_connection();
        let msg_num = self.new_message_num();
        let mut sub_set_general = connection.subscribe(msg_num).await?;
        let set = Bc::new_from_xml(
            BcMeta {
                msg_id: MSG_ID_SET_GENERAL,
                channel_id: self.channel_id,
                msg_num,
                response_code: 0,
                stream_type: 0,
                class: 0x6414,
            },
            BcXml {
                system_general: Some(SystemGeneral {
                    version: xml_ver(),
                    //osd_format: Some("MDY".to_string()),
                    time_format: Some(0),
                    // Reolink uses positive seconds to indicate a negative UTC offset:
                    time_zone: Some(-timestamp.offset().as_seconds()),
                    year: Some(timestamp.year()),
                    month: Some(timestamp.month()),
                    day: Some(timestamp.day()),
                    hour: Some(timestamp.hour()),
                    minute: Some(timestamp.minute()),
                    second: Some(timestamp.second()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );

        sub_set_general.send(set).await?;
        let msg = sub_set_general.recv().await?;
        if let BcMeta {
            response_code: 200, ..
        } = msg.meta
        {
        } else {
            return Err(Error::UnintelligibleReply {
                reply: std::sync::Arc::new(Box::new(msg)),
                why: "The camera did not accept the set time command.",
            });
        }

        Ok(())
    }
}

fn try_build_timestamp(
    timezone: i32,
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
) -> std::result::Result<OffsetDateTime, time::ComponentRangeError> {
    let date = Date::try_from_ymd(year, month, day)?;
    let time = Time::try_from_hms(hour, minute, second)?;
    let offset = if timezone > 0 {
        UtcOffset::west_seconds(timezone as u32)
    } else {
        UtcOffset::east_seconds(-timezone as u32)
    };

    Ok(PrimitiveDateTime::new(date, time).assume_offset(offset))
}
