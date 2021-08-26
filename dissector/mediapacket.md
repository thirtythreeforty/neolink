# Media Packets

This file attempts to document the encapsulated stream that makes up the binary
data of message ID 3. This stream represents the video and audio data.

The are six types of media packets

- Info V1

- Info V2

- I Frame

- P Frame

- AAC

- ADPCM

The I and P frames come in two formats H264 and H265.

The ADPCM is DVI-4 encoded with an extra header that represents the block size.

## Magic

Each packet can be distinguished with the following magic bytes

- Info V1: 0x31, 0x30, 0x30, 0x31
- Info V2: 0x31, 0x30, 0x30, 0x32
- I Frame: 0x30, 0x30, 0x64, 0x63
- P Frame: 0x30, 0x31, 0x64, 0x63
- AAC:  0x30, 0x35, 0x77, 0x62
- ADPCM: 0x30, 0x31, 0x77, 0x62


## Headers

The full headers for each of these are as follows:

Most of this information comes from the work of @twisteddx at his
[site](https://www.wasteofcash.com/BCConvert/BC_fileformat.txt)

- Info V1/V2:

  - 4 bytes magic
  - 4 bytes data size of header itself
  - 4 bytes video width
  - 4 bytes video height
  - 1 byte unknown. Known values 00/01
  - 1 byte Frames per second (Reolink=FPS, Swann=Appears to be the index value of the FPS setting)
  - 1 byte Start UTC year since 1900
  - 1 byte Start UTC month
  - 1 byte Start UTC date
  - 1 byte Start UTC hour
  - 1 byte Start UTC minute
  - 1 byte Start UTC seconds
  - 1 byte End UTC year since 1900
  - 1 byte End UTC month
  - 1 byte End UTC date
  - 1 byte End UTC hour
  - 1 byte End UTC minute
  - 1 byte End UTC seconds
  -  2 bytes reserved

- I Frame

  - 4 bytes magic
  - 4 bytes video type (ASCII text of either H264 or H265)
  - 4 bytes data size of payload after header
  - 4 bytes unknown. NVR channel count? Known values 1-00/08 2-00 3-00 4-00
  - 4 bytes Microseconds
  - 4 bytes unknown. Known values 1-00/23/5A 2-00 3-00 4-00
  - 4 bytes POSIX time_t 32bit UTC time (seconds since 00:00:00 Jan 1 1970)
  - 4 bytes unknown. Known values 1-00/06/29 2-00/01 3-00/C3 4-00

- P Frame
  - 4 bytes magic
  - 4 bytes video type (eg H264 or H265)
  - 4 bytes data size of payload after header
  - 4 bytes unknown. Known values 1-00 2-00 3-00 4-00
  - 4 bytes Microseconds
  - 4 bytes unknown. Known values 1-00/5A 2-00 3-00 4-00

- AAC

  - 4 bytes magic
  - 2 bytes data size of payload after header
  - 2 bytes data size of payload after header (Same as previous)

- ADPCM

  - 4 bytes magic
  - 2 bytes data size of payload after header
  - 2 bytes data size of payload after header

### ADPCM

After the ADPCM header is another header of the form

- 2 bytes Magic either 0x00 0x01 or 0x00 0x7a
- 2 Bytes DVI-4 Block size divided in bytes.

After this header the adpcm DVI-4 data follows should be 4+Block size bytes.

## Processing

The data in ID 3 messages represents an encapsulated stream. BC messages may
terminate mid media packet messages. Clients should create a buffer of the
BC ID 3 messages then read the media packet magic and expected length. Once
length is known they should wait for more BC message ID 3 packets until a
complete media packet is received before processing it.
