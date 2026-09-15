use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

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

pub fn parse_address(value: &str) -> Option<(IpAddr, Option<u8>)> {
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
        Some(width) if width > address_bits(&addr) => None,
        _ => Some((addr, prefix)),
    }
}

pub fn canonical_address(value: &str) -> Option<String> {
    match parse_address(value)? {
        (addr, Some(width)) => Some(format!("{addr}/{width}")),
        (addr, None) => Some(addr.to_string()),
    }
}

pub fn address_bits(addr: &IpAddr) -> u8 {
    if addr.is_ipv4() { 32 } else { 128 }
}

pub fn mask_to_prefix(addr: &IpAddr, prefix: u8) -> Option<IpAddr> {
    if prefix > address_bits(addr) {
        return None;
    }
    Some(match addr {
        IpAddr::V4(v4) => {
            let mask = u32::MAX.checked_shl(u32::from(32 - prefix)).unwrap_or(0);
            IpAddr::V4(Ipv4Addr::from(u32::from(*v4) & mask))
        }
        IpAddr::V6(v6) => {
            let mask = u128::MAX.checked_shl(u32::from(128 - prefix)).unwrap_or(0);
            IpAddr::V6(Ipv6Addr::from(u128::from(*v6) & mask))
        }
    })
}

pub fn canonical_hostname(value: &str) -> Option<String> {
    let trimmed = value.trim_end_matches('.');
    (!trimmed.is_empty()).then(|| trimmed.to_ascii_lowercase())
}
