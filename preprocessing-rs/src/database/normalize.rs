/// Normalize a CSV column name to snake_case: lowercase, runs of non-alphanumeric → `_`.
pub fn normalize_column_name(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut prev_was_sep = false;
    for ch in name.chars() {
        if ch.is_alphanumeric() {
            result.push(ch.to_ascii_lowercase());
            prev_was_sep = false;
        } else if !prev_was_sep {
            result.push('_');
            prev_was_sep = true;
        }
    }
    // Strip leading/trailing underscores.
    result.trim_matches('_').to_string()
}

/// Strip currency symbols and commas, then parse as f64. Returns None if not numeric.
pub fn parse_numeric(s: &str) -> Option<f64> {
    let cleaned: String = s.chars().filter(|&c| c != '$' && c != ',').collect();
    cleaned.trim().parse::<f64>().ok()
}

/// Try to parse a date string into ISO format (YYYY-MM-DD).
/// Accepts: %m/%d/%Y, %Y-%m-%d, %m/%d/%Y %H:%M:%S, ISO 8601 with/without time,
/// and DOB NOW format %m/%d/%y %I:%M:%S %p (2-digit year, 12-hour clock).
pub fn parse_date(s: &str) -> Option<String> {
    // Normalize runs of whitespace to a single space so formats don't need to
    // account for the double-space separator used by DOB NOW exports.
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Try formats in order of likelihood.
    let formats = [
        "%m/%d/%Y",
        "%Y-%m-%d",
        "%m/%d/%y %I:%M:%S %p",
        "%m/%d/%y %I:%M %p",
        "%m/%d/%Y %H:%M:%S",
        "%m/%d/%Y %I:%M:%S %p",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%m/%d/%Y %H:%M",
    ];

    for fmt in &formats {
        if let Ok(dt) = chrono::NaiveDate::parse_from_str(s, fmt) {
            return Some(dt.format("%Y-%m-%d").to_string());
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt.date().format("%Y-%m-%d").to_string());
        }
    }
    None
}

/// Construct a 10-digit BBL from (borough_number, block, lot).
/// block is zero-padded to 5 digits, lot to 4 digits.
pub fn build_bbl(borough: &str, block: &str, lot: &str) -> Option<String> {
    let boro: u8 = borough.trim().parse().ok()?;
    if !(1..=5).contains(&boro) {
        return None;
    }
    let block_n: u32 = block.trim().replace(',', "").parse().ok()?;
    let lot_n: u32 = lot.trim().replace(',', "").parse().ok()?;
    Some(format!("{}{:05}{:04}", boro, block_n, lot_n))
}

/// Parse a 10-digit BBL into (borough_number, block, lot).
#[allow(dead_code)]
pub fn parse_bbl(bbl: &str) -> Option<(u8, String, String)> {
    let bbl = bbl.trim();
    if bbl.len() != 10 {
        return None;
    }
    let boro: u8 = bbl[0..1].parse().ok()?;
    let block = bbl[1..6].to_string();
    let lot = bbl[6..10].to_string();
    Some((boro, block, lot))
}

/// Map borough name to number (1-5).
pub fn borough_name_to_number(name: &str) -> Option<u8> {
    match name.trim().to_uppercase().as_str() {
        "MANHATTAN" | "MN" => Some(1),
        "BRONX" | "BX" => Some(2),
        "BROOKLYN" | "BK" => Some(3),
        "QUEENS" | "QN" | "QS" => Some(4),
        "STATEN ISLAND" | "SI" => Some(5),
        _ => None,
    }
}

/// Map borough number to name.
pub fn borough_number_to_name(n: u8) -> Option<&'static str> {
    match n {
        1 => Some("MANHATTAN"),
        2 => Some("BRONX"),
        3 => Some("BROOKLYN"),
        4 => Some("QUEENS"),
        5 => Some("STATEN ISLAND"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_name_normalization() {
        assert_eq!(normalize_column_name("Height Roof"), "height_roof");
        assert_eq!(normalize_column_name("BASE_BBL"), "base_bbl");
        assert_eq!(normalize_column_name("C/O Issuance Date"), "c_o_issuance_date");
        assert_eq!(normalize_column_name("  leading "), "leading");
    }

    #[test]
    fn numeric_parsing() {
        assert_eq!(parse_numeric("$1,234.56"), Some(1234.56));
        assert_eq!(parse_numeric("100"), Some(100.0));
        assert_eq!(parse_numeric("N/A"), None);
        assert_eq!(parse_numeric(""), None);
    }

    #[test]
    fn date_parsing_formats() {
        assert_eq!(parse_date("03/15/2024"), Some("2024-03-15".to_string()));
        assert_eq!(parse_date("2024-03-15"), Some("2024-03-15".to_string()));
        assert_eq!(parse_date("03/15/2024 10:30:00"), Some("2024-03-15".to_string()));
        // DOB NOW format: 2-digit year, 12-hour clock, double space separator
        assert_eq!(parse_date("09/02/25  1:24:22 PM"), Some("2025-09-02".to_string()));
        assert_eq!(parse_date("10/02/24  1:39:31 PM"), Some("2024-10-02".to_string()));
        // DOB NOW job application dates: ISO with milliseconds
        assert_eq!(parse_date("2019-10-24T00:00:00.000"), Some("2019-10-24".to_string()));
        assert_eq!(parse_date("2025-06-05T18:00:26.000"), Some("2025-06-05".to_string()));
        assert_eq!(parse_date(""), None);
        assert_eq!(parse_date("not-a-date"), None);
    }

    #[test]
    fn bbl_construction() {
        assert_eq!(build_bbl("1", "123", "45"), Some("1001230045".to_string()));
        assert_eq!(build_bbl("6", "1", "1"), None); // invalid borough
    }

    #[test]
    fn bbl_parsing() {
        let (boro, block, lot) = parse_bbl("1001230045").unwrap();
        assert_eq!(boro, 1);
        assert_eq!(block, "00123");
        assert_eq!(lot, "0045");
        assert!(parse_bbl("short").is_none());
    }

    #[test]
    fn borough_mappings() {
        assert_eq!(borough_name_to_number("MANHATTAN"), Some(1));
        assert_eq!(borough_name_to_number("Staten Island"), Some(5));
        assert_eq!(borough_name_to_number("UNKNOWN"), None);
        assert_eq!(borough_number_to_name(3), Some("BROOKLYN"));
        assert_eq!(borough_number_to_name(9), None);
    }
}
