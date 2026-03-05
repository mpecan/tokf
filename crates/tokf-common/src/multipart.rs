const BOUNDARY: &str = "tokf-multipart-boundary";

/// Build a multipart/form-data body manually.
///
/// reqwest's streaming multipart body gets truncated when sent via the
/// blocking client (parts beyond the first two are silently dropped).
/// Building a byte buffer with known `Content-Length` avoids this issue.
pub fn build_body(fields: &[(&str, &[u8])]) -> (Vec<u8>, String) {
    let mut body = Vec::new();
    for (name, content) in fields {
        let header =
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n");
        body.extend_from_slice(header.as_bytes());
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
    (body, content_type)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn build_body_produces_valid_multipart() {
        let (body, content_type) = build_body(&[
            ("filter", b"command = \"test\"\n"),
            ("mit_license_accepted", b"true"),
            ("test:basic.toml", b"name = \"basic\"\n"),
        ]);
        assert!(content_type.contains("boundary="));
        let body_str = String::from_utf8(body).unwrap();
        // 3 parts + closing boundary = 4 boundary markers
        assert_eq!(
            body_str.matches(&format!("--{BOUNDARY}")).count(),
            4,
            "expected 4 boundary markers (3 parts + closing)"
        );
        assert!(body_str.contains("name=\"filter\""));
        assert!(body_str.contains("name=\"mit_license_accepted\""));
        assert!(body_str.contains("name=\"test:basic.toml\""));
    }

    #[test]
    fn build_body_empty_fields() {
        let (body, content_type) = build_body(&[]);
        assert!(content_type.starts_with("multipart/form-data; boundary="));
        let body_str = String::from_utf8(body).unwrap();
        assert!(body_str.contains("--tokf-multipart-boundary--"));
    }
}
