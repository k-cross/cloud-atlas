use std::net::IpAddr;

pub fn is_large_cidr(cidr: &str) -> bool {
    if cidr == "0.0.0.0/0" || cidr == "::/0" || cidr == "*" {
        return true;
    }
    if let Some(slash_idx) = cidr.find('/')
        && let Ok(prefix) = cidr[slash_idx + 1..].parse::<u8>()
    {
        return prefix < 16;
    }
    false
}

pub fn canonical_address(value: &str) -> Option<String> {
    let (addr, prefix) = match value.split_once('/') {
        Some((addr, prefix)) => (addr, Some(prefix.parse::<u8>().ok()?)),
        None => (value, None),
    };

    let (addr, prefix) = match addr.parse::<IpAddr>().ok()? {
        IpAddr::V4(v4) => (IpAddr::V4(v4), prefix),
        IpAddr::V6(v6) => match (v6.to_ipv4_mapped(), prefix) {
            // A mapped prefix shorter than the ::ffff:0:0/96 block spans more
            // than the v4 range it looks like, so it is not that range.
            (Some(v4), Some(width)) if width >= 96 => (IpAddr::V4(v4), Some(width - 96)),
            (Some(v4), None) => (IpAddr::V4(v4), None),
            _ => (IpAddr::V6(v6), prefix),
        },
    };

    match prefix {
        Some(width) if width > if addr.is_ipv4() { 32 } else { 128 } => None,
        Some(width) => Some(format!("{addr}/{width}")),
        None => Some(addr.to_string()),
    }
}

pub fn canonical_hostname(value: &str) -> Option<String> {
    let trimmed = value.trim_end_matches('.');
    (!trimmed.is_empty()).then(|| trimmed.to_ascii_lowercase())
}
