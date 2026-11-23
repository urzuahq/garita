const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// A stable 16-hex-digit FNV-1a 64 hash of rule, file and message; line and column are left out
/// so a finding keeps its fingerprint when lines shift.
pub fn fingerprint(rule: &str, file: Option<&str>, message: &str) -> String {
    let mut hash = OFFSET;
    let (tag, name): (&[u8], &[u8]) = match file {
        Some(file) => (b"f", file.as_bytes()),
        None => (b"-", b""),
    };
    let parts: [&[u8]; 6] = [rule.as_bytes(), &[0], tag, name, &[0], message.as_bytes()];
    for byte in parts.iter().flat_map(|p| p.iter()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")
}
