use super::{crc::calc_crc, model::*, xml::*, xml_crypto::decrypt};
use err_derive::Error;
use nom::IResult;
use nom::{
    combinator::*,
    error::{make_error, ErrorKind},
    number::streaming::*,
    take, Err,
};
use std::io::Read;

/// The error types used during deserialisation
#[derive(Debug, Error)]
pub enum Error {
    /// A Nom parsing error usually a malformed packet
    #[error(display = "Parsing error")]
    NomError(String),
    /// An IO error such as the stream being dropped
    #[error(display = "I/O error")]
    IoError(#[error(source)] std::io::Error),
}
type NomErrorType<'a> = nom::error::Error<&'a [u8]>;

impl<'a> From<nom::Err<NomErrorType<'a>>> for Error {
    fn from(k: nom::Err<NomErrorType<'a>>) -> Self {
        let reason = match k {
            nom::Err::Error(e) => format!("Nom Error: {:?}", e),
            nom::Err::Failure(e) => format!("Nom Error: {:?}", e),
            _ => "Unknown Nom error".to_string(),
        };
        Error::NomError(reason)
    }
}

fn read_from_reader<P, O, E, R>(mut parser: P, mut rdr: R) -> Result<O, E>
where
    R: Read,
    E: for<'a> From<nom::Err<NomErrorType<'a>>> + From<std::io::Error>,
    P: FnMut(&[u8]) -> nom::IResult<&[u8], O>,
{
    let mut input: Vec<u8> = Vec::new();
    loop {
        let to_read = match parser(&input) {
            Ok((_, parsed)) => return Ok(parsed),
            Err(nom::Err::Incomplete(needed)) => {
                match needed {
                    nom::Needed::Unknown => std::num::NonZeroUsize::new(1).unwrap(), // read one byte
                    nom::Needed::Size(len) => len,
                }
            }
            Err(e) => return Err(e.into()),
        };

        if 0 == (&mut rdr)
            .take(to_read.get() as u64)
            .read_to_end(&mut input)?
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Read returned 0 bytes",
            )
            .into());
        }
    }
}

impl BcUdp {
    pub(crate) fn deserialize<R: Read>(r: R) -> Result<BcUdp, Error> {
        // Throw away the nom-specific return types
        read_from_reader(|reader| bcudp(reader), r)
    }
}

fn bcudp(buf: &[u8]) -> IResult<&[u8], BcUdp> {
    let (buf, magic) = verify(le_u32, |x| {
        matches!(
            *x,
            MAGIC_HEADER_UDP_NEGO | MAGIC_HEADER_UDP_ACK | MAGIC_HEADER_UDP_DATA
        )
    })(buf)?;

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
    let (buf, payload_size) = le_u32(buf)?;
    let (buf, _unknown_a) = verify(le_u32, |&x| x == 1)(buf)?;
    let (buf, tid) = le_u32(buf)?;
    let (buf, checksum) = le_u32(buf)?;
    let (buf, enc_data_slice) = take!(buf, payload_size)?;

    let actual_checksum = calc_crc(enc_data_slice);
    assert_eq!(checksum, actual_checksum);

    let decrypted_payload = decrypt(tid, enc_data_slice);
    let payload = UdpXml::try_parse(decrypted_payload.as_slice())
        .map_err(|_| Err::Error(make_error(buf, ErrorKind::MapRes)))?;

    let data = UdpDiscovery { tid, payload };
    Ok((buf, data))
}

fn udp_ack(buf: &[u8]) -> IResult<&[u8], UdpAck> {
    let (buf, connection_id) = le_u32(buf)?;
    let (buf, _unknown_a) = verify(le_u32, |&x| x == 0)(buf)?;
    let (buf, _unknown_b) = verify(le_u32, |&x| x == 0)(buf)?;
    let (buf, packet_id) = le_u32(buf)?;
    let (buf, _unknown_c) = le_u32(buf)?;
    let (buf, _unknown_d) = verify(le_u32, |&x| x == 0)(buf)?;

    let data = UdpAck {
        connection_id,
        packet_id,
    };
    Ok((buf, data))
}

fn udp_data(buf: &[u8]) -> IResult<&[u8], UdpData> {
    let (buf, connection_id) = le_u32(buf)?;
    let (buf, _unknown_a) = verify(le_u32, |&x| x == 0)(buf)?;
    let (buf, packet_id) = le_u32(buf)?;
    let (buf, payload_size) = le_u32(buf)?;
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
    use crate::bc_protocol::FileSubscriber;
    use crate::bcudp::model::*;
    use crate::bcudp::xml::*;
    use assert_matches::assert_matches;
    use env_logger::Env;
    use log::*;
    use std::io::ErrorKind;
    use std::path::PathBuf;

    fn init() {
        let _ = env_logger::Builder::from_env(Env::default().default_filter_or("info"))
            .is_test(true)
            .try_init();
    }

    fn sample(name: &str) -> PathBuf {
        let dir = std::env::current_dir().unwrap(); // This is crate root during cargo test
        dir.join("src").join("bcudp").join("samples").join(name)
    }

    #[test]
    // Tests the decoding of a UdpDiscovery with a discovery xml
    fn test_nego_disconnect() {
        init();

        let mut subsciber = FileSubscriber::from_files(vec![sample("udp_negotiate_disc.bin")]);

        let e = BcUdp::deserialize(&mut subsciber);
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

        let mut subsciber = FileSubscriber::from_files(vec![sample("udp_negotiate_camt.bin")]);

        let e = BcUdp::deserialize(&mut subsciber);
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

        let mut subsciber = FileSubscriber::from_files(vec![sample("udp_negotiate_clientt.bin")]);

        let e = BcUdp::deserialize(&mut subsciber);
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

        let mut subsciber = FileSubscriber::from_files(vec![sample("udp_negotiate_camcfm.bin")]);

        let e = BcUdp::deserialize(&mut subsciber);
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

        let mut subsciber = FileSubscriber::from_files(vec![sample("udp_ack.bin")]);

        let e = BcUdp::deserialize(&mut subsciber);
        assert_matches!(
            e,
            Ok(BcUdp::Ack(UdpAck {
                connection_id: 80,
                packet_id: 2439,
            }))
        );
    }

    #[test]
    // Tests the decoding of an data packet
    fn test_data() {
        init();

        let mut subsciber = FileSubscriber::from_files(vec![sample("udp_data.bin")]);

        let e = BcUdp::deserialize(&mut subsciber);
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

        let mut subsciber = FileSubscriber::from_files(vec![
            sample("udp_multi_0.bin"),
            sample("udp_multi_1.bin"),
            sample("udp_multi_2.bin"),
            sample("udp_multi_3.bin"),
            sample("udp_multi_4.bin"),
            sample("udp_multi_5.bin"),
            sample("udp_multi_6.bin"),
            sample("udp_multi_7.bin"),
            sample("udp_multi_8.bin"),
            sample("udp_multi_9.bin"),
        ]);

        // Should derealise all of this
        loop {
            let e = BcUdp::deserialize(&mut subsciber);
            match e {
                Err(Error::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => {
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
