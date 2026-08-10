use super::encode_record;

fn decode_record(bytes: &[u8]) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let mut remaining = bytes;
    while !remaining.is_empty() {
        let newline = remaining.iter().position(|byte| *byte == b'\n').unwrap();
        let header = &remaining[..newline];
        remaining = &remaining[newline + 1..];
        if let Some(equals) = header.iter().position(|byte| *byte == b'=') {
            fields.push((
                String::from_utf8(header[..equals].to_vec()).unwrap(),
                String::from_utf8(header[equals + 1..].to_vec()).unwrap(),
            ));
            continue;
        }

        let key = String::from_utf8(header.to_vec()).unwrap();
        let length = u64::from_le_bytes(remaining[..8].try_into().unwrap()) as usize;
        remaining = &remaining[8..];
        let value = String::from_utf8(remaining[..length].to_vec()).unwrap();
        remaining = &remaining[length..];
        assert_eq!(remaining.first(), Some(&b'\n'));
        remaining = &remaining[1..];
        fields.push((key, value));
    }
    fields
}

#[test]
fn newline_value_uses_binary_framing_and_cannot_inject_a_field() {
    let forged = "resource\nAUDIT=forged";
    let record = encode_record(&[
        ("MESSAGE", forged),
        ("AUDIT", "writeback_rejected"),
        ("RESOURCE_NAME", "safe-name"),
    ]);

    assert_eq!(
        &record[..8],
        b"MESSAGE\n",
        "newline-containing value must not use KEY=value text framing"
    );
    assert_eq!(
        u64::from_le_bytes(record[8..16].try_into().unwrap()),
        forged.len() as u64
    );
    assert_eq!(
        decode_record(&record),
        vec![
            ("MESSAGE".to_string(), forged.to_string()),
            ("AUDIT".to_string(), "writeback_rejected".to_string()),
            ("RESOURCE_NAME".to_string(), "safe-name".to_string()),
        ]
    );
}

#[test]
fn newline_free_values_retain_text_framing() {
    assert_eq!(
        encode_record(&[("AUDIT", "writeback_rejected"), ("PRIORITY", "4")]),
        b"AUDIT=writeback_rejected\nPRIORITY=4\n"
    );
}
