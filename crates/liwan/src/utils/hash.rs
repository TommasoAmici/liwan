use anyhow::Context;
use argon2::Argon2;
use argon2::PasswordVerifier;
use argon2::password_hash::PasswordHasher;

use anyhow::Result;
use rand::distr::Alphanumeric;
use rand::distr::SampleString;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use subtle::ConstantTimeEq;

pub fn hash_password(password: &str) -> Result<String> {
    let hash = Argon2::default().hash_password(password.as_bytes()).context("Failed to hash password")?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> Result<()> {
    let hash = argon2::PasswordHash::new(hash).context("Invalid hash")?;
    let argon2 = Argon2::default();
    argon2.verify_password(password.as_bytes(), &hash).context("Failed to verify password")
}

pub fn visitor_group_id(ip: &IpAddr, user_agent: &str, daily_salt: &str, entity_id: &str) -> String {
    let mut buffer = [0u8; 64];
    let ip = format_ip(*ip, &mut buffer);
    hash_visitor_group(&[ip, user_agent.as_bytes(), daily_salt.as_bytes(), entity_id.as_bytes()])
}

pub fn visitor_group_id_cidr(
    ip: &IpAddr,
    ipv4_prefix: u8,
    ipv6_prefix: u8,
    daily_salt: &str,
    entity_id: &str,
) -> String {
    let masked_ip = match ip {
        IpAddr::V4(ip) => IpAddr::V4(mask_ipv4(*ip, ipv4_prefix)),
        IpAddr::V6(ip) => IpAddr::V6(mask_ipv6(*ip, ipv6_prefix)),
    };
    let mut buffer = [0u8; 64];
    let ip = format_ip(masked_ip, &mut buffer);
    hash_visitor_group(&[ip, daily_salt.as_bytes(), entity_id.as_bytes()])
}

/// Format an IP the way `Display` does, without the heap allocation. The hash
/// input must stay byte-identical to the previous `to_string()` form.
fn format_ip(ip: IpAddr, buffer: &mut [u8; 64]) -> &[u8] {
    use std::io::Write;
    let mut cursor = std::io::Cursor::new(&mut buffer[..]);
    write!(cursor, "{ip}").expect("an IP address fits in 64 bytes");
    let written = cursor.position() as usize;
    &buffer[..written]
}

fn hash_visitor_group(parts: &[&[u8]]) -> String {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let mut hasher = blake3::Hasher::new();
    for part in parts {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    let hash = hasher.finalize();
    URL_SAFE_NO_PAD.encode(&hash.as_bytes()[..12])
}

fn mask_ipv4(ip: Ipv4Addr, prefix: u8) -> Ipv4Addr {
    let prefix = prefix.min(32);
    let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
    Ipv4Addr::from(u32::from(ip) & mask)
}

fn mask_ipv6(ip: Ipv6Addr, prefix: u8) -> Ipv6Addr {
    let prefix = prefix.min(128);
    let mask = if prefix == 0 { 0 } else { u128::MAX << (128 - prefix) };
    Ipv6Addr::from(u128::from(ip) & mask)
}

pub fn visitor_group_id_fallback() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 16)
}

pub fn session_token() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 32)
}

/// Compare onboarding tokens without short-circuiting on their contents.
pub(crate) fn onboarding_token_matches(expected: &str, candidate: &str) -> bool {
    bool::from(expected.as_bytes().ct_eq(candidate.as_bytes()))
}

pub fn onboarding_token() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 16)
}

pub fn db_name() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_ip_matches_display_for_both_families() {
        for address in ["1.1.1.1", "255.255.255.255", "2001:db8::1", "::1", "::ffff:10.0.0.1"] {
            let ip: IpAddr = address.parse().unwrap();
            let mut buffer = [0u8; 64];
            assert_eq!(format_ip(ip, &mut buffer), ip.to_string().as_bytes(), "{address}");
        }
    }

    #[test]
    fn visitor_group_ids_hash_the_display_form_of_the_ip() {
        for address in ["203.0.113.10", "2001:db8::1"] {
            let ip: IpAddr = address.parse().unwrap();
            let expected = hash_visitor_group(&[ip.to_string().as_bytes(), b"user-agent", b"daily-salt", b"entity"]);
            assert_eq!(
                visitor_group_id(&ip, "user-agent", "daily-salt", "entity"),
                expected,
                "the hash input changed for {address}"
            );
        }
    }

    #[test]
    fn masked_visitor_group_ids_hash_the_masked_address() {
        let ip: IpAddr = "203.0.113.10".parse().unwrap();
        let expected = hash_visitor_group(&[b"203.0.113.0", b"daily-salt", b"entity"]);
        assert_eq!(visitor_group_id_cidr(&ip, 24, 56, "daily-salt", "entity"), expected);
    }
}
