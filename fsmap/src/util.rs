//! Small helpers: human sizes, size parsing, number formatting, dates, HTML escaping.

/// Format bytes in decimal units, the way Finder shows sizes (1 GB = 1,000,000,000 bytes).
pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["kB", "MB", "GB", "TB", "PB", "EB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64;
    let mut unit = "B";
    for u in UNITS {
        if v < 1000.0 {
            break;
        }
        v /= 1000.0;
        unit = u;
    }
    if v < 10.0 {
        format!("{v:.2} {unit}")
    } else if v < 100.0 {
        format!("{v:.1} {unit}")
    } else {
        format!("{v:.0} {unit}")
    }
}

/// Parse sizes like `1MiB`, `500k`, `2G`, `1.5GB`, `4096`.
/// Single letters and `*iB` are binary (1024); `kB`/`MB`/`GB`/`TB` are decimal (1000).
pub fn parse_size(s: &str) -> Result<u64, String> {
    let t = s.trim();
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let n: f64 = num
        .parse()
        .map_err(|_| format!("'{s}' is not a size (try 1MiB, 500k, 2G)"))?;
    let mult: u64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kib" => 1 << 10,
        "m" | "mib" => 1 << 20,
        "g" | "gib" => 1 << 30,
        "t" | "tib" => 1 << 40,
        "kb" => 1_000,
        "mb" => 1_000_000,
        "gb" => 1_000_000_000,
        "tb" => 1_000_000_000_000,
        other => return Err(format!("unknown size unit '{other}' in '{s}'")),
    };
    Ok((n * mult as f64).round() as u64)
}

/// 1234567 -> "1,234,567"
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Unix seconds -> "2026-11-02 14:05 UTC" (no time-zone database needed).
pub fn utc_string(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        rem / 3600,
        (rem % 3600) / 60
    )
}

pub fn html_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

/// A text bar like `██████░░░░` for the terminal.
pub fn bar(part: u64, whole: u64, width: usize) -> String {
    let filled = if whole == 0 {
        0
    } else {
        ((part as f64 / whole as f64) * width as f64).round() as usize
    }
    .min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(parse_size("1MiB").unwrap(), 1 << 20);
        assert_eq!(parse_size("500k").unwrap(), 500 * 1024);
        assert_eq!(parse_size("2G").unwrap(), 2 << 30);
        assert_eq!(parse_size("1.5GB").unwrap(), 1_500_000_000);
        assert_eq!(parse_size("4096").unwrap(), 4096);
        assert!(parse_size("lots").is_err());
        assert!(parse_size("3 parsecs").is_err());
    }

    #[test]
    fn human_and_thousands() {
        assert_eq!(human(999), "999 B");
        assert_eq!(human(1_000), "1.00 kB");
        assert_eq!(human(4_000_000_000_000), "4.00 TB");
        assert_eq!(human(256_000_000_000), "256 GB");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(12), "12");
    }

    #[test]
    fn dates_and_escape() {
        assert_eq!(utc_string(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc_string(1_709_210_096), "2024-02-29 12:34 UTC");
        assert_eq!(
            html_escape("<a href=\"x\">&</a>"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;"
        );
        assert_eq!(bar(1, 2, 4), "██░░");
    }
}
