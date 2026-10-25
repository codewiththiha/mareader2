//! The top-level domains that tell an address from a word with a dot in it.

/// Whether `label` is one, in any case.
pub(super) fn is_tld(label: &str) -> bool {
    TLDS.binary_search(&label.to_uppercase().as_str()).is_ok()
}

/// IANA's country codes and its generic, non-brand names, sorted.
#[rustfmt::skip]
const TLDS: &[&str] = &[
    "AC", "AD", "AE", "AERO", "AF", "AG", "AI", "AL", "AM", "AO", "APP", "AQ", "AR", "ARPA", "ART",
    "AS", "ASIA", "AT", "AU", "AW", "AX", "AZ", "BA", "BB", "BD", "BE", "BF", "BG", "BH", "BI",
    "BIZ", "BJ", "BLOG", "BM", "BN", "BO", "BR", "BS", "BT", "BV", "BW", "BY", "BZ", "CA", "CAT",
    "CC", "CD", "CF", "CG", "CH", "CI", "CK", "CL", "CLOUD", "CLUB", "CM", "CN", "CO", "COM",
    "COOP", "CR", "CU", "CV", "CW", "CX", "CY", "CZ", "DE", "DESIGN", "DEV", "DJ", "DK", "DM",
    "DO", "DZ", "EC", "EDU", "EE", "EG", "EMAIL", "ER", "ES", "ET", "EU", "FI", "FJ", "FK", "FM",
    "FO", "FR", "FUN", "GA", "GB", "GD", "GE", "GF", "GG", "GH", "GI", "GL", "GM", "GN", "GOV",
    "GP", "GQ", "GR", "GS", "GT", "GU", "GURU", "GW", "GY", "HK", "HM", "HN", "HR", "HT", "HU",
    "ID", "IE", "IL", "IM", "IN", "INFO", "INT", "IO", "IQ", "IR", "IS", "IT", "JE", "JM", "JO",
    "JOBS", "JP", "KE", "KG", "KH", "KI", "KM", "KN", "KP", "KR", "KW", "KY", "KZ", "LA", "LB",
    "LC", "LI", "LINK", "LIVE", "LK", "LR", "LS", "LT", "LU", "LV", "LY", "MA", "MC", "MD", "ME",
    "MG", "MH", "MIL", "MK", "ML", "MM", "MN", "MO", "MOBI", "MP", "MQ", "MR", "MS", "MT", "MU",
    "MUSEUM", "MV", "MW", "MX", "MY", "MZ", "NA", "NAME", "NC", "NE", "NET", "NEWS", "NF", "NG",
    "NI", "NL", "NO", "NP", "NR", "NU", "NZ", "OM", "ONLINE", "ORG", "PA", "PAGE", "PE", "PF",
    "PG", "PH", "PK", "PL", "PM", "PN", "POST", "PR", "PRO", "PS", "PT", "PW", "PY", "QA", "RE",
    "RO", "RS", "RU", "RW", "SA", "SB", "SC", "SD", "SE", "SG", "SH", "SHOP", "SI", "SITE", "SJ",
    "SK", "SL", "SM", "SN", "SO", "SPACE", "SR", "SS", "ST", "STORE", "SU", "SV", "SX", "SY", "SZ",
    "TC", "TD", "TECH", "TEL", "TF", "TG", "TH", "TJ", "TK", "TL", "TM", "TN", "TO", "TR",
    "TRAVEL", "TT", "TV", "TW", "TZ", "UA", "UG", "UK", "US", "UY", "UZ", "VA", "VC", "VE", "VG",
    "VI", "VN", "VU", "WEBSITE", "WF", "WIKI", "WS", "XXX", "XYZ", "YE", "YT", "ZA", "ZM", "ZW",
];
