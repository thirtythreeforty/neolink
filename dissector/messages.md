# BC Messages
---

This is an attempt to document the BC messages. It is subject to change
and some aspects of it may not be correct. Please feel free to submit
a PR to improve it.

- 1 Login Legacy

  - Client

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|
    | f0 de bc 0a  | 01 00 00 00  |  2c 07 00 00   |    00 00 00 01    |       01        |   dc    |     14 65     |

    - Body
    ```hex
    0000   31 38 35 31 42 34 43 36 34 31 36 31 36 34 34 33
    0010   36 42 45 32 46 44 31 38 41 34 32 31 37 31 31 00
    0020   38 34 36 30 31 35 41 42 36 45 39 38 30 30 43 31
    0030   34 44 30 30 42 43 41 37 31 32 44 42 38 39 44 00
    0040   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0050   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0060   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0070   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0080   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0090   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    00a0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    00b0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    00c0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    00d0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    00e0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    00f0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0100   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0110   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0120   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0130   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0140   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0150   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0160   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0170   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0180   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0190   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    01a0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    01b0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    01c0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    01d0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    01e0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    01f0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0200   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0210   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0220   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0230   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0240   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0250   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0260   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0270   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0280   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0290   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    02a0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    02b0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    02c0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    02d0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    02e0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    02f0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0300   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0310   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0320   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0330   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0340   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0350   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0360   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0370   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0380   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0390   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    03a0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    03b0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    03c0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    03d0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    03e0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    03f0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0400   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0410   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0420   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0430   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0440   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0450   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0460   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0470   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0480   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0490   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    04a0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    04b0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    04c0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    04d0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    04e0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    04f0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0500   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0510   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0520   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0530   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0540   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0550   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0560   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0570   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0580   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0590   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    05a0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    05b0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    05c0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    05d0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    05e0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    05f0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0600   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0610   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0620   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0630   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0640   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0650   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0660   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0670   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0680   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0690   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    06a0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    06b0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    06c0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    06d0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    06e0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    06f0   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0700   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0710   00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    0720   00 00 00 00 00 00 00 00 00 00 00 00
    ```

    - **Notes:** Body is hash of user and password and then a lot of zero pads
  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|
    | f0 de bc 0a  | 01 00 00 00  |  91 00 00 00   |    00 00 00 01    |       01        |   dd    |     14 66     |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
    <Encryption version="1.1">
    <type>md5</type>
    <nonce>13BCECE33DA453DB</nonce>
    </Encryption>
    </body>
    ```

    - **Notes:** Sends back a NOONCE used for the modern login message. This is
    effectively an upgrade request to use the modern xml style over legacy.
    A legacy camera likely replies differently but I don't have one to test on.

- 1 Login Modern

  - Client
    - Header
    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 01 00 00 00  |  28 01 00 00   |    00 00 00 01    |       00        |   00    |     14 64     |  00 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
    <LoginUser version="1.1">
    <userName>...</userName> <!-- Hash of username with noonce -->
    <password>...</password> <!-- Hash of password with noonce -->
    <userVer>1</userVer>
    </LoginUser>
    <LoginNet version="1.1">
    <type>LAN</type>
    <udpPort>0</udpPort>
    </LoginNet>
    </body>
    ```

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 01 00 00 00  |  2e 06 00 00   |    00 00 00 01    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
    <DeviceInfo version="1.1">
    <firmVersion>00000000000000</firmVersion>
    <IOInputPortNum>0</IOInputPortNum>
    <IOOutputPortNum>0</IOOutputPortNum>
    <diskNum>0</diskNum>
    <type>wifi_solo_ipc</type>
    <channelNum>1</channelNum>
    <audioNum>1</audioNum>
    <ipChannel>0</ipChannel>
    <analogChnNum>1</analogChnNum>
    <resolution>
    <resolutionName>2304*1296</resolutionName>
    <width>2304</width>
    <height>1296</height>
    </resolution>
    <language>English</language>
    <sdCard>1</sdCard>
    <ptzMode>pt</ptzMode>
    <typeInfo>IPC</typeInfo>
    <softVer>33555019</softVer>
    <hardVer>0</hardVer>
    <panelVer>0</panelVer>
    <hdChannel1>0</hdChannel1>
    <hdChannel2>0</hdChannel2>
    <hdChannel3>0</hdChannel3>
    <hdChannel4>0</hdChannel4>
    <norm>NTSC</norm>
    <osdFormat>DMY</osdFormat>
    <B485>0</B485>
    <supportAutoUpdate>0</supportAutoUpdate>
    <userVer>1</userVer>
    </DeviceInfo>
    <StreamInfoList version="1.1">
    <StreamInfo>
    <channelBits>1</channelBits>
    <encodeTable>
    <type>mainStream</type>
    <resolution>
    <width>2304</width>
    <height>1296</height>
    </resolution>
    <defaultFramerate>15</defaultFramerate>
    <defaultBitrate>2560</defaultBitrate>
    <framerateTable>15,12,10,8,6,4,2</framerateTable>
    <bitrateTable>1024,1536,2048,2560,3072</bitrateTable>
    </encodeTable>
    <encodeTable>
    <type>subStream</type>
    <resolution>
    <width>896</width>
    <height>512</height>
    </resolution>
    <defaultFramerate>15</defaultFramerate>
    <defaultBitrate>512</defaultBitrate>
    <framerateTable>15,12,10,8,6,4,2</framerateTable>
    <bitrateTable>128,256,384,512,768,1024</bitrateTable>
    </encodeTable>
    </StreamInfo>
    </StreamInfoList>
    </body>
    ```
- 2 Not observed

- 3 Stream

  - Client

    - Header
    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 03 00 00 00  |  aa 00 00 00   |    00 00 00 09    |       00        |   00    |     14 64     |  00 00 00 00  |

    - Body
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

    - **Notes:** This requests the camera to send this stream

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 03 00 00 00  |  8a 00 00 00   |    00 00 00 09    |       c8        |   00    |     00 00     |  6a 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <Extension version="1.1">
    <binaryData>1</binaryData>
    </Extension>
    ```

    - **Notes:** Camera then send the stream as a binary payload in all
    following messages of id 3

  - Camera Stream Binary

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 03 00 00 00  |  e8 5e 00 00   |    00 00 00 09    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - Body

      Body is binary. This binary represents an embedded stream which should
      be detailed elsewhere.

- 4-9 Not observed

- 10 Audio back-channel

  - Client

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 0a 00 00 00  |  68 00 00 00   |    00 00 00 0b    |       00        |   00    |     14 64     |  68 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <Extension version="1.1">
    <channelId>0</channelId>
    </Extension>
    ```

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 0a 00 00 00  |  f7 01 00 00   |    00 00 00 0b    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
    <TalkAbility version="1.1">
    <duplexList>
    <duplex>FDX</duplex>
    </duplexList>
    <audioStreamModeList>
    <audioStreamMode>followVideoStream</audioStreamMode>
    </audioStreamModeList>
    <audioConfigList>
    <audioConfig>
    <priority>0</priority>
    <audioType>adpcm</audioType>
    <sampleRate>16000</sampleRate>
    <samplePrecision>16</samplePrecision>
    <lengthPerEncoder>1024</lengthPerEncoder>
    <soundTrack>mono</soundTrack>
    </audioConfig>
    </audioConfigList>
    </TalkAbility>
    </body>
    ```




---
# Ancillary

I used these regex replace to make the header tables from the wireshark hex dump.
It may be useful for others working on it.

```
^0000   ([a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9] )([a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9] )([a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9] )([a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9])\n0010   ([a-f0-9][a-f0-9] )([a-f0-9][a-f0-9] )([a-f0-9][a-f0-9] [a-f0-9][a-f0-9])
```

```
|    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class |
|--------------|--------------|----------------|-------------------|-----------------|---------|---------------|
| $1 | $2 |  $3  |    $4    |       $5       |   $6   |     $7     |
```

```
^0000   ([a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9] )([a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9] )([a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9] )([a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9])\n0010   ([a-f0-9][a-f0-9] )([a-f0-9][a-f0-9] )([a-f0-9][a-f0-9] [a-f0-9][a-f0-9] )([a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9] [a-f0-9][a-f0-9])
```

```
|    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
|--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
| $1 | $2 |  $3  |    $4    |       $5       |   $6   |     $7    |  $8  |
```
