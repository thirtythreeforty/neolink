-- This is a Wireshark dissector for the Baichuan/Reolink proprietary IP camera protocol.
-- Copy/symlink it into ~/.local/lib/wireshark/plugins/ and restart Wireshark; it should
-- automatically attempt to decode TCP connections on port 9000.

local bc_protocol = Proto("Baichuan",  "Baichuan/Reolink IP Camera Protocol")

local magic_bytes = ProtoField.int32("baichuan.magic", "magic", base.DEC)
local message_id =  ProtoField.int32("baichuan.msg_id", "messageId", base.DEC)
local message_len = ProtoField.int32("baichuan.msg_len", "messageLen", base.DEC)
local xml_enc_offset = ProtoField.int8("baichuan.xml_encryption_offset", "xmlEncryptionOffset", base.DEC)
local encrypt_xml = ProtoField.bool("baichuan.encrypt_xml", "encrypt_xml", base.NONE)
local channel_id =  ProtoField.int8("baichuan.channel_id", "channel_id", base.DEC)
local stream_id = ProtoField.int8("baichuan.stream_id", "streamID", base.DEC)
local unknown = ProtoField.int8("baichuan.unknown", "unknown", base.DEC)
local msg_handle = ProtoField.int8("baichuan.message_handle", "messageHandle", base.DEC)
local status_code = ProtoField.int16("baichuan.status_code", "status_code", base.DEC)
local message_class = ProtoField.int32("baichuan.msg_class", "messageClass", base.DEC)
local f_bin_offset = ProtoField.int32("baichuan.bin_offset", "binOffset", base.DEC)
local username = ProtoField.string("baichuan.username", "username", base.ASCII)
local password = ProtoField.string("baichuan.password", "password", base.ASCII)

-- UDP Related content
local udp_magic = ProtoField.int32("baichuan.udp_magic", "udp_magic", base.DEC)
local udp_type = ProtoField.int8("baichuan.udp_type", "udp_type", base.DEC)
local udp_message_id = ProtoField.int8("baichuan.udp_message_id", "udp_message_id", base.DEC)
local udp_connection_id = ProtoField.int32("baichuan.udp_connection_id", "udp_connection_id", base.DEC)
local udp_unknown = ProtoField.int32("baichuan.udp_unknown", "udp_unknown", base.DEC)
local udp_tid = ProtoField.int32("baichuan.udp_tid", "udp_tid", base.DEC)
local udp_checksum = ProtoField.int32("baichuan.udp_checksum", "udp_checksum", base.DEC)
local udp_packet_count = ProtoField.int32("baichuan.udp_packet_count", "udp_packet_count", base.DEC)
local udp_last_ack_packet = ProtoField.int32("baichuan.udp_last_ack_packet", "udp_last_ack_packet", base.DEC)
local udp_ack_payload_size = ProtoField.int32("baichuan.udp_ack_payload_size", "ack_payload_size", base.DEC)
local udp_size = ProtoField.int32("baichuan.udp_size", "udp_size", base.DEC)

bc_protocol.fields = {
  magic_bytes,
  message_id,
  message_len,
  xml_enc_offset,
  channel_id,
  stream_id,
  unknown,
  msg_handle,
  encrypt_xml,
  status_code,
  message_class,
  f_bin_offset,
  username,
  password,
  udp_magic,
  udp_type,
  udp_message_id,
  udp_connection_id,
  udp_unknown,
  udp_tid,
  udp_checksum,
  udp_packet_count,
  udp_last_ack_packet,
  udp_ack_payload_size,
  udp_size,
}

local message_types = {
  [1]="login", -- <Encryption> <LoginUser>/<LoginNet> <DeviceInfo>/<StreamInfoList>
  [2]="logout",
  [3]="<Preview> (video)",
  [4]="<Preview> (stop)",
  [5]="<FileInfoList> (replay)",
  [7]="<FileInfoList> (stop)",
  [8]="<FileInfoList> (DL Video)",
  [10]="<TalkAbility>",
  [13]="<FileInfoList> (download)",
  [14]="<FileInfoList>",
  [15]="<FileInfoList>",
  [16]="<FileInfoList>",
  [18]="<PtzControl>",
  [23]="Reboot",
  [25]="<VideoInput> (write)",
  [26]="<VideoInput>", -- <InputAdvanceCfg>
  [31]="Start Motion Alarm",
  [33]="<AlarmEventList>",
  [36]="<ServerPort> (write)",
  [37]="<ServerPort>", -- <HttpPort>/<RtspPort>/<OnvifPort>/<HttpsPort>/<RtmpPort>
  [38]="<Ntp>",
  [39]="<Ntp> (write)",
  [40]="<Ddns>",
  [41]="<Ddns> (write)",
  [42]="<Email>",
  [43]="<Email> (write)",
  [44]="<OsdChannelName>", -- <OsdDatetime>
  [45]="<OsdChannelName> (write)",
  [46]="<MD>",
  [47]="<MD> (write)",
  [50]="<VideoLoss>",
  [51]="<VideoLoss> (write)",
  [52]="<Shelter> (priv mask)",
  [53]="<Shelter> (write)",
  [54]="<RecordCfg>",
  [55]="<RecordCfg> (write)",
  [56]="<Compression>",
  [57]="<Compression> (write)",
  [58]="<AbilitySupport>",  -- <UserList>
  [59]="<UserList> (write)",
  [65]="<ConfigFileInfo> (Export)",
  [66]="<ConfigFileInfo> (Import)",
  [67]="<ConfigFileInfo> (FW Upgrade)",
  [68]="<Ftp>",
  [69]="<Ftp> (write)",
  [70]="<FtpTask>",
  [71]="<FtpTask> (write)",
  [76]="<Ip>", -- <Dhcp>/<AutoDNS>/<Dns>
  [77]="<Ip> (write)",
  [78]="<VideoInput> (IPC desc)",
  [79]="<Serial> (ptz)",
  [80]="<VersionInfo>",
  [81]="<Record> (schedule)",
  [82]="<Record> (write)",
  [83]="<HandleException>",
  [84]="<HandleException> (write)",
  [91]="<DisplayOutput>",
  [92]="<DisplayOutput> (write)",
  [93]="<LinkType>",
  [97]="<Upnp>",
  [98]="<Upnp> (write)",
  [99]="<Restore> (factory default)",
  [100]="<AutoReboot> (write)",
  [101]="<AutoReboot>",
  [102]="<HDDInfoList>",
  [103]="<HddInitList> (format)",
  [104]="<SystemGeneral>",
  [105]="<SystemGeneral> (write)",
  [106]="<Dst>",
  [107]="<Dst> (write)",
  [108]="<ConfigFileInfo> (log)",
  [114]="<Uid>",
  [115]="<WifiSignal>",
  [120]="<OnlineUserList>",
  [122]="<PerformanceInfo>",
  [123]="<ReplaySeek>",
  [132]="<VideoInput>", -- <InputAdvanceCfg>
  [133]="<RfAlarm>",
  [141]="<Email> (test)",
  [142]="<DayRecords>",
  [145]="<ChannelInfoList>",
  [146]="<StreamInfoList>",
  [151]="<AbilityInfo>",
  [190]="PTZ Preset",
  [194]="<Ftp> (test)",
  [199]="<Support>",
  [208]="<LedState>",
  [209]="<LedState> (write)",
  [210]="<PTOP>",
  [211]="<PTOP> (write)",
  [212]="<rfAlarmCfg>",
  [216]="<EmailTask> (write)",
  [217]="<EmailTask>",
  [218]="<PushTask> (write)",
  [219]="<PushTask>",
  [228]="<Crop>",
  [229]="<Crop> (write)",
  [230]="<cropSnap>",
  [234]="UDP Keep Alive",
  [252]="<BatteryInfoList>",
  [253]="<BatteryInfo>",
  [272]="<findAlarmVideo>",
  [273]="<alarmVideoInfo>",
  [274]="<findAlarmVideo>",
}

local message_classes = {
  [0x6514]="legacy",
  [0x6614]="modern",
  [0x6414]="modern",
  [0x0000]="modern",
}

local header_lengths = {
  [0x6514]=20,
  [0x6614]=20,
  [0x6414]=24,
  [0x0000]=24,
}

local function xml_decrypt(ba, offset)
  local key = "\031\045\060\075\090\105\120\255" -- 1f, 2d, 3c, 4b, 5a, 69, 78 ,ff
  local e = ByteArray.new()
  e:set_size(ba:len())
  for i=0,ba:len() - 1 do
    e:set_index(i, bit32.bxor(bit32.band(offset, 0xFF), bit32.bxor(ba:get_index(i), key:byte(((i + offset) % 8) + 1))))
  end
  return e
end

local function get_header_len(buffer)
  local magic = buffer(0, 4):le_uint()
  if magic ~= 0x0abcdef0 and magic ~= 0x0fedcba0 then
    -- Unknown magic
    return -1 -- No header found
  end
  local header_len = header_lengths[buffer(18, 2):le_uint()]
  return header_len
end

local function get_header(buffer)
  -- bin_offset is either nil (no binary data) or nonzero
  -- TODO: bin_offset is actually stateful!
  local bin_offset = nil
  local return_code = nil
  local encr_xml = nil
  local header_len = header_lengths[buffer(18, 2):le_uint()]
  if header_len == 24 then
    bin_offset = buffer(20, 4):le_uint() -- if NHD-805/806 legacy protocol 30 30 30 30 aka "0000"
    return_code =  buffer(16, 2):le_uint()
  else
    encr_xml = buffer(16, 1):le_uint()
  end
  local msg_type = buffer(4, 4):le_uint()
  local stream_text = "HD (Clear)"
  if buffer(13, 1):le_uint() == 1 then
    stream_text = "SD (Fluent)"
  end
  return {
    magic = buffer(0, 4):le_uint(),
    msg_type = buffer(4, 4):le_uint(),
    msg_type_str = message_types[msg_type] or "unknown",
    msg_len = buffer(8, 4):le_uint(),
    encrypt_xml = encr_xml,
    channel_id = buffer(12, 1):le_uint(),
    enc_offset = buffer(12, 1):le_uint(),
    stream_type = stream_text,
    unknown = buffer(14, 1):le_uint(),
    msg_handle = buffer(15, 1):le_uint(),
    msg_cls = buffer(18, 2):le_uint(),
    status_code = return_code,
    class = message_classes[buffer(18, 2):le_uint()],
    header_len = header_lengths[buffer(18, 2):le_uint()],
    bin_offset = bin_offset,
  }
end

local function process_header(buffer, headers_tree)
  local header_data = get_header(buffer)
  local header = headers_tree:add(bc_protocol, buffer(0, header_data.header_len),
    "Baichuan Message Header, length: " .. header_data.header_len .. ", type " .. header_data.msg_type)
  local stream_text = " HD (Clear)"
  if buffer(13, 1):le_uint() == 1 then
    stream_text = " SD (Fluent)"
  end
  header:add_le(magic_bytes, buffer(0, 4))
  header:add_le(message_id,  buffer(4, 4))
        :append_text(" (" .. header_data.msg_type_str .. ")")
  header:add_le(message_len, buffer(8, 4))

  header:add_le(xml_enc_offset, buffer(12, 1))
        :append_text(" (& 0xF == " .. bit32.band(header_data.enc_offset, 0xF) .. ")")

  header:add_le(channel_id, buffer(12, 1))
  header:add_le(stream_id, buffer(13, 1))
        :append_text(stream_text)
  header:add_le(unknown, buffer(14, 1))
  header:add_le(msg_handle, buffer(15, 1))

  header:add_le(message_class, buffer(18, 2)):append_text(" (" .. header_data.class .. ")")

  if header_data.header_len == 24 then
    header:add_le(status_code, buffer(16, 2))
    header:add_le(f_bin_offset, buffer(20, 4))
  else
    header:add_le(encrypt_xml, buffer(16, 1))
  end
  return header_data.header_len
end

local function process_body(header, body_buffer, bc_subtree, pinfo)
  if header.msg_len == 0 then
    return
  end

  local body = bc_subtree:add(bc_protocol, body_buffer(0,header.msg_len),
    "Baichuan Message Body, " .. header.class .. ", length: " .. header.msg_len .. ", type " .. header.msg_type)

  if header.class == "legacy" then
    if header.msg_type == 1 then
      body:add_le(username, body_buffer(0, 32))
      body:add_le(password, body_buffer(0 + 32, 32))
    end
  else
    local xml_len = header.bin_offset
    if xml_len == nil then
      xml_len = header.msg_len
    end
    local xml_buffer = body_buffer(0, xml_len)
    if xml_len > 0 then
      local body_tvb = xml_buffer:tvb("Meta Payload")
      body:add(body_tvb(), "Meta Payload")
      if xml_len >= 4 then
        if xml_decrypt(xml_buffer(0,5):bytes(), header.enc_offset):raw() == "<?xml" then -- Encrypted xml found
          local ba = xml_buffer:bytes()
          local decrypted = xml_decrypt(ba, header.enc_offset)
          body_tvb = decrypted:tvb("Decrypted XML (in Meta Payload)")
          -- Create a tree item that, when clicked, automatically shows the tab we just created
          body:add(body_tvb(), "Decrypted XML (in Meta Payload)")
          Dissector.get("xml"):call(body_tvb, pinfo, body)
        elseif xml_buffer(0,5):string() == "<?xml" then  -- Unencrypted xml
          body:add(body_tvb(), "XML (in Meta Payload)")
          Dissector.get("xml"):call(body_tvb, pinfo, body)
        else
          body:add(body_tvb(), "Binary (in Meta Payload)")
        end
      end
    end

    if header.bin_offset ~= nil then
      local bin_len = header.msg_len - header.bin_offset
      if bin_len > 0 then
        local binary_buffer = body_buffer(header.bin_offset, bin_len) -- Don't extend beyond msg size
        local body_tvb = binary_buffer:tvb("Main Payload");
        body:add(body_tvb(), "Main Payload")
        if bin_len > 4 then
          if xml_decrypt(binary_buffer(0,5):bytes(), header.enc_offset):raw() == "<?xml" then -- Encrypted xml found
            local decrypted = xml_decrypt(binary_buffer:bytes(), header.enc_offset)
            body_tvb = decrypted:tvb("Decrypted XML (in Main Payload)")
            -- Create a tree item that, when clicked, automatically shows the tab we just created
            body:add(body_tvb(), "Decrypted XML (in Main Payload)")
            Dissector.get("xml"):call(body_tvb, pinfo, body)
          elseif binary_buffer(0,5):string() == "<?xml" then  -- Unencrypted xml
            body:add(body_tvb(), "XML (in Main Payload)")
            Dissector.get("xml"):call(body_tvb, pinfo, body)
          else
            body:add(body_tvb(), "Binary (in Main Payload)")
          end
        end
      end
    end
  end
end


-- UDP CONTENT
local udp_fragments = {}

local function rshift(x, by)
  return math.floor(x / 2 ^ by)
end

local function udp_decrypt(data, tid)
  local result = ByteArray.new()
  result:set_size(data:len())
  local key = {
    0x1f2d3c4b, 0x5a6c7f8d,
    0x38172e4b, 0x8271635a,
    0x863f1a2b, 0xa5c6f7d8,
    0x8371e1b4, 0x17f2d3a5
  }

  for i=1, 8 do
    key[i] = key[i] + tid
  end

  local i = data:len() + 3
  if i < 0 then
    i = data:len() + 6
  end

  for x=0, rshift(i, 2) do
    local index = bit32.band(x, 7)
    local xor_key_word = key[index + 1]
    for b=0, 3 do
      local byte_index = x * 4 + b
      local val = data:get_index(byte_index)
      local key_byte = bit32.extract(xor_key_word, b*8, 8)
      val = bit32.bxor(key_byte, val)
      result:set_index(byte_index, val)
      if byte_index >= data:len() - 1 then
        return result
      end
    end
  end
  return result
end


local function get_udp_header_len(buffer)
  local udpmagic = buffer(1, 3):le_uint()
  if udpmagic ~= 0x2a87cf then
    return 0
  else
    local udptype = buffer(0, 1):le_uint()
    if udptype == 0x3a then
      return 20
    elseif udptype == 0x31 then
      return 20
    elseif udptype == 0x20 then
      return 28
    elseif udptype == 0x10 then
      return 20
    else
      return -1
    end
  end
end

local function get_udp_header(buffer)
  local udp_class = buffer(0, 1):le_uint()
  local l_udp_magic = buffer(1, 3):le_uint()
  local length = get_udp_header_len(buffer)
  local l_udp_size = nil
  local udp_unknown1 = nil
  local l_udp_tid = nil
  local l_udp_checksum = nil
  local udp_unknown2 = nil
  local udp_unknown3 = nil
  local udp_unknown4 = nil
  local l_udp_last_ack_packet = nil
  local l_udp_ack_payload_size = nil
  local l_udp_connection_id = nil
  local l_udp_packet_count = nil
  if udp_class == 0x3a then
    l_udp_size = buffer(4, 4):le_uint()
    udp_unknown1 = buffer(8, 4):le_uint()
    l_udp_tid = buffer(12, 4):le_uint()
    l_udp_checksum = buffer(16, 4):le_uint()
  elseif udp_class == 0x31 then
    l_udp_size = buffer(4, 4):le_uint()
    udp_unknown1 = buffer(8, 4):le_uint()
    l_udp_tid = buffer(12, 4):le_uint()
    l_udp_checksum = buffer(16, 4):le_uint()
  elseif udp_class == 0x20 then
    l_udp_connection_id = buffer(4, 4):le_uint()
    udp_unknown1 = buffer(8, 4):le_uint()
    udp_unknown2 = buffer(12, 4):le_uint()
    l_udp_last_ack_packet = buffer(16, 4):le_uint()
    udp_unknown3 = buffer(20, 4):le_uint()
    l_udp_ack_payload_size = buffer(24, 4):le_uint()
  elseif udp_class == 0x10 then
    l_udp_connection_id = buffer(4, 4):le_uint()
    udp_unknown1 = buffer(8, 4):le_uint()
    l_udp_packet_count = buffer(12, 4):le_uint()
    l_udp_size = buffer(16, 4):le_uint()
  end
  return {
    length = length,
    class = udp_class,
    magic = l_udp_magic,
    payload_size = l_udp_size,
    unknown1 = udp_unknown1,
    unknown2 = udp_unknown2,
    unknown3 = udp_unknown3,
    unknown4 = udp_unknown4,
    tid = l_udp_tid,
    checksum = l_udp_checksum,
    connection_id = l_udp_connection_id,
    packet_count = l_udp_packet_count,
    last_ack_packet = l_udp_last_ack_packet,
    ack_payload_size = l_udp_ack_payload_size
  }
end

local function process_udp_header(buffer, headers_tree)
  local header_data = get_udp_header(buffer)
  local header = headers_tree:add(bc_protocol, buffer(0, header_data.length),
    "Baichuan UDP Header, length: " .. header_data.length .. ", type " .. header_data.class)
  header:add_le(udp_magic, buffer(0,4))
  header:add_le(udp_type, buffer(0,1))
  if header_data.class == 0x3a then
    header:add_le(udp_size, buffer(4, 4))
    header:add_le(udp_unknown,buffer(8, 4))
    header:add_le(udp_tid, buffer(12, 4))
    header:add_le(udp_checksum, buffer(16, 4))
  elseif header_data.class == 0x31 then
    header:add_le(udp_size, buffer(4, 4))
    header:add_le(udp_unknown,buffer(8, 4))
    header:add_le(udp_tid, buffer(12, 4))
    header:add_le(udp_checksum, buffer(16, 4))
  elseif header_data.class == 0x20 then
    header:add_le(udp_connection_id, buffer(4, 4))
    header:add_le(udp_unknown, buffer(8, 4))
    header:add_le(udp_unknown, buffer(12, 4))
    header:add_le(udp_last_ack_packet, buffer(16, 4))
    header:add_le(udp_unknown, buffer(20, 4))
    header:add_le(udp_ack_payload_size, buffer(24, 4))
  elseif header_data.class == 0x10 then
    header:add_le(udp_connection_id, buffer(4, 4))
    header:add_le(udp_unknown, buffer(8, 4))
    header:add_le(udp_packet_count, buffer(12, 4))
    header:add_le(udp_size, buffer(16, 4))
  end
  return header_data.length
end

local function process_bc_message(buffer, pinfo, tree)
  pinfo.cols.protocol = bc_protocol.name

  local sub_buffer = buffer
  local table_msg_type_str = {}
  local table_msg_type = {}

  local continue_loop = true
  while ( continue_loop )
  do
    local header_len = get_header_len(sub_buffer(0, nil))

    if header_len >= 0 then
      -- Valid magic and header found
      local header = get_header(sub_buffer(0, nil))
      table.insert(table_msg_type_str, header.msg_type_str)
      table.insert(table_msg_type, header.msg_type)

      -- Get full header and body
      local full_body_len =  header.msg_len + header.header_len

      local remaining = sub_buffer:len() - header.header_len

      local bc_subtree = tree:add(bc_protocol, sub_buffer(0, header.full_body_len),
        "Baichuan IP Camera Protocol, " .. header.msg_type_str .. ":" .. header.msg_type .. " message")
      process_header(sub_buffer, bc_subtree)
      if header.header_len < sub_buffer:len() then
        local body_buffer = sub_buffer(header.header_len,nil)
        process_body(header, body_buffer, bc_subtree, pinfo)

        remaining = body_buffer:len() - header.msg_len
      end

      -- Remaning bytes?
      if remaining == 0 then
        continue_loop = false
      else
        sub_buffer = sub_buffer(full_body_len, nil)
      end
    else
      return
    end
  end

  local msg_type_strs = table.concat(table_msg_type_str, ",")
  local msg_types = table.concat(table_msg_type, ",")
  pinfo.cols['info'] = msg_type_strs .. ", type " .. msg_types
  return
end

local function is_complete_bc_message(buffer)
  local length = buffer:len()
  if length == 0 then
    return "DONE"
  end

  local sub_buffer = buffer

  local continue_loop = true
  while ( continue_loop )
  do

    -- Get min bytes for a magic and header len
    if sub_buffer:len() < 20 then
      -- Need more bytes but we don't have a header to learn how many bytes
      return "+1"
    end
    local header_len = get_header_len(sub_buffer(0, nil))

    if header_len >= 0 then
      -- Valid magic and header found

      -- Ensure min bytes for full header
      if sub_buffer:len() < header_len then
        -- Need more bytes
        return header_len - sub_buffer:len()
      end


      local header = get_header(sub_buffer(0, nil))

      -- Get full header and body
      local full_body_len =  header.msg_len + header.header_len
      if sub_buffer:len() < full_body_len then
        return full_body_len - sub_buffer:len()
      end

      local remaining = sub_buffer:len() - header.header_len
      if header.header_len < sub_buffer:len() then
        local body_buffer = sub_buffer(header.header_len, nil)
        remaining = body_buffer:len() - header.msg_len
      end

      -- Remaning bytes?
      if remaining == 0 then
        continue_loop = false
      else
        sub_buffer = sub_buffer(full_body_len, sub_buffer:len() - full_body_len)
      end
    else
      return "NOMAGIC"
    end
  end
  return "DONE"
end

local function udp_reassemple(udp_header, subbuffer, more, pinfo, tree)
  -- Cache udp message for later lookup
  local con_id = udp_header.connection_id
  if udp_fragments[con_id] == nil then
    udp_fragments[con_id] = {}
  end
  local mess_id = udp_header.packet_count
  if udp_fragments[con_id][mess_id] == nil then
    udp_fragments[con_id][mess_id] = {}
  end
  udp_fragments[con_id][mess_id]['result'] = more
  udp_fragments[con_id][mess_id]['message_id'] = mess_id
  udp_fragments[con_id][mess_id]['buffer'] = subbuffer:bytes()

  -- Go backwards from current ID until:
  -- I hit a result that is not NOMAGIC
  -- Can be myself
  local start_idx = mess_id
  local start_fragment = udp_fragments[con_id][start_idx]
  while start_fragment.result == "NOMAGIC" do
    start_idx = start_idx -1
    start_fragment = udp_fragments[con_id][start_idx]
    if start_fragment == nil then
      break
    end
  end

  if start_fragment ~= nil then -- Found a starting fragment
    local needed = start_fragment.result
    if needed == "DONE" then
      if start_fragment.message_id == udp_header.packet_count then
        process_bc_message(start_fragment.buffer:tvb(), pinfo, tree)
      end
    elseif needed == "+1" then
      -- Cannot handle in UDP...
      -- Only happens in off chance not enough data for
      -- even the header
      -- Never observed to date
      return
    else
      -- pinfo.cols['info'] = "SEARCHING"
      local next_id = start_fragment.message_id + 1
      local reassembled = ByteArray.new()
      local total_packet = 1
      reassembled = reassembled .. start_fragment.buffer
      local target_len = reassembled:len() + start_fragment.result
      while reassembled:len() < target_len do
        local next_fragment = udp_fragments[con_id][next_id]
        if next_fragment ~= nil then
          reassembled = reassembled .. next_fragment.buffer
          total_packet = total_packet + 1
        else
          break
        end
      end
      if reassembled:len() >= target_len then
        start_fragment.result = "DONE"
        process_bc_message(reassembled:tvb("Reassembled UDP"), pinfo, tree)
      end
    end
  end
end

function bc_protocol.init ()
   udp_fragments = {}
end

function bc_protocol.dissector(buffer, pinfo, tree)
  local subbuffer = nil
  local udp_header = nil
  if pinfo.can_desegment == 0 then -- UDP
    local udp_header_len = get_udp_header_len(buffer)
    if udp_header_len > 0 then
      udp_header = get_udp_header(buffer(0, udp_header_len))
      process_udp_header(buffer(0, udp_header_len), tree)
      if udp_header.class == 0x3a then
        local decrypted_bytes = udp_decrypt(buffer(udp_header_len, nil):bytes(), udp_header.tid)
        local decryped_tvb = decrypted_bytes:tvb("UDP Decrypted Message")
        local subtree = tree:add(bc_protocol, decryped_tvb, "UDP Message Data")
        Dissector.get("xml"):call(decryped_tvb, pinfo, subtree)
        pinfo.cols.protocol = bc_protocol.name .. " UDP Heartbeat"
      elseif udp_header.class == 0x31 then
        pinfo.cols.protocol = bc_protocol.name .. " UDP Relay"

      elseif udp_header.class == 0x20 then
        pinfo.cols.protocol = bc_protocol.name .. " UDP ACK"
        if udp_header.ack_payload_size > 0 then
          tree:add(bc_protocol, buffer(28,udp_header.ack_payload_size), "BcUdp Ack Payload")
        end
      else
        subbuffer = buffer(udp_header_len, nil)
      end
    else
      subbuffer = buffer(udp_header_len, nil)
    end
  else
    subbuffer = buffer
  end
  if subbuffer ~= nil then
    local more = is_complete_bc_message(subbuffer)
    if pinfo.can_desegment == 1 then -- TCP can use the desegment method
      if more == "DONE" then
        process_bc_message(subbuffer, pinfo, tree)
        return
      elseif more == "+1" then
        pinfo.desegment_len = DESEGMENT_ONE_MORE_SEGMENT
        pinfo.desegment_offset = 0
        return subbuffer:len()
      elseif more == "NOMAGIC" then
        return
      else
        pinfo.desegment_len = more
        pinfo.desegment_offset = 0
        return subbuffer:len()
      end
    else -- UDP can not use the desegment method, must reassemble manually
      if udp_header ~= nil then
        if udp_header.class == 0x10 then -- Continuable udp class
          udp_reassemple(udp_header, subbuffer, more, pinfo, tree)
        end
      end
    end
  end
end
--- END UDP CONTENT

local added_udp_ports = {}
local function heuristic_checker_udp(buffer, pinfo, tree)
    -- guard for length
    local length = buffer:len()
    if length < 4 then return false end
    local potential_magic = buffer(0,4):le_uint()

    if potential_magic ~= 0x2a87cf3a  and
        potential_magic ~= 0x2a87cf20 and
        potential_magic ~= 0x2a87cf10 and
        potential_magic ~= 0x2a87cf31 then

      return false
    end

    if added_udp_ports[pinfo.dst_port] == nil then
      table.insert(added_udp_ports, pinfo.dst_port)
      DissectorTable.get("udp.port"):add(pinfo.dst_port, bc_protocol)
    end
    if added_udp_ports[pinfo.src_port] == nil then
      table.insert(added_udp_ports, pinfo.src_port)
      DissectorTable.get("udp.port"):add(pinfo.src_port, bc_protocol)
    end

    bc_protocol.dissector(buffer, pinfo, tree)
    return true
end

local added_tcp_ports = {}

local function heuristic_checker_tcp(buffer, pinfo, tree)
    -- guard for length
    local length = buffer:len()
    if length < 4 then return false end
    local potential_magic = buffer(0,4):le_uint()
    if potential_magic ~= 0x0abcdef0 and
        potential_magic ~= 0x0fedcba0  then
      return false
    end

    if added_tcp_ports[pinfo.dst_port] == nil then
      table.insert(added_tcp_ports, pinfo.dst_port)
      DissectorTable.get("tcp.port"):add(pinfo.dst_port, bc_protocol)
    end
    if added_tcp_ports[pinfo.src_port] == nil then
      table.insert(added_tcp_ports, pinfo.src_port)
      DissectorTable.get("tcp.port"):add(pinfo.src_port, bc_protocol)
    end

    bc_protocol.dissector(buffer, pinfo, tree)
    return true
end

bc_protocol:register_heuristic("udp", heuristic_checker_udp)
bc_protocol:register_heuristic("tcp", heuristic_checker_tcp)
-- DissectorTable.get("tcp.port"):add(53959, bc_protocol) -- change to your own custom port

-- DissectorTable.get("udp.port"):add(2000, bc_protocol)
-- DissectorTable.get("udp.port"):add(2015, bc_protocol)
-- DissectorTable.get("udp.port"):add(2018, bc_protocol)
-- DissectorTable.get("udp.port"):add(2000, bc_protocol)
-- DissectorTable.get("udp.port"):add(9999, bc_protocol)
