//! Text encodings for subtitle files.

use encoding_rs::Encoding;

/// (label understood by `encoding_rs`, human readable name)
pub const ENCODINGS: &[(&str, &str)] = &[
    ("auto", "Auto-detect"),
    ("utf-8", "Unicode (UTF-8)"),
    ("utf-16le", "Unicode (UTF-16 LE)"),
    ("utf-16be", "Unicode (UTF-16 BE)"),
    ("shift_jis", "Japanese (Shift-JIS)"),
    ("euc-jp", "Japanese (EUC-JP)"),
    ("iso-2022-jp", "Japanese (ISO-2022-JP)"),
    ("gb18030", "Chinese Simplified (GB18030/GBK)"),
    ("big5", "Chinese Traditional (Big5)"),
    ("euc-kr", "Korean (EUC-KR)"),
    ("windows-1250", "Central European (Windows-1250)"),
    ("windows-1251", "Cyrillic (Windows-1251)"),
    ("koi8-r", "Cyrillic (KOI8-R)"),
    ("windows-1252", "Western European (Windows-1252)"),
    ("windows-1253", "Greek (Windows-1253)"),
    ("windows-1254", "Turkish (Windows-1254)"),
    ("windows-1255", "Hebrew (Windows-1255)"),
    ("windows-1256", "Arabic (Windows-1256)"),
    ("windows-1257", "Baltic (Windows-1257)"),
    ("windows-1258", "Vietnamese (Windows-1258)"),
    ("windows-874", "Thai (Windows-874)"),
    ("iso-8859-2", "Central European (ISO-8859-2)"),
    ("iso-8859-5", "Cyrillic (ISO-8859-5)"),
    ("iso-8859-15", "Western European (ISO-8859-15)"),
];

/// Decode raw bytes using `label` (`"auto"` sniffs BOM / UTF-8 / common CJK).
pub fn decode(bytes: &[u8], label: &str) -> String {
    let label = label.trim().to_ascii_lowercase();
    if label.is_empty() || label == "auto" {
        return decode_auto(bytes);
    }
    let enc = Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::UTF_8);
    // decode() honours a BOM if present, which is what users expect.
    let (text, _, _) = enc.decode(bytes);
    text.into_owned()
}

fn decode_auto(bytes: &[u8]) -> String {
    if let Some((enc, _)) = Encoding::for_bom(bytes) {
        return enc.decode(bytes).0.into_owned();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    for enc in [encoding_rs::SHIFT_JIS, encoding_rs::EUC_KR, encoding_rs::GB18030, encoding_rs::BIG5] {
        let (text, _, had_errors) = enc.decode(bytes);
        if !had_errors {
            return text.into_owned();
        }
    }
    encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned()
}

pub fn index_of(label: &str) -> usize {
    ENCODINGS.iter().position(|(l, _)| l.eq_ignore_ascii_case(label)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sjis_auto() {
        let (bytes, _, _) = encoding_rs::SHIFT_JIS.encode("こんにちは、世界");
        assert_eq!(decode(&bytes, "auto"), "こんにちは、世界");
        assert_eq!(decode(&bytes, "shift_jis"), "こんにちは、世界");
    }
    #[test]
    fn bom() {
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice("abc".as_bytes());
        assert_eq!(decode(&b, "auto"), "abc");
        assert_eq!(decode(&b, "utf-8"), "abc");
    }
}
