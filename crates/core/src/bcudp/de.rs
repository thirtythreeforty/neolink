use super::{crc::calc_crc, model::*, xml::*, xml_crypto::decrypt};
use crate::Error;
use bytes::{Buf, BytesMut};
use nom::{
    combinator::*,
    error::{context as error_context, ContextError, ErrorKind, ParseError},
    number::streaming::*,
    take, Err,
};

type IResult<I, O, E = nom::error::VerboseError<I>> = Result<(I, O), nom::Err<E>>;

fn make_error<I, E: ParseError<I>>(input: I, ctx: &'static str, kind: ErrorKind) -> E
where
    I: std::marker::Copy,
    E: ContextError<I>,
{
    E::add_context(input, ctx, E::from_error_kind(input, kind))
}

impl BcUdp {
    pub(crate) fn deserialize(buf: &mut BytesMut) -> Result<BcUdp, Error> {
        const TYPICAL_HEADER: usize = 20;
        let (result, len) = match consumed(bcudp)(buf) {
            Ok((_, (parsed_buff, result))) => Ok((result, parsed_buff.len())),
            Err(e) => Err(e),
        }?;
        buf.advance(len);
        buf.reserve(len + TYPICAL_HEADER); // Preallocate for future buffer calls
        Ok(result)
    }
}

fn bcudp(buf: &[u8]) -> IResult<&[u8], BcUdp> {
    let (buf, magic) = error_context(
        "Magic is invalid",
        verify(le_u32, |x| {
            matches!(
                *x,
                MAGIC_HEADER_UDP_NEGO | MAGIC_HEADER_UDP_ACK | MAGIC_HEADER_UDP_DATA
            )
        }),
    )(buf)?;

    match magic {
        MAGIC_HEADER_UDP_NEGO => {
            let (buf, payload) = udp_disc(buf)?;
            Ok((buf, BcUdp::Discovery(payload)))
        }
        MAGIC_HEADER_UDP_ACK => {
            let (buf, payload) = udp_ack(buf)?;
            Ok((buf, BcUdp::Ack(payload)))
        }
        MAGIC_HEADER_UDP_DATA => {
            let (buf, payload) = udp_data(buf)?;
            Ok((buf, BcUdp::Data(payload)))
        }
        _ => unreachable!(),
    }
}

fn udp_disc(buf: &[u8]) -> IResult<&[u8], UdpDiscovery> {
    let (buf, payload_size) = error_context("DISC: Missing payload size", le_u32)(buf)?;
    let (buf, _unknown_a) = error_context(
        "DISC: Unable to verify UnknowA",
        verify(le_u32, |&x| x == 1),
    )(buf)?;
    let (buf, tid) = error_context("DISC: Missing TID", le_u32)(buf)?;
    let (buf, checksum) = error_context("DISC: Missing checksum", le_u32)(buf)?;
    let (buf, enc_data_slice) = take!(buf, payload_size)?;

    let actual_checksum = calc_crc(enc_data_slice);
    assert_eq!(checksum, actual_checksum);

    let decrypted_payload = decrypt(tid, enc_data_slice);
    let payload = UdpXml::try_parse(decrypted_payload.as_slice()).map_err(|_| {
        Err::Error(make_error(
            buf,
            "DISC: Unable to decode UDPXml",
            ErrorKind::MapRes,
        ))
    })?;

    let data = UdpDiscovery { tid, payload };
    Ok((buf, data))
}

fn udp_ack(buf: &[u8]) -> IResult<&[u8], UdpAck> {
    let (buf, connection_id) = error_context("ACK: Missing connect ID", le_i32)(buf)?;
    let (buf, _unknown_a) =
        error_context("ACK: Unable to verify UnknowA", verify(le_u32, |&x| x == 0))(buf)?;
    let (buf, group_id) = error_context(
        "ACK: Unable to verify UnknowB",
        verify(le_u32, |&x| x == 0 || x == 0xffffffff),
    )(buf)?;
    let (buf, packet_id) = error_context("Missing packet_id", le_u32)(buf)?; // This is the point at which the camera has contigious
                                                                             // packets to
    let (buf, _unknown) = error_context("ACK: Missing unknown", le_u32)(buf)?;
    let (buf, payload_size) = error_context("ACK: Missing payload_size", le_u32)(buf)?;
    let (buf, payload) = if payload_size > 0 {
        let (buf, t_payload) = take!(buf, payload_size)?; // It is a binary payload of
                                                          // `00 01 01 01 01 00 01`
                                                          // This is a truth map of missing packets
                                                          // since last contigious packet_id up
                                                          // to the last packet we sent and it recieved
        (buf, t_payload.to_vec())
    } else {
        (buf, vec![])
    };

    let data = UdpAck {
        connection_id,
        packet_id,
        group_id,
        payload,
    };
    Ok((buf, data))
}

fn udp_data(buf: &[u8]) -> IResult<&[u8], UdpData> {
    let (buf, connection_id) = error_context("DATA: Missing connection_id", le_i32)(buf)?;
    let (buf, _unknown_a) =
        error_context("DATA: Unable to verify UnownA", verify(le_u32, |&x| x == 0))(buf)?;
    let (buf, packet_id) = error_context("DATA: Missing packet_id", le_u32)(buf)?;
    let (buf, payload_size) = error_context("DATA: Missing payload_size", le_u32)(buf)?;
    let (buf, payload) = take!(buf, payload_size)?;

    let data = UdpData {
        connection_id,
        packet_id,
        payload: payload.to_vec(),
    };
    Ok((buf, data))
}

#[cfg(test)]
mod tests {
    use super::Error;
    use crate::bcudp::model::*;
    use crate::bcudp::xml::*;
    use assert_matches::assert_matches;
    use bytes::BytesMut;
    use env_logger::Env;
    use log::*;
    use std::io::ErrorKind;

    fn init() {
        let _ = env_logger::Builder::from_env(Env::default().default_filter_or("info"))
            .is_test(true)
            .try_init();
    }

    #[test]
    // Tests the decoding of a UdpDiscovery with a discovery xml
    fn test_nego_disconnect() {
        init();

        let sample = include_bytes!("samples/udp_negotiate_disc.bin");

        let e = BcUdp::deserialize(&mut BytesMut::from(&sample[..]));
        assert_matches!(
            e,
            Ok(BcUdp::Discovery(UdpDiscovery {
                tid: 96,
                payload: UdpXml {
                    c2d_disc: Some(C2dDisc {
                        cid: 82000,
                        did: 80,
                    }),
                    ..
                },
            }))
        );
    }

    #[test]
    // Tests the decoding of a UdpDiscovery with a Camera Transmission xml
    fn test_nego_cam_transmission() {
        init();

        let sample = include_bytes!("samples/udp_negotiate_camt.bin");

        let e = BcUdp::deserialize(&mut BytesMut::from(&sample[..]));
        assert_matches!(
            e,
            Ok(BcUdp::Discovery(UdpDiscovery {
                tid: 113,
                payload: UdpXml {
                    d2c_t: Some(D2cT {
                        sid: 62098713,
                        conn: conn_str,
                        cid: 82001,
                        did: 96,
                    }),
                    ..
                },
            })) if &conn_str == "local"
        );
    }

    #[test]
    // Tests the decoding of a UdpDiscovery with a Client Transmission xml
    fn test_nego_client_transmission() {
        init();

        let sample = include_bytes!("samples/udp_negotiate_clientt.bin");

        let e = BcUdp::deserialize(&mut BytesMut::from(&sample[..]));
        assert_matches!(
            e,
            Ok(BcUdp::Discovery(UdpDiscovery {
                tid: 1101,
                payload: UdpXml {
                    c2d_t: Some(C2dT {
                        sid: 62098713,
                        conn: conn_str,
                        cid: 82001,
                        mtu: 1350,
                    }),
                    ..
                },
            })) if &conn_str == "local"
        );
    }

    #[test]
    // Tests the decoding of a UdpDiscovery with a Camera CFM xml
    fn test_nego_cfm() {
        init();

        let sample = include_bytes!("samples/udp_negotiate_camcfm.bin");

        let e = BcUdp::deserialize(&mut BytesMut::from(&sample[..]));
        assert_matches!(
            e,
            Ok(BcUdp::Discovery(UdpDiscovery {
                tid: 1101,
                payload: UdpXml {
                    d2c_cfm: Some(D2cCfm {
                        sid: 62098713,
                        conn: conn_str,
                        rsp: 0,
                        cid: 82001,
                        did: 96,
                        time_r: 0,
                    }),
                    ..
                },
            })) if &conn_str == "local"
        );
    }

    #[test]
    // Tests the decoding of an acknoledge packet
    fn test_ack() {
        init();

        let sample = include_bytes!("samples/udp_ack.bin");

        let e = BcUdp::deserialize(&mut BytesMut::from(&sample[..]));
        assert_matches!(
            e,
            Ok(BcUdp::Ack(UdpAck {
                connection_id: 80,
                packet_id: 2439,
                ..
            }))
        );
    }

    #[test]
    // Tests the decoding of an data packet
    fn test_data() {
        init();

        let sample = include_bytes!("samples/udp_data.bin");

        let e = BcUdp::deserialize(&mut BytesMut::from(&sample[..]));
        assert_matches!(
            e,
            Ok(BcUdp::Data(UdpData {
                connection_id: 82000,
                packet_id: 2439,
                payload: payload_data
            })) if payload_data.len() == 1176
        );
    }

    #[test]
    // Tests the decoding of multiple packets
    fn test_multi_packets() {
        init();

        let sample = [
            include_bytes!("samples/udp_multi_0.bin").as_ref(),
            include_bytes!("samples/udp_multi_1.bin").as_ref(),
            include_bytes!("samples/udp_multi_2.bin").as_ref(),
            include_bytes!("samples/udp_multi_3.bin").as_ref(),
            include_bytes!("samples/udp_multi_4.bin").as_ref(),
            include_bytes!("samples/udp_multi_5.bin").as_ref(),
            include_bytes!("samples/udp_multi_6.bin").as_ref(),
            include_bytes!("samples/udp_multi_7.bin").as_ref(),
            include_bytes!("samples/udp_multi_8.bin").as_ref(),
            include_bytes!("samples/udp_multi_9.bin").as_ref(),
        ]
        .concat();

        // Should derealise all of this
        loop {
            let e = BcUdp::deserialize(&mut BytesMut::from(&sample[..]));
            match e {
                Err(Error::Io(e)) if e.kind() == ErrorKind::UnexpectedEof => {
                    // Reach end of files
                    break;
                }
                Err(Error::NomIncomplete(_)) => {
                    // Reach end of files
                    break;
                }
                Err(e) => {
                    error!("{:?}", e);
                    panic!();
                }
                Ok(_) => {}
            }
        }
    }
}
