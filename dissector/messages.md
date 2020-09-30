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
      Body is hash of user and password and then a lot of zero pads

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

- 11-30 Not observed

- 31 Unknown

  - Client

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 1f 00 00 00  |  00 00 00 00   |    00 00 00 05    |       00        |   00    |     14 64     |  00 00 00 00  |

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 1f 00 00 00  |  00 00 00 00   |    00 00 00 05    |       c8        |   00    |     00 00     |  00 00 00 00  |

  - **Notes:** Neither client nor camera have any body. Just header
  This makes it difficult to figure out what this message does with
  out active experimentation

- 32 Not observed

- 33 Motion Detection

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 21 00 00 00  |  f0 00 00 00   |    00 00 00 05    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
    <AlarmEventList version="1.1">
    <AlarmEvent version="1.1">
    <channelId>0</channelId>
    <status>MD</status> <!-- "MD" for motion "none" for none -->
    <recording>0</recording>
    <timeStamp>0</timeStamp>
    </AlarmEvent>
    </AlarmEventList>
    </body>
    ```

  - **Notes:** There is no message from the client observed

- 34-57 Not observed

- 58 User capabilities

  - Client

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 3a 00 00 00  |  6b 00 00 00   |    00 00 00 03    |       00        |   00    |     14 64     |  6b 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <Extension version="1.1">
    <userName>...</userName> <!-- Plain text username -->
    </Extension>
    ```

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 3a 00 00 00  |  a4 03 00 00   |    00 00 00 03    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
    <AbilitySuppport version="1.1">
    <userName></userName>
    <system>1</system>
    <streaming>1</streaming>
    <record>1</record>
    <network>1</network>
    <PTZ>1</PTZ>
    <IO>0</IO>
    <alarm>1</alarm>
    <image>1</image>
    <video>1</video>
    <audio>1</audio>
    <security>1</security>
    <replay>1</replay>
    <disk>1</disk>
    </AbilitySuppport>
    <UserList version="1.1">
    <User>
    <userId>0</userId>
    <userName>...</userName> <!-- Plain text user names and passwords -->
    <password>...</password> <!-- For all accounts even admins -->
    <userLevel>1</userLevel> <!-- 1 for admin 0 for normal -->
    <loginState>0</loginState>
    <userSetState>none</userSetState>
    </User>
    <User>
    <userId>0</userId>
    <userName>...</userName> <!-- Yes this is horrible security -->
    <password>...</password>
    <userLevel>0</userLevel>
    <loginState>0</loginState>
    <userSetState>none</userSetState>
    </User>
    <User>
    <userId>0</userId>
    <userName>...</userName>
    <password>...</password>
    <userLevel>1</userLevel>
    <loginState>1</loginState>
    <userSetState>none</userSetState>
    </User>
    </UserList>
    </body>
    ```

- 59-77 Not observed

- 78 Stream brightness/contrast etc

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 4e 00 00 00  |  d3 00 00 00   |    08 db 9c 00    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - Body
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

  - **Notes:** No message from client

- 79 PTZ Details

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 4f 00 00 00  |  3b 01 00 00   |    08 db 9c 00    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
    <Serial version="1.1">
    <channelId>0</channelId>
    <baudRate>9600</baudRate>
    <dataBit>CS8</dataBit>
    <stopBit>1</stopBit>
    <parity>none</parity>
    <flowControl>none</flowControl>
    <controlProtocol>PELCO_D</controlProtocol>
    <controlAddress>1</controlAddress>
    </Serial>
    </body>
    ```

- 80 Camera Model

  - Client

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 50 00 00 00  |  00 00 00 00   |    00 00 00 08    |       00        |   00    |     14 64     |  00 00 00 00  |

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 50 00 00 00  |  f0 01 00 00   |    00 00 00 08    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
    <VersionInfo version="1.1">
    <name>Cammy02</name>
    <type>E1</type>
    <serialNumber>00000000000000</serialNumber>
    <buildDay>build 19110800</buildDay>
    <hardwareVersion>IPC_517SD5</hardwareVersion>
    <cfgVersion>v2.0.0.0</cfgVersion>
    <firmwareVersion>v2.0.0.587_19110800</firmwareVersion>
    <detail>IPC_51716M110000000100000</detail>
    <IEClient>IEClient</IEClient>
    <pakSuffix>pak</pakSuffix>
    <helpVersion>blackPointsLevel=0</helpVersion>
    </VersionInfo>
    </body>
    ```

- 81-101 Not observed

- 102 HDD Info

  - Client

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 66 00 00 00  |  00 00 00 00   |    00 00 00 07    |       00        |   00    |     14 64     |  00 00 00 00  |

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 66 00 00 00  |  55 00 00 00   |    00 00 00 07    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
    <HddInfoList version="1.1" />
    </body>
    ```

- 102-103 Not observed

- 104 Camera DateTime

  - Client

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 68 00 00 00  |  00 00 00 00   |    00 00 00 0a    |       00        |   00    |     14 64     |  00 00 00 00  |

    - Body

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 68 00 00 00  |  a4 01 00 00   |    00 00 00 0a    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
    <SystemGeneral version="1.1">
    <timeZone>-25200</timeZone>
    <osdFormat>DMY</osdFormat>
    <year>2020</year>
    <month>9</month>
    <day>29</day>
    <hour>8</hour>
    <minute>10</minute>
    <second>34</second>
    <deviceId>0</deviceId>
    <timeFormat>0</timeFormat>
    <language>English</language>
    <deviceName>Cammy02</deviceName>
    </SystemGeneral>
    <Norm version="1.1">
    <norm>NTSC</norm>
    </Norm>
    </body>
    ```

- 105-114 Not observed

- 115 Camera Wifi Signal Strength

  - Client

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 73 00 00 00  |  00 00 00 00   |    00 00 00 0c    |       00        |   00    |     14 64     |  00 00 00 00  |

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 73 00 00 00  |  75 00 00 00   |    00 00 00 0c    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
    <WifiSignal version="1.1">
    <signal>-40</signal>
    </WifiSignal>
    </body>
    ```
  - **Notes:** Client polls this message repeatedly by resending the request
  header and getting back a new reply from the camera

- 116-132 Not observed

- 133 RF Alarm

  - Client

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 85 00 00 00  |  00 00 00 00   |    00 00 00 06    |       00        |   00    |     14 64     |  00 00 00 00  |

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 85 00 00 00  |  7f 00 00 00   |    00 00 00 06    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
    <RfAlarm version="1.1">
    <enable>1</enable>
    <type>none</type>
    </RfAlarm>
    </body>
    ```

  - **Notes:** I think this is a setting for should it wake up wifi and transmit
  when motion detected or not. The reolink website says this applies to battery
  camera models.

- 134-145 Not observed

- 146 Stream Info

  - Client

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 92 00 00 00  |  00 00 00 00   |    00 00 00 04    |       00        |   00    |     14 64     |  00 00 00 00  |

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 92 00 00 00  |  fc 02 00 00   |    00 00 00 04    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
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

- 147-150 Not observed

- 151 User Ability Info

  - Client

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 97 00 00 00  |  a7 00 00 00   |    00 00 00 02    |       00        |   00    |     14 64     |  a7 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <Extension version="1.1">
    <userName>...</userName> <!-- Plain text username -->
    <token>system, network, alarm, record, video, image</token>
    </Extension>
    ```
  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | 97 00 00 00  |  ac 03 00 00   |    00 00 00 02    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
    <AbilityInfo version="1.1">
    <userName>...</userName> <!-- Plain text username -->
    <system>
    <subModule>
    <abilityValue>general_rw, norm_rw, version_ro, uid_ro, autoReboot_rw, restore_rw, reboot_rw, shutdown_rw, dst_rw, log_ro, performance_ro, upgrade_rw, export_rw, import_rw, bootPwd_rw</abilityValue>
    </subModule>
    </system>
    <network>
    <subModule>
    <abilityValue>port_rw, dns_rw, email_rw, ipFilter_rw, localLink_rw, pppoe_rw, upnp_rw, wifi_rw, ntp_rw, netStatus_rw, ptop_rw, autontp_rw</abilityValue>
    </subModule>
    </network>
    <alarm>
    <subModule>
    <channelId>0</channelId>
    <abilityValue>motion_rw</abilityValue>
    </subModule>
    </alarm>
    <image>
    <subModule>
    <channelId>0</channelId>
    <abilityValue>ispBasic_rw, ispAdvance_rw, ledState_rw</abilityValue>
    </subModule>
    </image>
    <video>
    <subModule>
    <channelId>0</channelId>
    <abilityValue>osdName_rw, osdTime_rw, shelter_rw</abilityValue>
    </subModule>
    </video>
    </AbilityInfo>
    </body>
    ```

- 152-189 Not observed

- 190 PTZ Preset

  - Client

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | be 00 00 00  |  68 00 00 00   |    00 00 00 0d    |       00        |   00    |     14 64     |  68 00 00 00  |

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
    | f0 de bc 0a  | be 00 00 00  |  86 00 00 00   |    00 00 00 0d    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
    <PtzPreset version="1.1">
    <channelId>0</channelId>
    <presetList />
    </PtzPreset>
    </body>
    ```

- 191 Not observed

- 192 Unknown

  - Client

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | c0 00 00 00  |  00 00 00 00   |    00 00 00 05    |       00        |   00    |     14 64     |  00 00 00 00  |

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | c0 00 00 00  |  00 00 00 00   |    00 00 00 05    |       c8        |   00    |     00 00     |  00 00 00 00  |

    - **Notes:** Header only reply so cannot determine what this does without
    active analysis

- 193-198 Not observed

- 199 Camera Ability Info

  - Client

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | c7 00 00 00  |  00 00 00 00   |    00 00 00 02    |       00        |   00    |     14 64     |  00 00 00 00  |

  - Camera

    - Header

    |    magic     |  message id  | message length | encryption offset | Encryption flag | Unknown | message class | binary length |
    |--------------|--------------|----------------|-------------------|-----------------|---------|---------------|---------------|
    | f0 de bc 0a  | c7 00 00 00  |  f6 05 00 00   |    00 00 00 02    |       c8        |   00    |     00 00     |  00 00 00 00  |


    - Body
    ```xml
    <?xml version="1.0" encoding="UTF-8" ?>
    <body>
    <Support version="1.1">
    <IOInputPortNum>0</IOInputPortNum>
    <IOOutputPortNum>0</IOOutputPortNum>
    <diskNum>0</diskNum>
    <channelNum>1</channelNum>
    <audioNum>1</audioNum>
    <ptzMode>pt</ptzMode>
    <ptzCfg>0</ptzCfg>
    <B485>0</B485>
    <autoUpdate>0</autoUpdate>
    <pushAlarm>1</pushAlarm>
    <ftp>0</ftp>
    <ftpTest>1</ftpTest>
    <email>1</email>
    <wifi>5</wifi>
    <record>0</record>
    <wifiTest>1</wifiTest>
    <rtsp>0</rtsp>
    <onvif>0</onvif>
    <audioTalk>1</audioTalk>
    <rfVersion>0</rfVersion>
    <rtmp>0</rtmp>
    <noExternStream>1</noExternStream>
    <timeFormat>1</timeFormat>
    <ddnsVersion>1</ddnsVersion>
    <emailVersion>3</emailVersion>
    <pushVersion>1</pushVersion>
    <pushType>1</pushType>
    <audioAlarm>1</audioAlarm>
    <apMode>0</apMode>
    <cloudVersion>30</cloudVersion>
    <replayVersion>1</replayVersion>
    <mobComVersion>0</mobComVersion>
    <syncTime>1</syncTime>
    <netPort>1</netPort>
    <videoStandard>0</videoStandard>
    <smartHome>
    <version>1</version>
    <item>
    <name>googleHome</name>
    <ver>1</ver>
    </item>
    <item>
    <name>amazonAlexa</name>
    <ver>1</ver>
    </item>
    </smartHome>
    <item>
    <chnID>0</chnID>
    <ptzType>3</ptzType>
    <ptzPreset>0</ptzPreset>
    <ptzPatrol>0</ptzPatrol>
    <ptzTattern>0</ptzTattern>
    <ptzControl>0</ptzControl>
    <rfCfg>0</rfCfg>
    <noAudio>0</noAudio>
    <autoFocus>0</autoFocus>
    <videoClip>0</videoClip>
    <battery>0</battery>
    <ispCfg>0</ispCfg>
    <osdCfg>1</osdCfg>
    <batAnalysis>0</batAnalysis>
    <dynamicReso>0</dynamicReso>
    <audioVersion>15</audioVersion>
    <ledCtrl>1</ledCtrl>
    <motion>1</motion>
    </item>
    </Support>
    </body>
    ```

- 200+ Not observed


---
# Ancillary

I used these regex replace to make the header tables in this document
from the wireshark hex dump. It may be useful for others working on this.

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
