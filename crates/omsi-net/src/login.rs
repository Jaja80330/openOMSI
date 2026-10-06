//! A server's own login (a web page that knows its players - the NEROSY server's Leitstelle,
//! through Discord): the server lets in only games that bring a token it signed, and the
//! player is called there by the name in the token.
//!
//! The server says where the login is in its status (`ServerInfo::login`, `GET /status`); the
//! launcher has the player log in there (in the browser) and passes the token to the game
//! (`OMSI_JOIN_TOKEN`), which sends it in its `HELLO`. A token is
//! `v1.<account>.<name, hex of its UTF-8>.<expiry, Unix s>.<HMAC-SHA256 of all before it, hex>`,
//! signed with the key the server and its login share (`join_key` of server.cfg).

/// What a valid token says.
#[derive(Debug, Clone, PartialEq)]
pub struct Login {
    /// The account it is for (the login's own id: a Discord user id).
    pub account: String,
    /// The name the player has on the server.
    pub name: String,
    pub expires: u64,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn sign(key: &[u8], body: &str) -> String {
    let k = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    hex(ring::hmac::sign(&k, body.as_bytes()).as_ref())
}

/// A token for `account` called `name` until `expires` (what the login issues; the tests).
pub fn issue(key: &[u8], account: &str, name: &str, expires: u64) -> String {
    let body = format!("v1.{account}.{}.{expires}", hex(name.as_bytes()));
    let sig = sign(key, &body);
    format!("{body}.{sig}")
}

/// Why a token does not let a player in.
#[derive(Debug, Clone, PartialEq)]
pub enum Refused {
    Missing,
    Invalid,
    Expired,
}

/// Check a token with the key at `now` (Unix s).
pub fn check(key: &[u8], token: &str, now: u64) -> Result<Login, Refused> {
    let token = token.trim();
    if token.is_empty() {
        return Err(Refused::Missing);
    }
    let (body, sig) = token.rsplit_once('.').ok_or(Refused::Invalid)?;
    let k = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    let sig = unhex(sig).ok_or(Refused::Invalid)?;
    ring::hmac::verify(&k, body.as_bytes(), &sig).map_err(|_| Refused::Invalid)?;
    let mut it = body.split('.');
    if it.next() != Some("v1") {
        return Err(Refused::Invalid);
    }
    let account = it.next().filter(|a| !a.is_empty()).ok_or(Refused::Invalid)?.to_string();
    let name = unhex(it.next().ok_or(Refused::Invalid)?).and_then(|b| String::from_utf8(b).ok()).ok_or(Refused::Invalid)?;
    let expires: u64 = it.next().and_then(|e| e.parse().ok()).ok_or(Refused::Invalid)?;
    if it.next().is_some() {
        return Err(Refused::Invalid);
    }
    if expires <= now {
        return Err(Refused::Expired);
    }
    Ok(Login { account, name, expires })
}

/// The key of server.cfg's `join_key` (hex, 32 bytes or more).
pub fn key_of(text: &str) -> Option<Vec<u8>> {
    unhex(text.trim()).filter(|k| k.len() >= 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_signed_with_the_key_lets_in_and_names_the_player() {
        let key = b"0123456789abcdef0123456789abcdef";
        let t = issue(key, "123456789", "Jérôme | TLA", 2_000_000_000);
        assert_eq!(check(key, &t, 1_900_000_000), Ok(Login { account: "123456789".into(), name: "Jérôme | TLA".into(), expires: 2_000_000_000 }));
        // (no '|' in the token itself: it goes in a datagram of '|'-separated fields)
        assert!(!t.contains('|'));
        assert_eq!(check(key, &t, 2_000_000_000), Err(Refused::Expired));
        assert_eq!(check(b"another key, another server.....", &t, 0), Err(Refused::Invalid));
        // another name in the same token: the signature no longer holds
        let forged = t.replacen(&hex("Jérôme | TLA".as_bytes()), &hex(b"Admin"), 1);
        assert_eq!(check(key, &forged, 0), Err(Refused::Invalid));
        assert_eq!(check(key, "", 0), Err(Refused::Missing));
        assert_eq!(check(key, "v1.x.y", 0), Err(Refused::Invalid));
        assert_eq!(key_of("00112233445566778899aabbccddeeff").map(|k| k.len()), Some(16));
        // a token as the NEROSY Leitstelle signs it (Python's hmac): taken
        let py = "v1.42.4a65616e20544c41.4000000000.86f45519de576f7af4e1c331cf2783d0b84cfb5c130b1fe4e4dd06725f177e81";
        assert_eq!(check(key, py, 0).map(|l| l.name), Ok("Jean TLA".to_string()));
        assert_eq!(issue(key, "42", "Jean TLA", 4_000_000_000), py);
        assert!(key_of("short").is_none());
    }
}
