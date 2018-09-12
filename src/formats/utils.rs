

pub(crate) fn from_ascii(buf: &[u8]) -> String {
    let idx =
        if let Some(idx) = buf.iter().rposition(|x| (*x as char).is_alphanumeric()) {
            idx + 1
        } else {
            buf.len()
        };

    let mut s = "".to_string();
    for c in &buf[0..idx] {
        s.push(*c as char);
    }

    s
}
