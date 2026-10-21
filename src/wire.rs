use {crate::Res, std::io::Cursor, wlproxy::proto};

pub(crate) fn read_uint(body: &[u8], word: usize) -> Res<u32> {
    let bytes = body
        .get(word * 4..word * 4 + 4)
        .ok_or("message too short")?;
    let bytes: [u8; 4] = bytes.try_into().map_err(|_| "invalid uint argument")?;
    Ok(u32::from_ne_bytes(bytes))
}

pub(crate) fn read_string(cursor: &mut Cursor<&[u8]>) -> Res<String> {
    Ok(proto::read_arg_string(cursor)?.unwrap_or_default())
}

pub(crate) fn uint_body(value: u32) -> Vec<u8> {
    value.to_ne_bytes().to_vec()
}

pub(crate) fn string_body(value: &str) -> Vec<u8> {
    let mut body = Vec::new();
    proto::write_arg_string(&mut body, value).expect("writing a string to a Vec cannot fail");
    body
}
