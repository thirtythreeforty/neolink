use super::{crc::calc_crc, model::*, xml_crypto::encrypt};
use crate::Error;
use cookie_factory::bytes::*;
use cookie_factory::sequence::tuple;
use cookie_factory::SerializeFn;
use cookie_factory::{combinator::*, gen};
use std::io::Write;

impl BcUdp {
    pub(crate) fn serialize<W: Write>(&self, buf: W) -> Result<W, Error> {
        let (buf, _) = match &self {
            BcUdp::Discovery(payload) => {
                let xml_payload = encrypt(payload.tid, &payload.payload.serialize(vec![]).unwrap());
                gen(bcudp_disc(payload, &xml_payload), buf)?
            }
            BcUdp::Ack(payload) => {
                let binary_payload = &payload.payload;
                gen(bcudp_ack(payload, binary_payload), buf)?
            }
            BcUdp::Data(payload) => {
                let binary_payload = &payload.payload;
                gen(bcudp_data(payload, binary_payload), buf)?
            }
        };

        Ok(buf)
    }
}

fn bcudp_disc<'a, W: 'a + Write>(
    payload: &'a UdpDiscovery,
    xml_payload: &'a [u8],
) -> impl SerializeFn<W> + 'a {
    let checksum = calc_crc(xml_payload);
    tuple((
        le_u32(MAGIC_HEADER_UDP_NEGO),
        le_u32(xml_payload.len() as u32),
        le_u32(1),
        le_u32(payload.tid),
        le_u32(checksum),
        slice(xml_payload),
    ))
}

fn bcudp_ack<'a, W: 'a + Write>(
    payload: &'a UdpAck,
    binary_payload: &'a [u8],
) -> impl SerializeFn<W> + 'a {
    tuple((
        le_u32(MAGIC_HEADER_UDP_ACK),
        le_i32(payload.connection_id),
        le_u32(0),
        le_u32(0),
        le_u32(payload.packet_id),
        le_u32(0),
        le_u32(binary_payload.len() as u32),
        slice(binary_payload),
    ))
}

fn bcudp_data<'a, W: 'a + Write>(
    payload: &'a UdpData,
    binary_payload: &'a [u8],
) -> impl SerializeFn<W> + 'a {
    tuple((
        le_u32(MAGIC_HEADER_UDP_DATA),
        le_i32(payload.connection_id),
        le_u32(0),
        le_u32(payload.packet_id),
        le_u32(binary_payload.len() as u32),
        slice(binary_payload),
    ))
}

#[cfg(test)]
mod tests {
    use crate::bcudp::model::*;
    use bytes::BytesMut;
    use env_logger::Env;

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

        let msg = BcUdp::deserialize(&mut BytesMut::from(&sample[..])).unwrap();
        let ser_buf: Vec<u8> = msg.serialize(vec![]).unwrap();
        let msg2 = BcUdp::deserialize(&mut BytesMut::from(ser_buf.as_slice())).unwrap();
        assert_eq!(msg, msg2);
        // Raw samples don't quite match exactly
        // because the yaserde for xml puts spaces and new lines in different places
        // then the raw data from the camera so we skip this last assert
        //assert_eq!(&sample[..], ser_buf.as_slice());
    }

    #[test]
    // Tests the decoding of a UdpDiscovery with a Camera Transmission xml
    fn test_nego_cam_transmission() {
        init();

        let sample = include_bytes!("samples/udp_negotiate_camt.bin");

        let msg = BcUdp::deserialize(&mut BytesMut::from(&sample[..])).unwrap();
        let ser_buf = msg.serialize(vec![]).unwrap();
        let msg2 = BcUdp::deserialize(&mut BytesMut::from(ser_buf.as_slice())).unwrap();
        assert_eq!(msg, msg2);
        // Raw samples don't quite match exactly
        // because the yaserde for xml puts spaces and new lines in different places
        // then the raw data from the camera so we skip this last assert
        //assert_eq!(&sample[..], ser_buf.as_slice());
    }

    #[test]
    // Tests the decoding of a UdpDiscovery with a Client Transmission xml
    fn test_nego_client_transmission() {
        init();

        let sample = include_bytes!("samples/udp_negotiate_clientt.bin");

        let msg = BcUdp::deserialize(&mut BytesMut::from(&sample[..])).unwrap();
        let ser_buf = msg.serialize(vec![]).unwrap();
        let msg2 = BcUdp::deserialize(&mut BytesMut::from(ser_buf.as_slice())).unwrap();
        assert_eq!(msg, msg2);
        // Raw samples don't quite match exactly
        // because the yaserde for xml puts spaces and new lines in different places
        // then the raw data from the camera so we skip this last assert
        //assert_eq!(&sample[..], ser_buf.as_slice());
    }

    #[test]
    // Tests the decoding of a UdpDiscovery with a Camera CFM xml
    fn test_nego_cfm() {
        init();

        let sample = include_bytes!("samples/udp_negotiate_camcfm.bin");

        let msg = BcUdp::deserialize(&mut BytesMut::from(&sample[..])).unwrap();
        let ser_buf = msg.serialize(vec![]).unwrap();
        let msg2 = BcUdp::deserialize(&mut BytesMut::from(ser_buf.as_slice())).unwrap();
        assert_eq!(msg, msg2);
        // Raw samples don't quite match exactly
        // because the yaserde for xml puts spaces and new lines in different places
        // then the raw data from the camera so we skip this last assert
        //assert_eq!(&sample[..], ser_buf.as_slice());
    }

    #[test]
    // Tests the decoding of an acknoledge packet
    fn test_ack() {
        init();

        let sample = include_bytes!("samples/udp_ack.bin");

        let msg = BcUdp::deserialize(&mut BytesMut::from(&sample[..])).unwrap();
        let ser_buf = msg.serialize(vec![]).unwrap();
        let msg2 = BcUdp::deserialize(&mut BytesMut::from(ser_buf.as_slice())).unwrap();
        assert_eq!(msg, msg2);
        assert_eq!(&sample[..], ser_buf.as_slice());
    }

    #[test]
    // Tests the decoding of an data packet
    fn test_data() {
        init();

        let sample = include_bytes!("samples/udp_data.bin");

        let msg = BcUdp::deserialize(&mut BytesMut::from(&sample[..])).unwrap();
        let ser_buf = msg.serialize(vec![]).unwrap();
        let msg2 = BcUdp::deserialize(&mut BytesMut::from(ser_buf.as_slice())).unwrap();
        assert_eq!(msg, msg2);
        assert_eq!(&sample[..], ser_buf.as_slice());
    }
}
