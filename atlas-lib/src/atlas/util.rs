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
