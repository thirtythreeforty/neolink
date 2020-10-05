# BC Protocol

This is an attempt to document the BC protocol. It is far from complete
but should serve as a good basis for those wishing to develop apps for
BC cameras.

## Messages

Each message has the general format:

- Header: 20-24 bytes

- Message Body

### Header

The header has the format:

|    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class |
|--------------|--------------|----------------|-------------------|-----------------|---------|---------------|
| f0 de bc 0a  | 01 00 00 00  |  2c 07 00 00   |    00 00 00 01    |       01        |   dc    |     14 65     |

Or

|    Magic     |  Message ID  | Message Length | Encryption Offset | Encryption Flag | Unknown | Message Class | Binary Offset |
|--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
| f0 de bc 0a  | 01 00 00 00  |  28 01 00 00   |    00 00 00 01    |       00        |   00    |     14 64     |  00 00 00 00  |


- Magic 4 bytes
- ID 4 bytes
- Message length 4 bytes
- Encryption offset 4 bytes
- Encryption flag (not always honoured by clients) 1 byte
- Unknown 1 byte (always zero)
- Binary offset 4 bytes (Presence depend on message class)

#### Magic

The magic bytes for BC messages is always `f0 de bc 0a`. When receiving packet
this should be used to quickly discard invalid packets.

#### Message ID

Each function in BC has its own message ID. For example login is 1, video data
is 3, motion detection is 33.

For a more complete list please see the [messages doc](dissector/messages.md)

#### Message length

The message length contains the full length of the data to follow the header
this includes both the XML and binary parts.

#### Encryption offset

The encryption offset is used as part of the decoding process. It is combined
with the key to decrypt the data.

Here is an example decrypter in rust

```rust
const XML_KEY: [u8; 8] = [0x1F, 0x2D, 0x3C, 0x4B, 0x5A, 0x69, 0x78, 0xFF];

pub fn crypt(offset: u32, buf: &[u8]) -> Vec<u8> {
    let key_iter = XML_KEY.iter().cycle().skip(offset as usize % 8);
    key_iter
        .zip(buf)
        .map(|(key, i)| *i ^ key ^ (offset as u8))
        .collect()
}
```

In short the key is offset by the encryption offset in the header. Then each
encrypted byte is paired with the offseted keys bytes (looping the offseted
key as necessary). Then each byte is XORed with the paired key byte and the
offset.

The key is the same for all cameras.

Older cameras do not use encryption and all messages are sent as plain text.

#### Encryption Flag

This flag is 0 for unencrypted or non zero for encrypted. This flag seems
only to be valid during the login step, and seems to be arbitrary otherwise.

Clients should decide to use encryption or not based on this byte that the
camera send during login. Then should use that same setting for all further
messages with the camera.

#### Unknown

The purpose of this byte is unknown but is always zero. If you figure out
what it does please submit a PR.

#### Message class

The message class determines the length of the header. The following classes
and header lengths are known.

- 0x6514: "legacy" 20 bytes

- 0x6614: "modern" 20 bytes

- 0x6414: "modern" 24 bytes

- 0x0000: "modern" 24 bytes

#### Binary offset

For messages that contain the binary offset field this represents where to start
the binary part of the message. The total length of XML + binary is equal
to the message length in the header so Binary Offset is
total_length - this_offset. Where as this field also represents the end of the
XML part of the message.

Some messages seems to not set this field correctly. For example message ID 1
sets this to zero meaning that all data in the message is binary, however
it contains XML. This best way to tell is this is binary or XML is to check the
message ID and determine from there. Alternatively you could look for `<xml?` at
the beginning of the message (although may need to decrypt first for this to
work)

# Login

For message details see the [docs](dissector/messages.md)

Clients should login by

- Send legacy login message
    - User and pass MD5'ed
    - Capped at 32 bytes with a null terminator
    - Bytes 32 is always zero so only first 31 bytes are compared

- Receive modern upgrade message with nonce in XML

- Send modern login:
  - User and pass concatenated with the nonce
  - Send MD5'ed user and password

- Receive reply with device info

# Starting Video

Video is requested and received with message ID 3.

Video is requested with an XML of the following format:

```xml
<?xml version="1.0" encoding="UTF-8" ?>
<body>
<Preview version="1.1">
<channelId>0</channelId>
<handle>0</handle>
<streamType>mainStream</streamType>
</Preview>
</body>
```

streamType can be either

- `mainStream` in which case it will be HD
- `subStream` in which case it will be SD


channelId is part of the NVR when multiple cameras use the same IP. In this
case each camera has its own channelId.

Not sure what handle does.


The reply is first a message with the following xml

```xml
<?xml version="1.0" encoding="UTF-8" ?>
<Extension version="1.1">
<binaryData>1</binaryData>
</Extension>
```

After which all message bodies of type id 3 are binary.

The binary represents a stream of data. It can be interrupted by packet
boundaries. Clients should create a buffer and pop bytes for processing when
complete media packets are received. Media packets descriptions can be found in
the [docs](dissector/mediapacket.md)

# Other Function

Other data can be received from the camera by sending the appropriate header to
the camera. For example sending the header for ID 78

|    Magic     |  Message ID  | Message Length | Encryption Offset | Encryption Flag | Unknown | Message Class | Binary Offset |
|--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
| f0 de bc 0a  | 4e 00 00 00  |  d3 00 00 00   |    08 db 9c 00    |       c8        |   00    |     00 00     |  00 00 00 00  |

The camera will reply with an xml with brightness and contrast

```xml
<?xml version="1.0" encoding="UTF-8" ?>
<body>
<VideoInput version="1.1">
<channelId>0</channelId>
<bright>128</bright>
<contrast>128</contrast>
<saturation>128</saturation>
<hue>128</hue>
</VideoInput>
</body>
```

Some message IDs also require input along with the request header. For example

ID 151 which is the users ability info requires the header

|    Magic     |  Message ID  | Message Length | Encryption Offset | Encryption Flag | Unknown | Message Class | Binary Offset |
|--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
| f0 de bc 0a  | 97 00 00 00  |  a7 00 00 00   |    00 00 00 02    |       00        |   00    |     14 64     |  a7 00 00 00  |

and the body of

```xml
<?xml version="1.0" encoding="UTF-8" ?>
<Extension version="1.1">
<userName>...</userName> <!-- Plain text username -->
<token>system, network, alarm, record, video, image</token>
</Extension>
```

Which contains the plain text of the username of interest and the tokens for
abilities you want to know about.

Details of expected formats should be found from the
[docs](dissector/messages.md)
