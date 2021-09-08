# BcUDP

This document describes the UDP protocol. UDP, unlike TCP, is lossy, there
is no guarantee that all packets sent will be received or that they are received
in the same order. As such the protocol involves sending meta data back and forth
where the camera and client acknowledge packets they have received.

Additionally the UDP max data size is smaller so packets will be split more
perhaps even several UDP packets per BC message, whereas in TCP the Bc messages were
always in one TCP packet.

# Packets Types

There are three types of UDP packets with their own header

- UDP Discovery: These packets have the magic `3acf872a` and contain encrypted
                 xml about the connection that will be used
- UDP Ack:       These packets have the magic `20cf872a` they are header only and
                 contain acknowledgement of packets
- UDP Data:      These packets contain part of a BC messagse with a special UDP
                 header that describes the udp message number, to help reassemble
                 the packets. They have the magic: `10cf872a`

# UDP Discovery

These messages are sent as part of the initial connection discovery.
It is a sequence of messages with different xmls.

If there is no reply is received within  500ms the last message is resent.

## Header 20 Bytes

- 4 Bytes magic: `3acf872a`
- 4 Bytes data size: Size in bytes of payload
- 4 Bytes unknown: Always `01000000` for for UDP Discovery
- 4 Bytes Transmission ID: The unique ID of the transmission, every successful
                           round trip of Discovery it is incremented.
                           The number is the same for the same transmission
                           and may be used to identify repeated messages.
                           It is also used for the encryption.
- 4 Bytes Checksum: The checksum of the payload encrypted payload

The checksum can be calculated with a polynomial of `0x04c11db7`, an init value
of `0x00000000` and an  xorout of `0x00000000`.


## Payload

- The XML payloads are encrypted with a simple xor algorithm. See the full
  source in neolink for details.

## Start Discovery payload

Client send this packet as a broadcast on 255.255.255.255 on port 2015
to init a connection

```xml
<P2P>
  <C2D_S>
    <to>
      <port>57268</port>
    </to>
  </C2D_S>
</P2P>
```

It also sends this binary on port 2000 as a broadcast

```
aaaa0000
```

---

**Camera Replies with Binary**

Camera replies with binary data to port 3000

This data seems to include the
- Camera name
- ip address
- TCP port
- Camera UID


**Tcp camera**

If the camera supports tcp the client will use this binary data to open a
standard tcp bc connection

**Udp Camera**

If the camera supports UDP the client will send the following xml packet
as a broadcast on 255.255.255.255 on port 2018/2015

**Known UID**

If the UID is already known you can skip to this step.

```xml
<P2P>
  <C2D_C>
    <uid>95270000YGAKNWKJ</uid>
    <cli>
      <port>24862</port>
    </cli>
    <cid>849013</cid>
    <mtu>1350</mtu>
    <debug>0</debug>
    <p>MAC</p>
  </C2D_C>
</P2P>
```

**Both**

A camera can do both of the above. In this case it will try to login
to both udp and tcp. Then the udp will disconnect.

---

## D2C_C_R Payload Camera

Camera replies with this payload on the port specified in `C2D_C`

```xml
<P2P>
  <D2C_C_R>
    <timer>
      <def>3000</def>
      <hb>10000</hb>
      <hbt>60000</hbt>
    </timer>
    <rsp>0</rsp>
    <cid>849013</cid>
    <did>192</did>
  </D2C_C_R>
</P2P>
```

- **timer** Unknown timer of some sort`
- **rsp**: Unknown
- **cid**: The connection ID of the client
- **did**: The connection ID of the camera



## T Payload Client

Client then replies with this.

```xml
<P2P>
  <C2D_T>
    <sid>62097899</sid>
    <conn>local</conn>
    <cid>82000</cid>
    <mtu>1350</mtu>
  </C2D_T>
</P2P>
```

- **sid**: ID of the camera
- **conn**: Type of connection only observed `local` value
- **cid**: The connection ID of the client
- **did**: The connection ID of the camera
- **mtu**: The maximum transmission unit of the connection. Which is the
           largest packet size in bytes


## T Payload Camera

Camera replies with this payload on the port specified in `C2D_C`

```xml
<P2P>
<D2C_T>
<sid>62097899</sid>
<conn>local</conn>
<cid>82000</cid>
<did>528</did>
</D2C_T>
</P2P>
```
**Login**

The client can now login over UDP


## Dissconnect

After a message ID 02 (logout) the client send this

```xml
<P2P>
  <C2D_DISC>
    <cid>82000</cid>
    <did>80</did>
  </C2D_DISC>
</P2P>
```

- **cid**: The connection ID of the client
- **did**: The connection ID of the camera

The camera also send it but with a `D2C_DISC` tag

## Other

Some cameras seem to also send this before login

```xml
<P2P>
  <D2C_CFM>
    <sid>62097899</sid>
    <conn>local</conn>
    <rsp>0</rsp>
    <cid>82000</cid>
    <did>80</did>
    <time_r>0</time_r>
  </D2C_CFM>
</P2P>
```

- **sid**: ID of the camera
- **conn**: Type of connection only observed `local` value
- **rsp**: Unknown always 0
- **cid**: The connection ID of the client
- **did**: The connection ID of the camera
- **time_r**: Should be the time but it is always 0

# UDP Ack

These messages are sent to acknowledge receipt of message

## Header 28 Bytes

- 4 Bytes magic: `20cf872a`
- 4 Bytes Connection ID: This is the connection ID negotiated during UDP Discovery
- 4 Bytes unknown: Always `00000000` for for UDP Ack
- 4 Bytes unknown: Always `00000000` for for UDP Ack
- 4 Bytes Last Packet ID: Last received packet id
- 4 Bytes Unknown: Observed values `00000000`, `d6010000`, `d7160000` `09e00000` **NEEDS INFO**
- Payload Size
- Paload

**To Investigate:**
  - Why does the unknown byte change. It starts at zero and remains that way
    for a second. Then seems to change and remain at the new value for another second.
    This may be some metric such as average ttl for the last second, or some calculation
    of the jitter

Here's an example of a UDP Ack payload (size 203 bytes)

```hex
0000   00 01 01 01 01 01 01 01 01 01 01 01 00 01 01 01
0010   01 01 01 01 01 01 01 01 01 01 01 01 01 01 01 01
0020   01 01 01 01 00 01 01 01 01 01 01 01 01 01 01 01
0030   01 01 01 01 01 01 01 01 01 01 01 01 01 01 01 01
0040   01 01 01 01 01 01 01 01 01 01 01 01 01 01 01 01
0050   01 01 01 01 01 01 01 01 01 01 01 01 01 01 01 01
0060   01 01 01 01 01 01 00 01 01 01 01 01 01 01 01 01
0070   01 01 01 01 01 01 01 01 01 01 01 01 01 01 01 01
0080   01 01 01 01 01 01 01 01 01 01 00 01 01 01 01 01
0090   01 01 01 01 01 01 01 01 01 01 01 01 01 01 01 01
00a0   01 01 01 01 01 01 01 01 01 01 01 01 01 01 01 01
00b0   01 01 01 01 01 01 01 01 01 01 01 01 01 01 01 01
00c0   01 01 01 01 01 01 01 01 01 01 01 01
```

The payload is a truth table. If the packet ID in header is 50
then the first byte corresponds to packet id 51, the second 52
etc etc. If the byte if `00` then the camera will resend that packet

If this payload is allowed to grow to ~ 205 bytes the camera sends a
disconnect request and drops the connection



# UDP Data

UDP data packets contain the BC packets with an extra header.

Whenever a packet is received a UDP Ack is sent. If the sender of the packet
does not get the Ack within 1000ms it resends the packet

## Header 20 Bytes

- 4 Bytes magic: `10cf872a`
- 4 Bytes Connection ID: This is the connection ID negotiated during UDP Discovery
- 4 Bytes unknown: Always `00000000` for for UDP Data
- 4 Bytes Packet ID: The ID of the packet mono-atomically increases for each new packet
- 4 Bytes Payload Size: Size of UDP payload in bytes

## Payload

The payload is a standard Bc Packet
