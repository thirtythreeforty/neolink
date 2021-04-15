-- This is a Wireshark dissector for the Baichuan/Reolink proprietary IP camera protocol.
-- Copy/symlink it into ~/.local/lib/wireshark/plugins/ and restart Wireshark; it should
-- automatically attempt to decode TCP connections on port 9000.

bc_protocol = Proto("Baichuan",  "Baichuan/Reolink IP Camera Protocol")

magic_bytes = ProtoField.int32("baichuan.magic", "magic", base.DEC)
message_id =  ProtoField.int32("baichuan.msg_id", "messageId", base.DEC)
message_len = ProtoField.int32("baichuan.msg_len", "messageLen", base.DEC)
xml_enc_offset = ProtoField.int8("baichuan.xml_encryption_offset", "xmlEncryptionOffset", base.DEC)
encrypt_xml = ProtoField.bool("baichuan.encrypt_xml", "encrypt_xml", base.NONE)
channel_id =  ProtoField.int8("baichuan.channel_id", "channel_id", base.DEC)
stream_id = ProtoField.int8("baichuan.stream_id", "streamID", base.DEC)
unknown = ProtoField.int8("baichuan.unknown", "unknown", base.DEC)
msg_handle = ProtoField.int8("baichuan.message_handle", "messageHandle", base.DEC)
status_code = ProtoField.int16("baichuan.status_code", "status_code", base.DEC)
message_class = ProtoField.int32("baichuan.msg_class", "messageClass", base.DEC)
f_payload_offset = ProtoField.int32("baichuan.payload_offset", "binOffset", base.DEC)
username = ProtoField.string("baichuan.username", "username", base.ASCII)
password = ProtoField.string("baichuan.password", "password", base.ASCII)

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
  f_payload_offset,
  username,
  password,
}

message_types = {
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
  [216]="<EmailTask> (write)",
  [217]="<EmailTask>",
  [218]="<PushTask> (write)",
  [219]="<PushTask>",
  [228]="<Crop>",
  [229]="<Crop> (write)",
  [230]="<cropSnap>",
  [272]="<findAlarmVideo>",
  [273]="<alarmVideoInfo>",
  [274]="<findAlarmVideo>",
}

message_classes = {
  [0x6514]="legacy",
  [0x6614]="modern",
  [0x6414]="modern",
  [0x0000]="modern",
}

header_lengths = {
  [0x6514]=20,
  [0x6614]=20,
  [0x6414]=24,
  [0x0000]=24,
}

function xml_encrypt(ba, offset)
  local key = "\031\045\060\075\090\105\120\255" -- 1f, 2d, 3c, 4b, 5a, 69, 78 ,ff
  local e = ByteArray.new()
  e:set_size(ba:len())
  for i=0,ba:len() - 1 do
    e:set_index(i, bit32.bxor(bit32.band(offset, 0xFF), bit32.bxor(ba:get_index(i), key:byte(((i + offset) % 8) + 1))))
  end
  return e
end

function get_header_len(buffer)
  local magic = buffer(0, 4):le_uint()
  if magic == 0x0abcdef0 then
    -- Client <-> BC
  elseif magic == 0x0fedcba0 then
    -- BC <-> BC
  else
    -- Unknown magic
    return -1 -- No header found
  end
  local header_len = header_lengths[buffer(18, 2):le_uint()]
  return header_len
end

function get_header(buffer)
  -- bin_offset is either nil (no binary data) or nonzero
  -- TODO: bin_offset is actually stateful!
  local bin_offset = nil
  local status_code = nil
  local encrypt_xml = nil
  local header_len = header_lengths[buffer(18, 2):le_uint()]
  local msg_type = buffer(4, 4):le_uint()
  if header_len == 24 then
    bin_offset = buffer(20, 4):le_uint() -- if NHD-805/806 legacy protocol 30 30 30 30 aka "0000"
    status_code =  buffer(16, 2):le_uint()
  else
    encrypt_xml = buffer(16, 1):le_uint()
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
    encrypt_xml = encrypt_xml,
    channel_id = buffer(12, 1):le_uint(),
    enc_offset = buffer(12, 1):le_uint(),
    stream_type = stream_text,
    unknown = buffer(14, 1):le_uint(),
    msg_handle = buffer(15, 1):le_uint(),
    msg_cls = buffer(18, 2):le_uint(),
    status_code = status_code,
    class = message_classes[buffer(18, 2):le_uint()],
    header_len = header_lengths[buffer(18, 2):le_uint()],
    bin_offset = bin_offset,
  }
end

function process_header(buffer, headers_tree)
  local header_data = get_header(buffer)
  local header = headers_tree:add(bc_protocol, buffer(0, header_len),
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
  return header_len
end

function process_body(header, body_buffer, bc_subtree, pinfo)
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
        if xml_encrypt(xml_buffer(0,5):bytes(), header.enc_offset):raw() == "<?xml" then -- Encrypted xml found
          local ba = xml_buffer:bytes()
          local decrypted = xml_encrypt(ba, header.enc_offset)
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
        body_tvb = binary_buffer:tvb("Main Payload");
        body:add(body_tvb(), "Main Payload")
        if bin_len > 4 then
          if xml_encrypt(binary_buffer(0,5):bytes(), header.enc_offset):raw() == "<?xml" then -- Encrypted xml found
            local decrypted = xml_encrypt(binary_buffer:bytes(), header.enc_offset)
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

function bc_protocol.dissector(buffer, pinfo, tree)
  length = buffer:len()
  if length == 0 then return end

  pinfo.cols.protocol = bc_protocol.name

  local sub_buffer = buffer
  local table_msg_type_str = {}
  local table_msg_type = {}

  local continue_loop = true
  while ( continue_loop )
  do
    -- Get min bytes for a magic and header len
    if sub_buffer:len() < 20 then
      -- Need more bytes but we don't have a header to learn how many bytes
      pinfo.desegment_len = DESEGMENT_ONE_MORE_SEGMENT
      pinfo.desegment_offset = 0
      pinfo.cols['info'] = "Need more header"
      return
    end

    header_len = get_header_len(sub_buffer(0, nil))
    if header_len >= 0 then
      pinfo.cols['info'] = "Valid header"
      -- Valid magic and header found

      -- Ensure min bytes for full header
      if sub_buffer:len() < header_len then
        pinfo.cols['info'] = "Need even more header"
        -- Need more bytes
        pinfo.desegment_len = header_len - sub_buffer:len()
        pinfo.desegment_offset = 0
        return buffer:len()
      end


      local header = get_header(sub_buffer(0, nil))
      table.insert(table_msg_type_str, header.msg_type_str)
      table.insert(table_msg_type, header.msg_type)

      -- Get full header and body
      local full_body_len =  header.msg_len + header.header_len
      if sub_buffer:len() < full_body_len then
        -- need more bytes,
        -- from https://wiki.wireshark.org/Lua/Dissectors#TCP_reassembly
        pinfo.cols['info'] = "Need more body: " .. sub_buffer:len() .. " expected " .. full_body_len
        pinfo.desegment_len = full_body_len - sub_buffer:len()
        pinfo.desegment_offset = 0
        return buffer:len()
      end

      local remaining = sub_buffer:len() - header.header_len

      local bc_subtree = tree:add(bc_protocol, sub_buffer(0, header.msg_len),
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
        pinfo.cols['info'] = "Another packet"
        sub_buffer = sub_buffer(full_body_len, nil)
      end
    else
      pinfo.cols['info'] = "Invalid magic"
      return -- Not a valid header
    end
  end

  local msg_type_strs = table.concat(table_msg_type_str, ",")
  local msg_types = table.concat(table_msg_type, ",")
  pinfo.cols['info'] = msg_type_strs .. ", type " .. msg_types
end

DissectorTable.get("tcp.port"):add(9000, bc_protocol)
DissectorTable.get("tcp.port"):add(52941, bc_protocol) -- change to your own custom port
