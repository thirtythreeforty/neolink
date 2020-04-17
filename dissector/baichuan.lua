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
  [3]="video",
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

function bc_protocol.dissector(buffer, pinfo, tree)
  length = buffer:len()
  if length == 0 then return end

  pinfo.cols.protocol = bc_protocol.name

  local magic = buffer(0, 4):le_uint()
  if magic ~= 0x0abcdef0 then
    -- Case 3, capture started in middle of packet,
    -- from https://wiki.wireshark.org/Lua/Dissectors#TCP_reassembly
    -- The camera always seems to emit a new TCP packet for a new message
    return 0
  end

  if buffer:len() < 20 then
    -- Need more bytes but we don't have a header to learn how many bytes
    return DESEGMENT_ONE_MORE_SEGMENT
  end

  local msg_type = buffer(4, 4):le_uint()
  local msg_type_str = message_types[msg_type] or "unknown"
  local msg_len = buffer(8, 4):le_uint()
  local enc_offset = buffer(12, 4):le_uint()
  local msg_cls = buffer(18, 2):le_uint()
  local encrypted = (msg_cls == 0x6414 or buffer(16, 1):le_uint() ~= 0)
  local class = message_classes[buffer(18, 2):le_uint()]
  local header_len = header_lengths[buffer(18, 2):le_uint()]

  if buffer:len() ~= msg_len + header_len then
    -- Case 1, need more bytes,
    -- from https://wiki.wireshark.org/Lua/Dissectors#TCP_reassembly
    pinfo.desegment_len = msg_len + header_len - buffer:len()
    pinfo.desegment_offset = 0
    return buffer:len()
  end

  -- bin_offset is either nil (no binary data) or nonzero
  local bin_offset = nil
  if header_len == 24 then
    bin_offset = buffer(20, 4):le_uint()
    if bin_offset == 0 then bin_offset = nil end
  end

  local bc_subtree = tree:add(bc_protocol, buffer(),
    "Baichuan IP Camera Protocol, " .. msg_type_str .. " message")
  local header = bc_subtree:add(bc_protocol, buffer(0, header_len),
    "Baichuan Message Header, length: " .. header_len)

  header:add_le(magic_bytes, buffer(0, 4))
  header:add_le(message_id,  buffer(4, 4))
        :append_text(" (" .. msg_type_str .. ")")
  header:add_le(message_len, buffer(8, 4))
  header:add_le(xml_enc_offset, buffer(12, 4))
        :append_text(" (& 0xF == " .. bit32.band(enc_offset, 0xF) .. ")")
  header:add_le(xml_enc_used, buffer(16, 1))
  header:add_le(message_class, buffer(18, 2))
        :append_text(" (" .. class .. ")")
  header:add_le(f_bin_offset, buffer(20, 4))

  if msg_len == 0 then
    return
  end

  local body = bc_subtree:add(bc_protocol, buffer(header_len, nil),
    "Baichuan Message Body, " .. class .. ", length: " .. msg_len .. ", encrypted: " .. tostring(encrypted))

  if class == "legacy" then
    if msg_type == 1 then
      body:add_le(username, buffer(header_len, 32))
      body:add_le(password, buffer(header_len + 32, 32))
    end
  else
    local body_tvb = buffer(header_len, bin_offset):tvb()
    body:add(body_tvb(), "Encrypted XML")

    if encrypted then
      local ba = buffer(header_len, bin_offset):bytes()
      local decrypted = xml_encrypt(ba, enc_offset)
      body_tvb = decrypted:tvb("Decrypted XML")
      -- Create a tree item that, when clicked, automatically shows the tab we just created
      body:add(body_tvb(), "Decrypted XML")
    end
    Dissector.get("xml"):call(body_tvb, pinfo, body)

    if bin_offset ~= nil then
      local binary_tvb = buffer(header_len + bin_offset, nil):tvb()
      body:add(binary_tvb(), "Binary Payload")
      if msg_type == 0x03 then -- video
        Dissector.get("h265"):call(binary_tvb, pinfo, tree)
      end
    end
  end
end

DissectorTable.get("tcp.port"):add(9000, bc_protocol)
