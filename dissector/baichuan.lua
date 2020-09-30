-- This is a Wireshark dissector for the Baichuan/Reolink proprietary IP camera protocol.
-- Copy/symlink it into ~/.local/lib/wireshark/plugins/ and restart Wireshark; it should
-- automatically attempt to decode TCP connections on port 9000.

bc_protocol = Proto("Baichuan",  "Baichuan/Reolink IP Camera Protocol")

magic_bytes = ProtoField.int32("baichuan.magic", "magic", base.DEC)
message_id =  ProtoField.int32("baichuan.msg_id", "messageId", base.DEC)
message_len = ProtoField.int32("baichuan.msg_len", "messageLen", base.DEC)
xml_enc_offset = ProtoField.int32("baichuan.xml_encryption_offset", "xmlEncryptionOffset", base.DEC)
xml_enc_used = ProtoField.bool("baichuan.xml_encryption_used", "encrypted", base.NONE)
message_class = ProtoField.int32("baichuan.msg_class", "messageClass", base.DEC)
f_bin_offset = ProtoField.int32("baichuan.bin_offset", "binOffset", base.DEC)
username = ProtoField.string("baichuan.username", "username", base.ASCII)
password = ProtoField.string("baichuan.password", "password", base.ASCII)

bc_protocol.fields = {
  magic_bytes,
  message_id,
  message_len,
  xml_enc_offset,
  xml_enc_used,
  message_class,
  f_bin_offset,
  username,
  password,
}

message_types = {
  [1]="login",
  [3]="<Preview> (video)",
  [58]="<AbilitySupport>",
  [78]="<VideoInput>",
  [79]="<Serial>",
  [80]="<VersionInfo>",
  [114]="<Uid>",
  [133]="<RfAlarm>"
  [146]="<StreamInfoList>",
  [151]="<AbilityInfo>",
  [230]="<cropSnap>",
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
  local key = "\031\045\060\075\090\105\120\255"
  local e = ByteArray.new()
  e:set_size(ba:len())
  for i=0,ba:len() - 1 do
    e:set_index(i, bit32.bxor(bit32.band(offset, 0xFF), bit32.bxor(ba:get_index(i), key:byte(((i + offset) % 8) + 1))))
  end
  return e
end

function get_header_len(buffer)
  local magic = buffer(0, 4):le_uint()
  if magic ~= 0x0abcdef0 then
    return -1 -- No header found
  end
  local header_len = header_lengths[buffer(18, 2):le_uint()]
  return header_len
end

function get_header(buffer)
  -- bin_offset is either nil (no binary data) or nonzero
  -- TODO: bin_offset is actually stateful!
  local bin_offset = nil
  local header_len = header_lengths[buffer(18, 2):le_uint()]
  local msg_type = buffer(4, 4):le_uint()
  if header_len == 24 then
    bin_offset = buffer(20, 4):le_uint() -- if NHD-805/806 legacy protocol 30 30 30 30 aka "0000"
  end
  local msg_type = buffer(4, 4):le_uint()
  return {
    magic = buffer(0, 4):le_uint(),
    msg_type = buffer(4, 4):le_uint(),
    msg_type_str = message_types[msg_type] or "unknown",
    msg_len = buffer(8, 4):le_uint(),
    enc_offset = buffer(12, 4):le_uint(),
    msg_cls = buffer(18, 2):le_uint(),
    encrypted = (msg_cls == 0x6414 or buffer(16, 1):le_uint() ~= 0),
    class = message_classes[buffer(18, 2):le_uint()],
    header_len = header_lengths[buffer(18, 2):le_uint()],
    bin_offset = bin_offset,
  }
end

function process_header(buffer, headers_tree)
  local header_data = get_header(buffer)
  local header = headers_tree:add(bc_protocol, buffer(0, header_len),
    "Baichuan Message Header, length: " .. header_data.header_len .. ", type " .. header_data.msg_type)
  header:add_le(magic_bytes, buffer(0, 4))
  header:add_le(message_id,  buffer(4, 4))
        :append_text(" (" .. header_data.msg_type_str .. ")")
  header:add_le(message_len, buffer(8, 4))
  header:add_le(xml_enc_offset, buffer(12, 4))
        :append_text(" (& 0xF == " .. bit32.band(header_data.enc_offset, 0xF) .. ")")
  header:add_le(xml_enc_used, buffer(16, 1))
  header:add_le(message_class, buffer(18, 2)):append_text(" (" .. header_data.class .. ")")
  if header_data.header_len == 24 then
    header:add_le(f_bin_offset, buffer(20, 4))
  end
  return header_len
end

function process_body(header, body_buffer, bc_subtree, pinfo)
  if header.msg_len == 0 then
    return
  end

  local body = bc_subtree:add(bc_protocol, body_buffer(0,header.msg_len),
    "Baichuan Message Body, " .. header.class .. ", length: " .. header.msg_len .. ", type " .. header.msg_type .. ", encrypted: " .. tostring(header.encrypted))

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
      local body_tvb = xml_buffer:tvb()
      if xml_len >= 4 then
        if xml_encrypt(binary_buffer(0,5):bytes(), header.enc_offset):raw() == "<?xml" then -- Encrypted xml found
          body:add(body_tvb(), "XML Payload")
          local ba = xml_buffer:bytes()
          local decrypted = xml_encrypt(ba, header.enc_offset)
          body_tvb = decrypted:tvb("Decrypted XML")
          -- Create a tree item that, when clicked, automatically shows the tab we just created
          body:add(body_tvb(), "Decrypted XML")
          Dissector.get("xml"):call(body_tvb, pinfo, body)
        elseif binary_buffer(0,5):string() == "<?xml" then  -- Unencrypted xml
          body:add(body_tvb(), "Decrypted XML")
          Dissector.get("xml"):call(body_tvb, pinfo, body)
        end
      end
    end

    if header.bin_offset ~= nil then
      local bin_len = header.msg_len - header.bin_offset
      if bin_len > 0 then
        local binary_buffer = body_buffer(header.bin_offset, bin_len) -- Don't extend beyond msg size
        if bin_len > 4 then
          if xml_encrypt(binary_buffer(0,5):bytes(), header.enc_offset):raw() == "<?xml" then -- Encrypted xml found
            local decrypted = xml_encrypt(binary_buffer:bytes(), header.enc_offset)
            body_tvb = decrypted:tvb("Decrypted XML (in binary block)")
            -- Create a tree item that, when clicked, automatically shows the tab we just created
            body:add(body_tvb(), "Decrypted XML (in binary block)")
            Dissector.get("xml"):call(body_tvb, pinfo, body)
          end
        else
          local binary_tvb = binary_buffer:tvb()
          body:add(binary_tvb(), "Binary Payload")
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
DissectorTable.get("tcp.port"):add(52941, bc_protocol)
