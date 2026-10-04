//! Signed messages between the two Macs.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

const FRAME_LIMIT: usize = 16 * 1024;

#[derive(Clone)]
pub struct Identity {
    signing: SigningKey,
}

impl Identity {
    pub fn generate() -> Self {
        Self {
            signing: SigningKey::generate(&mut OsRng),
        }
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    pub fn load_or_create(path: &Path) -> Self {
        if let Ok(text) = fs::read_to_string(path) {
            if let Some(identity) = text_to_identity(&text) {
                return identity;
            }
            eprintln!("hop: identity file did not parse, creating a new one");
        }
        let identity = Self::generate();
        if let Err(err) = identity.save(path) {
            eprintln!("hop: could not save the identity ({err})");
        }
        identity
    }

    fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&IdentityFile {
            secret: hex(&self.signing.to_bytes()),
        })
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, text + "\n")?;
        fs::rename(&tmp, path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct IdentityFile {
    secret: String,
}

fn text_to_identity(text: &str) -> Option<Identity> {
    let file: IdentityFile = serde_json::from_str(text).ok()?;
    let secret = parse_key(&file.secret)?;
    Some(Identity {
        signing: SigningKey::from_bytes(&secret),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Body {
    pub op: String,
    pub id: String,
    pub exp: u64,
    #[serde(default)]
    pub addresses: Vec<String>,
}

pub enum Reply {
    Message(&'static str),
    Connect,
}

pub fn canon(address: &str) -> String {
    let hex: String = address
        .chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .map(|ch| ch.to_ascii_lowercase())
        .collect();
    if hex.is_empty() || hex.len() % 2 != 0 {
        return String::new();
    }
    let mut out = String::with_capacity(hex.len() / 2 * 3);
    for (index, ch) in hex.chars().enumerate() {
        if index > 0 && index % 2 == 0 {
            out.push('-');
        }
        out.push(ch);
    }
    out
}

fn listed(paired: &HashSet<String>, addresses: &[String]) -> bool {
    !addresses.is_empty()
        && addresses.iter().all(|address| {
            let address = canon(address);
            !address.is_empty() && paired.iter().any(|have| canon(have) == address)
        })
}

pub fn reply(
    request_op: &str,
    allowed: bool,
    fresh: bool,
    paired: &HashSet<String>,
    addresses: &[String],
) -> Reply {
    let known = listed(paired, addresses);
    match request_op {
        "take" if !allowed || !fresh || addresses.is_empty() => Reply::Message("refuse"),
        "take" if !known => Reply::Message("unpaired"),
        "take" => Reply::Message("accept"),
        "released" if allowed && fresh && known => Reply::Connect,
        "released" => Reply::Message("failed"),
        _ => Reply::Message("refuse"),
    }
}

pub fn fresh(exp: u64, now: u64) -> bool {
    exp + 5 >= now && exp <= now + 60
}

pub fn seal(identity: &Identity, body: &Body) -> Vec<u8> {
    let json = serde_json::to_vec(body).unwrap_or_default();
    let signature = identity.signing.sign(&json);
    let mut out = Vec::with_capacity(32 + 64 + json.len());
    out.extend_from_slice(&identity.public_key());
    out.extend_from_slice(&signature.to_bytes());
    out.extend_from_slice(&json);
    out
}

pub fn unseal(bytes: &[u8]) -> Option<([u8; 32], Body)> {
    if bytes.len() < 96 {
        return None;
    }
    let key_bytes: [u8; 32] = bytes[..32].try_into().ok()?;
    let sig_bytes: [u8; 64] = bytes[32..96].try_into().ok()?;
    let json = &bytes[96..];
    let key = VerifyingKey::from_bytes(&key_bytes).ok()?;
    let signature = Signature::from_bytes(&sig_bytes);
    key.verify(json, &signature).ok()?;
    let body = serde_json::from_slice(json).ok()?;
    Some((key_bytes, body))
}

pub fn write_frame(stream: &mut impl Write, sealed: &[u8]) -> io::Result<()> {
    if sealed.len() > FRAME_LIMIT {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "long"));
    }
    stream.write_all(&(sealed.len() as u32).to_be_bytes())?;
    stream.write_all(sealed)?;
    stream.flush()
}

pub fn read_frame(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > FRAME_LIMIT {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "long"));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

pub fn parse_key(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (index, chunk) in text.as_bytes().chunks(2).enumerate() {
        let digits = std::str::from_utf8(chunk).ok()?;
        out[index] = u8::from_str_radix(digits, 16).ok()?;
    }
    Some(out)
}

pub fn new_id() -> String {
    let mut bytes = [0u8; 16];
    rand::RngCore::fill_bytes(&mut OsRng, &mut bytes);
    hex(&bytes)
}

pub fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{
        Body, Identity, Reply, canon, fresh, hex, parse_key, read_frame, reply, seal, unseal,
        write_frame,
    };
    use std::collections::HashSet;
    use std::net::TcpListener;
    use std::thread;

    fn paired(address: &str) -> HashSet<String> {
        HashSet::from([address.to_string()])
    }

    #[test]
    fn a_stranger_is_refused_and_bluetooth_is_not_asked_to_connect() {
        let mouse = paired("aa-bb");
        assert!(matches!(
            reply("take", false, true, &mouse, &["aa-bb".into()]),
            Reply::Message("refuse")
        ));
        assert!(matches!(
            reply("released", false, true, &mouse, &["aa-bb".into()]),
            Reply::Message("failed")
        ));
    }

    #[test]
    fn an_allowed_mac_is_accepted_only_for_paired_devices() {
        let mouse = paired("aa-bb");
        assert!(matches!(
            reply("take", true, true, &mouse, &["aa-bb".into()]),
            Reply::Message("accept")
        ));
        assert!(matches!(
            reply("take", true, true, &mouse, &["cc-dd".into()]),
            Reply::Message("unpaired")
        ));
        assert!(matches!(
            reply("take", true, true, &mouse, &[]),
            Reply::Message("refuse")
        ));
        assert!(matches!(
            reply("released", true, true, &mouse, &["aa-bb".into()]),
            Reply::Connect
        ));
        let mouse = paired("aa-bb-cc-dd-ee-ff");
        assert!(matches!(
            reply("take", true, true, &mouse, &["AA:BB:CC:DD:EE:FF".into()]),
            Reply::Message("accept")
        ));
    }

    #[test]
    fn an_old_or_replayed_request_is_refused() {
        let mouse = paired("aa-bb");
        assert!(matches!(
            reply("take", true, false, &mouse, &["aa-bb".into()]),
            Reply::Message("refuse")
        ));
        assert!(fresh(100, 100));
        assert!(!fresh(100, 106));
        assert!(!fresh(200, 100));
    }

    #[test]
    fn a_changed_signature_does_not_unseal() {
        let identity = Identity::generate();
        let body = Body {
            op: "take".into(),
            id: "1".into(),
            exp: 10,
            addresses: vec!["aa-bb".into()],
        };
        let mut sealed = seal(&identity, &body);
        let last = sealed.len() - 1;
        sealed[last] ^= 0xff;
        assert!(unseal(&sealed).is_none());
        sealed[last] ^= 0xff;
        let (key, opened) = unseal(&sealed).unwrap();
        assert_eq!(key, identity.public_key());
        assert_eq!(opened.op, "take");
    }

    #[test]
    fn a_frame_round_trips_between_two_sockets() {
        let server_key = Identity::generate();
        let client_key = Identity::generate();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let expected = client_key.public_key();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let bytes = read_frame(&mut stream).unwrap();
            let (key, body) = unseal(&bytes).unwrap();
            assert_eq!(key, expected);
            assert_eq!(body.addresses, vec!["aa-bb".to_string()]);
            let reply = Body {
                op: "refuse".into(),
                id: body.id,
                exp: body.exp,
                addresses: Vec::new(),
            };
            write_frame(&mut stream, &seal(&server_key, &reply)).unwrap();
        });
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = Body {
            op: "take".into(),
            id: "abc".into(),
            exp: 30,
            addresses: vec!["aa-bb".into()],
        };
        write_frame(&mut stream, &seal(&client_key, &body)).unwrap();
        let (key, reply) = unseal(&read_frame(&mut stream).unwrap()).unwrap();
        assert_eq!(reply.op, "refuse");
        assert_eq!(reply.id, "abc");
        assert_ne!(key, client_key.public_key());
        server.join().unwrap();
    }

    #[test]
    fn addresses_match_with_colons_or_dashes() {
        assert_eq!(canon("AA:BB:CC:DD:EE:FF"), "aa-bb-cc-dd-ee-ff");
        assert_eq!(canon("aa-bb-cc-dd-ee-ff"), "aa-bb-cc-dd-ee-ff");
        assert_eq!(canon("not a device"), "");
    }

    #[test]
    fn hex_keys_round_trip() {
        let key = Identity::generate().public_key();
        assert_eq!(parse_key(&hex(&key)), Some(key));
        assert_eq!(parse_key("zz"), None);
    }
}
