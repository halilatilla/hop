//! Finds the other Mac on the local network and carries a signed send.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{self, ErrorKind};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::handoff::Peer;
use crate::wire::{self, Body, Identity, Reply};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nearby {
    pub id: String,
    pub name: String,
    pub code: String,
}

#[derive(Clone, Debug)]
pub struct Target {
    pub key: [u8; 32],
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    None,
    Nearby,
    Ready,
    Crowd,
}

struct Sight {
    instance: String,
    key: Option<[u8; 32]>,
    host: String,
    port: u16,
    resolved: bool,
}

struct World {
    identity: Identity,
    allowed: HashMap<[u8; 32], String>,
    paired: HashSet<String>,
    shared: HashSet<String>,
    incoming_add: Vec<(String, String)>,
    incoming_remove: Vec<String>,
    acked_removals: Vec<String>,
    seen_ids: VecDeque<String>,
    sights: Vec<Sight>,
}

static MOVING: AtomicBool = AtomicBool::new(false);

struct MoveGuard;

impl Drop for MoveGuard {
    fn drop(&mut self) {
        MOVING.store(false, Ordering::Release);
    }
}

fn begin_move() -> Option<MoveGuard> {
    MOVING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .ok()
        .map(|_| MoveGuard)
}

static WORLD: OnceLock<Arc<Mutex<World>>> = OnceLock::new();

pub fn start() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let dir = crate::choice::config_dir();
        let identity = Identity::load_or_create(&dir.join("identity.json"));
        let allowed = load_peers(&dir.join("peers.json"));
        let world = Arc::new(Mutex::new(World {
            identity,
            allowed,
            paired: HashSet::new(),
            shared: HashSet::new(),
            incoming_add: Vec::new(),
            incoming_remove: Vec::new(),
            acked_removals: Vec::new(),
            seen_ids: VecDeque::new(),
            sights: Vec::new(),
        }));
        let _ = WORLD.set(world.clone());
        #[cfg(target_os = "macos")]
        {
            let _ = thread::Builder::new()
                .name("hop-link".into())
                .spawn(move || serve(world));
        }
    });
}

pub fn seen() -> Seen {
    let Some(world) = world() else {
        return Seen::None;
    };
    let Ok(world) = world.lock() else {
        return Seen::None;
    };
    let self_key = world.identity.public_key();
    let mut ready = 0;
    let mut nearby = false;
    for sight in &world.sights {
        let Some(key) = sight.key else {
            continue;
        };
        if !sight.resolved || key == self_key {
            continue;
        }
        if world.allowed.contains_key(&key) {
            ready += 1;
        } else {
            nearby = true;
        }
    }
    if ready == 1 {
        Seen::Ready
    } else if ready > 1 {
        Seen::Crowd
    } else if nearby {
        Seen::Nearby
    } else {
        Seen::None
    }
}

pub fn this_mac() -> String {
    #[cfg(target_os = "macos")]
    {
        computer_name()
    }
    #[cfg(not(target_os = "macos"))]
    {
        "This Mac".to_string()
    }
}

pub fn other_mac() -> String {
    let Some(world) = world() else {
        return "The other Mac".to_string();
    };
    let Ok(world) = world.lock() else {
        return "The other Mac".to_string();
    };
    let self_key = world.identity.public_key();
    let mut found = None;
    for sight in &world.sights {
        let Some(key) = sight.key else {
            continue;
        };
        if !sight.resolved || key == self_key || !world.allowed.contains_key(&key) {
            continue;
        }
        if found.is_some() {
            return "The other Mac".to_string();
        }
        let name = if sight.instance.is_empty() {
            world
                .allowed
                .get(&key)
                .cloned()
                .unwrap_or_else(|| "The other Mac".to_string())
        } else {
            sight.instance.clone()
        };
        found = Some(name);
    }
    found
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "The other Mac".to_string())
}

pub fn peer() -> Peer {
    if seen() == Seen::Ready {
        Peer::Ready
    } else {
        Peer::Missing
    }
}

pub fn pending() -> Vec<Nearby> {
    let Some(world) = world() else {
        return Vec::new();
    };
    let Ok(world) = world.lock() else {
        return Vec::new();
    };
    let self_key = world.identity.public_key();
    world
        .sights
        .iter()
        .filter_map(|sight| {
            let key = sight.key?;
            if !sight.resolved || key == self_key || world.allowed.contains_key(&key) {
                return None;
            }
            let name = if sight.instance.is_empty() {
                "Mac".to_string()
            } else {
                sight.instance.clone()
            };
            Some(Nearby {
                id: wire::hex(&key),
                name,
                code: wire::pair_code(&self_key, &key),
            })
        })
        .collect()
}

pub fn allow(id: &str) {
    let Some(key) = wire::parse_key(id) else {
        return;
    };
    let Some(world) = world() else {
        return;
    };
    let Ok(mut world) = world.lock() else {
        return;
    };
    if key == world.identity.public_key() {
        return;
    }
    let name = world
        .sights
        .iter()
        .find(|sight| sight.key == Some(key))
        .map(|sight| sight.instance.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Mac".to_string());
    world.allowed.insert(key, name);
    let peers = saved_from(&world.allowed);
    if let Err(err) = save_peers(&peers_path(), &peers) {
        eprintln!("hop: could not save the allowed Mac ({err})");
    }
}

const FORGET_FAILED: &str = "Hop could not forget this Mac.";

pub fn forgettable() -> Option<String> {
    let world = world()?;
    let world = world.lock().ok()?;
    let self_key = world.identity.public_key();
    let mut resolved = Vec::new();
    for sight in &world.sights {
        let Some(key) = sight.key else {
            continue;
        };
        if !sight.resolved || key == self_key || !world.allowed.contains_key(&key) {
            continue;
        }
        resolved.push(key);
    }
    let key = if resolved.len() == 1 {
        resolved[0]
    } else if resolved.is_empty() && world.allowed.len() == 1 {
        *world.allowed.keys().next()?
    } else {
        return None;
    };
    Some(wire::hex(&key))
}

pub fn forget(id: &str) -> Result<(), &'static str> {
    let Some(world) = world() else {
        return Err(FORGET_FAILED);
    };
    let Ok(mut world) = world.lock() else {
        return Err(FORGET_FAILED);
    };
    revoke(&peers_path(), &mut world.allowed, id)
}

pub fn set_paired(addresses: HashSet<String>) {
    let Some(world) = world() else {
        return;
    };
    if let Ok(mut world) = world.lock() {
        world.paired = addresses
            .into_iter()
            .map(|address| wire::canon(&address))
            .filter(|address| !address.is_empty())
            .collect();
    }
}

pub fn target() -> Option<Target> {
    let world = world()?;
    let world = world.lock().ok()?;
    let self_key = world.identity.public_key();
    let mut found = None;
    for sight in &world.sights {
        let Some(key) = sight.key else {
            continue;
        };
        if !sight.resolved || key == self_key || !world.allowed.contains_key(&key) {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(Target {
            key,
            host: sight.host.clone(),
            port: sight.port,
        });
    }
    found
}

pub fn set_shared(addresses: HashSet<String>) {
    let Some(world) = world() else {
        return;
    };
    if let Ok(mut world) = world.lock() {
        let mut shared: HashSet<String> = addresses
            .into_iter()
            .map(|address| wire::canon(&address))
            .filter(|address| !address.is_empty())
            .collect();
        for (address, _) in &world.incoming_add {
            shared.insert(address.clone());
        }
        world.shared = shared;
    }
}

pub struct Incoming {
    pub add: Vec<(String, String)>,
    pub remove: Vec<String>,
}

pub fn take_incoming() -> Incoming {
    let Some(world) = world() else {
        return Incoming {
            add: Vec::new(),
            remove: Vec::new(),
        };
    };
    let Ok(mut world) = world.lock() else {
        return Incoming {
            add: Vec::new(),
            remove: Vec::new(),
        };
    };
    Incoming {
        add: std::mem::take(&mut world.incoming_add),
        remove: std::mem::take(&mut world.incoming_remove),
    }
}

pub fn take_acked() -> Vec<String> {
    let Some(world) = world() else {
        return Vec::new();
    };
    world
        .lock()
        .map(|mut world| std::mem::take(&mut world.acked_removals))
        .unwrap_or_default()
}

pub fn announce(add: Vec<String>, names: Vec<String>, remove: Vec<String>) {
    if add.is_empty() && remove.is_empty() {
        return;
    }
    thread::spawn(move || {
        let Some(target) = target() else {
            return;
        };
        let mut acked = Vec::new();
        if !remove.is_empty() {
            if let Ok(reply) = message(&target, "unshare", &remove, &[], Duration::from_secs(5)) {
                if reply.op == "kept" {
                    acked = remove;
                }
            }
        }
        if !add.is_empty() {
            let _ = message(&target, "share", &add, &names, Duration::from_secs(5));
        }
        if acked.is_empty() {
            return;
        }
        if let Some(world) = world() {
            if let Ok(mut world) = world.lock() {
                world.acked_removals.extend(acked);
            }
        }
    });
}

pub fn claim(target: Target, addresses: Vec<String>) -> crate::handoff::Outcome {
    use crate::handoff::{Outcome, StayReason, WantResult};

    let addresses: Vec<String> = addresses
        .iter()
        .map(|address| wire::canon(address))
        .filter(|address| !address.is_empty())
        .collect();
    if addresses.is_empty() {
        return Outcome::Stayed(StayReason::NothingThere);
    }
    let Some(_guard) = begin_move() else {
        return Outcome::Stayed(StayReason::Busy);
    };
    let Some(world) = world() else {
        return Outcome::Stayed(StayReason::CouldNotReach);
    };
    let Ok(identity) = world.lock().map(|world| world.identity.clone()) else {
        return Outcome::Stayed(StayReason::CouldNotReach);
    };
    let _ = message(&target, "share", &addresses, &[], Duration::from_secs(5));
    let id = wire::new_id();
    let body = Body {
        op: "want".to_string(),
        id: id.clone(),
        exp: wire::now_secs().saturating_add(30),
        addresses: addresses.clone(),
        names: Vec::new(),
    };
    let mut stream = match connect_host(&target.host, target.port) {
        Ok(stream) => stream,
        Err(_) => return Outcome::Stayed(StayReason::CouldNotReach),
    };
    nosigpipe(&stream);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    if wire::write_frame(&mut stream, &wire::seal(&identity, &body)).is_err() {
        return Outcome::Stayed(StayReason::CouldNotReach);
    }
    let reply_bytes = match wire::read_frame(&mut stream) {
        Ok(bytes) => bytes,
        Err(err) => return Outcome::Stayed(crate::handoff::stay_after_read(err.kind())),
    };
    let Some((key, reply)) = wire::unseal(&reply_bytes) else {
        return Outcome::Stayed(StayReason::BadReply);
    };
    if key != target.key || reply.id != id {
        return Outcome::Stayed(StayReason::BadReply);
    }
    match crate::handoff::after_want(Some(reply.op.as_str())) {
        WantResult::Stay(reason) => Outcome::Stayed(reason),
        WantResult::Local => {
            if crate::bluetooth::connect_all(&addresses) {
                Outcome::Moved(addresses)
            } else {
                Outcome::Stayed(StayReason::NothingThere)
            }
        }
        WantResult::Connect => {
            let got: Vec<String> = reply
                .addresses
                .iter()
                .map(|address| wire::canon(address))
                .filter(|address| !address.is_empty())
                .collect();
            let got = if got.is_empty() {
                addresses.clone()
            } else {
                got
            };
            let ok = crate::bluetooth::connect_all(&got);
            let ack = Body {
                op: if ok { "took" } else { "failed" }.to_string(),
                id,
                exp: reply.exp,
                addresses: Vec::new(),
                names: Vec::new(),
            };
            let _ = wire::write_frame(&mut stream, &wire::seal(&identity, &ack));
            let rest: Vec<String> = addresses
                .into_iter()
                .filter(|address| !got.contains(address))
                .collect();
            if !rest.is_empty() {
                let _ = crate::bluetooth::connect_all(&rest);
            }
            if ok {
                Outcome::Moved(got)
            } else {
                Outcome::Stayed(StayReason::StayedThere)
            }
        }
    }
}

fn message(
    target: &Target,
    op: &str,
    addresses: &[String],
    names: &[String],
    timeout: Duration,
) -> Result<Body, String> {
    let world = world().ok_or_else(|| "Hop is not listening.".to_string())?;
    let identity = world
        .lock()
        .map_err(|_| "Hop could not read its key.".to_string())?
        .identity
        .clone();
    let id = wire::new_id();
    let body = Body {
        op: op.to_string(),
        id: id.clone(),
        exp: wire::now_secs().saturating_add(20),
        addresses: addresses.to_vec(),
        names: names.to_vec(),
    };
    let mut stream = connect_host(&target.host, target.port)?;
    nosigpipe(&stream);
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    wire::write_frame(&mut stream, &wire::seal(&identity, &body)).map_err(|err| err.to_string())?;
    let reply_bytes = wire::read_frame(&mut stream).map_err(|err| err.to_string())?;
    let (key, reply) =
        wire::unseal(&reply_bytes).ok_or_else(|| "The other Mac sent a bad reply.".to_string())?;
    if key != target.key || reply.id != id {
        return Err("The other Mac sent an unexpected reply.".into());
    }
    Ok(reply)
}

fn world() -> Option<Arc<Mutex<World>>> {
    WORLD.get().cloned()
}

fn peers_path() -> PathBuf {
    crate::choice::config_dir().join("peers.json")
}

fn handle_client(mut stream: TcpStream, world: Arc<Mutex<World>>) {
    nosigpipe(&stream);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let Ok(bytes) = wire::read_frame(&mut stream) else {
        return;
    };
    let Some((key, body)) = wire::unseal(&bytes) else {
        return;
    };
    let (identity, decision) = {
        let Ok(mut world) = world.lock() else {
            return;
        };
        let fresh = wire::fresh(body.exp, wire::now_secs()) && world.remember(&body.id);
        let allowed = world.allowed.contains_key(&key);
        let decision = wire::reply(&body.op, allowed, fresh, &world.paired, &body.addresses);
        (world.identity.clone(), decision)
    };
    if matches!(decision, Reply::Message("kept")) {
        record_list(&world, &body);
    }
    if matches!(decision, Reply::Give) {
        let (shared, pending) = world
            .lock()
            .map(|world| {
                let pending = world
                    .incoming_add
                    .iter()
                    .map(|(address, _)| address.clone())
                    .collect();
                (world.shared.clone(), pending)
            })
            .unwrap_or_else(|_| (HashSet::new(), HashSet::new()));
        give_devices(&mut stream, &identity, &key, &body, &shared, &pending);
        return;
    }
    let op = match decision {
        Reply::Message(op) => op,
        Reply::Connect => {
            if crate::bluetooth::connect_all(&body.addresses) {
                "took"
            } else {
                "failed"
            }
        }
        Reply::Give => return,
    };
    let reply = Body {
        op: op.to_string(),
        id: body.id,
        exp: body.exp,
        addresses: Vec::new(),
        names: Vec::new(),
    };
    let _ = wire::write_frame(&mut stream, &wire::seal(&identity, &reply));
}

fn record_list(world: &Arc<Mutex<World>>, body: &Body) {
    let Ok(mut world) = world.lock() else {
        return;
    };
    match body.op.as_str() {
        "share" => {
            for (index, address) in body.addresses.iter().enumerate() {
                let address = wire::canon(address);
                if address.is_empty() {
                    continue;
                }
                let name = body
                    .names
                    .get(index)
                    .map(|name| name.trim().to_string())
                    .unwrap_or_default();
                world.incoming_add.push((address, name));
            }
        }
        "unshare" => {
            for address in &body.addresses {
                let address = wire::canon(address);
                if !address.is_empty() {
                    world.incoming_remove.push(address);
                }
            }
        }
        _ => {}
    }
}

fn give_devices(
    stream: &mut TcpStream,
    identity: &Identity,
    key: &[u8; 32],
    body: &Body,
    shared: &HashSet<String>,
    pending: &HashSet<String>,
) {
    let wanted: Vec<String> = body
        .addresses
        .iter()
        .map(|address| wire::canon(address))
        .filter(|address| {
            !address.is_empty() && (shared.contains(address) || pending.contains(address))
        })
        .collect();
    let held = crate::bluetooth::connected_ones(&wanted);
    if held.is_empty() {
        let _ = write_reply(stream, identity, "absent", &body.id, body.exp, &[]);
        return;
    }
    let Some(_guard) = begin_move() else {
        let _ = write_reply(stream, identity, "busy", &body.id, body.exp, &[]);
        return;
    };
    if !crate::bluetooth::release_all(&held) {
        let _ = crate::bluetooth::connect_all(&held);
        let _ = write_reply(stream, identity, "still", &body.id, body.exp, &[]);
        return;
    }
    if write_reply(stream, identity, "released", &body.id, body.exp, &held).is_err() {
        let _ = crate::bluetooth::connect_all(&held);
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(70)));
    if !read_took(stream, key, &body.id) {
        let _ = crate::bluetooth::connect_all(&held);
    }
}

fn write_reply(
    stream: &mut TcpStream,
    identity: &Identity,
    op: &str,
    id: &str,
    exp: u64,
    addresses: &[String],
) -> io::Result<()> {
    let reply = Body {
        op: op.to_string(),
        id: id.to_string(),
        exp,
        addresses: addresses.to_vec(),
        names: Vec::new(),
    };
    wire::write_frame(stream, &wire::seal(identity, &reply))
}

fn read_took(stream: &mut TcpStream, key: &[u8; 32], id: &str) -> bool {
    let Ok(bytes) = wire::read_frame(stream) else {
        return false;
    };
    let Some((reply_key, reply)) = wire::unseal(&bytes) else {
        return false;
    };
    reply_key == *key && reply.id == id && reply.op == "took"
}

impl World {
    fn remember(&mut self, id: &str) -> bool {
        if self.seen_ids.iter().any(|seen| seen == id) {
            return false;
        }
        self.seen_ids.push_back(id.to_string());
        while self.seen_ids.len() > 64 {
            self.seen_ids.pop_front();
        }
        true
    }
}

fn connect_host(host: &str, port: u16) -> Result<TcpStream, String> {
    let mut addrs = (host, port)
        .to_socket_addrs()
        .map_err(|err| err.to_string())?
        .collect::<Vec<_>>();
    addrs.sort_by_key(|addr| addr.is_ipv6());
    let mut last = "could not connect".to_string();
    let mut tried = false;
    for addr in addrs {
        tried = true;
        match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
            Ok(stream) => return Ok(stream),
            Err(err) => last = err.to_string(),
        }
    }
    if tried {
        Err(last)
    } else {
        Err(format!("could not find {host}"))
    }
}

fn nosigpipe(stream: &TcpStream) {
    #[cfg(target_os = "macos")]
    unsafe {
        let yes: i32 = 1;
        setsockopt(
            stream.as_raw_fd(),
            0xffff,
            0x1022,
            &yes as *const i32 as *const LibcVoid,
            4,
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = stream;
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct SavedPeer {
    key: String,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct PeersFile {
    #[serde(default)]
    peers: Vec<SavedPeer>,
}

fn load_peers(path: &Path) -> HashMap<[u8; 32], String> {
    let Ok(text) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(file) = serde_json::from_str::<PeersFile>(&text) else {
        eprintln!("hop: allowed Macs file did not parse");
        return HashMap::new();
    };
    file.peers
        .into_iter()
        .filter_map(|peer| Some((wire::parse_key(&peer.key)?, peer.name)))
        .collect()
}

fn saved_from(allowed: &HashMap<[u8; 32], String>) -> Vec<SavedPeer> {
    allowed
        .iter()
        .map(|(key, name)| SavedPeer {
            key: wire::hex(key),
            name: name.clone(),
        })
        .collect()
}

fn revoke(
    path: &Path,
    allowed: &mut HashMap<[u8; 32], String>,
    id: &str,
) -> Result<(), &'static str> {
    let Some(key) = wire::parse_key(id) else {
        return Err(FORGET_FAILED);
    };
    if !allowed.contains_key(&key) {
        return Err(FORGET_FAILED);
    }
    let peers: Vec<SavedPeer> = allowed
        .iter()
        .filter(|(saved, _)| **saved != key)
        .map(|(saved, name)| SavedPeer {
            key: wire::hex(saved),
            name: name.clone(),
        })
        .collect();
    if save_peers(path, &peers).is_err() {
        return Err(FORGET_FAILED);
    }
    allowed.remove(&key);
    Ok(())
}

fn save_peers(path: &Path, peers: &[SavedPeer]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&PeersFile {
        peers: peers.to_vec(),
    })
    .map_err(|err| io::Error::new(ErrorKind::InvalidData, err.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text + "\n")?;
    fs::rename(tmp, path)
}

#[cfg(target_os = "macos")]
use std::ffi::{CStr, CString};
#[cfg(target_os = "macos")]
use std::os::raw::{c_char, c_void};

#[cfg(target_os = "macos")]
type DNSServiceRef = *mut c_void;

#[cfg(target_os = "macos")]
#[link(name = "System")]
unsafe extern "C" {
    fn DNSServiceRegister(
        sd_ref: *mut DNSServiceRef,
        flags: u32,
        interface_index: u32,
        name: *const c_char,
        regtype: *const c_char,
        domain: *const c_char,
        host: *const c_char,
        port: u16,
        txt_len: u16,
        txt: *const c_void,
        callback: Option<RegisterReply>,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceBrowse(
        sd_ref: *mut DNSServiceRef,
        flags: u32,
        interface_index: u32,
        regtype: *const c_char,
        domain: *const c_char,
        callback: BrowseReply,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceResolve(
        sd_ref: *mut DNSServiceRef,
        flags: u32,
        interface_index: u32,
        name: *const c_char,
        regtype: *const c_char,
        domain: *const c_char,
        callback: ResolveReply,
        context: *mut c_void,
    ) -> i32;
    fn DNSServiceProcessResult(sd_ref: DNSServiceRef) -> i32;
    fn DNSServiceRefDeallocate(sd_ref: DNSServiceRef);
    fn DNSServiceRefSockFD(sd_ref: DNSServiceRef) -> i32;
    fn setsockopt(socket: i32, level: i32, name: i32, value: *const LibcVoid, len: u32) -> i32;
    fn poll(fds: *mut PollFd, nfds: u32, timeout: i32) -> i32;
}

#[cfg(target_os = "macos")]
type LibcVoid = c_void;

#[cfg(target_os = "macos")]
type RegisterReply = unsafe extern "C" fn(
    DNSServiceRef,
    u32,
    i32,
    *const c_char,
    *const c_char,
    *const c_char,
    *mut c_void,
);

#[cfg(target_os = "macos")]
type BrowseReply = unsafe extern "C" fn(
    DNSServiceRef,
    u32,
    u32,
    i32,
    *const c_char,
    *const c_char,
    *const c_char,
    *mut c_void,
);

#[cfg(target_os = "macos")]
type ResolveReply = unsafe extern "C" fn(
    DNSServiceRef,
    u32,
    u32,
    i32,
    *const c_char,
    *const c_char,
    u16,
    u16,
    *const u8,
    *mut c_void,
);

#[cfg(target_os = "macos")]
#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

#[cfg(target_os = "macos")]
struct Shared {
    world: Arc<Mutex<World>>,
    resolves: Mutex<Vec<Resolve>>,
}

#[cfg(target_os = "macos")]
struct Resolve {
    sd: usize,
    instance: String,
    done: bool,
    error: bool,
    key: Option<[u8; 32]>,
    host: String,
    port: u16,
}

#[cfg(target_os = "macos")]
const ADDED: u32 = 0x2;
#[cfg(target_os = "macos")]
const POLLIN: i16 = 1;

#[cfg(target_os = "macos")]
fn serve(world: Arc<Mutex<World>>) {
    let listener = match TcpListener::bind(("0.0.0.0", 0)) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("hop: could not listen for the other Mac ({err})");
            return;
        }
    };
    let _ = listener.set_nonblocking(true);
    let port = listener.local_addr().map(|addr| addr.port()).unwrap_or(0);
    let pk = world
        .lock()
        .map(|world| wire::hex(&world.identity.public_key()))
        .unwrap_or_default();
    let shared = Arc::new(Shared {
        world: world.clone(),
        resolves: Mutex::new(Vec::new()),
    });
    let context = Arc::into_raw(shared.clone()) as *mut c_void;
    let register = register_service(&service_name(&computer_name()), port, &pk);
    let browse = start_browse(context);
    loop {
        let mut fds = Vec::new();
        push_fd(&mut fds, register);
        push_fd(&mut fds, browse);
        let resolve_refs: Vec<usize> = shared
            .resolves
            .lock()
            .map(|slots| slots.iter().map(|slot| slot.sd).collect())
            .unwrap_or_default();
        for sd in &resolve_refs {
            push_fd(&mut fds, *sd as DNSServiceRef);
        }
        let listener_fd = listener.as_raw_fd();
        fds.push(PollFd {
            fd: listener_fd,
            events: POLLIN,
            revents: 0,
        });
        unsafe {
            poll(fds.as_mut_ptr(), fds.len() as u32, 200);
        }
        for fd in &fds {
            if fd.revents & POLLIN == 0 {
                continue;
            }
            if fd.fd == listener_fd {
                accept_all(&listener, &world);
            } else if !register.is_null() && fd.fd == sock(register) {
                unsafe {
                    DNSServiceProcessResult(register);
                }
            } else if !browse.is_null() && fd.fd == sock(browse) {
                unsafe {
                    DNSServiceProcessResult(browse);
                }
            } else {
                for sd in &resolve_refs {
                    let ptr = *sd as DNSServiceRef;
                    if fd.fd == sock(ptr) {
                        unsafe {
                            DNSServiceProcessResult(ptr);
                        }
                    }
                }
            }
        }
        finish_resolves(&shared);
    }
}

#[cfg(target_os = "macos")]
fn accepted(stream: TcpStream) -> TcpStream {
    let _ = stream.set_nonblocking(false);
    stream
}

#[cfg(target_os = "macos")]
fn accept_all(listener: &TcpListener, world: &Arc<Mutex<World>>) {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let stream = accepted(stream);
                let world = world.clone();
                thread::spawn(move || handle_client(stream, world));
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => break,
            Err(_) => break,
        }
    }
}

#[cfg(target_os = "macos")]
fn push_fd(fds: &mut Vec<PollFd>, sd: DNSServiceRef) {
    let fd = sock(sd);
    if fd >= 0 {
        fds.push(PollFd {
            fd,
            events: POLLIN,
            revents: 0,
        });
    }
}

#[cfg(target_os = "macos")]
fn sock(sd: DNSServiceRef) -> i32 {
    if sd.is_null() {
        -1
    } else {
        unsafe { DNSServiceRefSockFD(sd) }
    }
}

#[cfg(target_os = "macos")]
fn register_service(name: &str, port: u16, pk_hex: &str) -> DNSServiceRef {
    let mut sd: DNSServiceRef = std::ptr::null_mut();
    let Ok(c_name) = CString::new(name) else {
        return std::ptr::null_mut();
    };
    let Ok(c_type) = CString::new("_hop._tcp") else {
        return std::ptr::null_mut();
    };
    let txt = txt_record(pk_hex);
    let err = unsafe {
        DNSServiceRegister(
            &mut sd,
            0,
            0,
            c_name.as_ptr(),
            c_type.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            port.to_be(),
            txt.len() as u16,
            txt.as_ptr() as *const c_void,
            None,
            std::ptr::null_mut(),
        )
    };
    if err != 0 {
        eprintln!("hop: could not announce on the network ({err})");
        return std::ptr::null_mut();
    }
    sd
}

#[cfg(target_os = "macos")]
fn start_browse(context: *mut c_void) -> DNSServiceRef {
    let mut sd: DNSServiceRef = std::ptr::null_mut();
    let Ok(c_type) = CString::new("_hop._tcp") else {
        return std::ptr::null_mut();
    };
    let err = unsafe {
        DNSServiceBrowse(
            &mut sd,
            0,
            0,
            c_type.as_ptr(),
            std::ptr::null(),
            browse_reply,
            context,
        )
    };
    if err != 0 {
        eprintln!("hop: could not look for the other Mac ({err})");
        return std::ptr::null_mut();
    }
    sd
}

#[cfg(target_os = "macos")]
fn txt_record(pk_hex: &str) -> Vec<u8> {
    let entry = format!("pk={pk_hex}");
    let bytes = entry.as_bytes();
    let mut out = Vec::with_capacity(1 + bytes.len());
    out.push(bytes.len() as u8);
    out.extend_from_slice(bytes);
    out
}

#[cfg(target_os = "macos")]
fn txt_key(txt: &[u8]) -> Option<[u8; 32]> {
    let mut index = 0;
    while index < txt.len() {
        let len = txt[index] as usize;
        index += 1;
        if index + len > txt.len() {
            break;
        }
        let entry = &txt[index..index + len];
        index += len;
        let Ok(text) = std::str::from_utf8(entry) else {
            continue;
        };
        if let Some(value) = text.strip_prefix("pk=") {
            return wire::parse_key(value);
        }
    }
    None
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn browse_reply(
    _sd: DNSServiceRef,
    flags: u32,
    interface_index: u32,
    error: i32,
    name: *const c_char,
    regtype: *const c_char,
    domain: *const c_char,
    context: *mut c_void,
) {
    if error != 0 || context.is_null() {
        return;
    }
    let instance = c_string(name);
    let shared = unsafe { Arc::from_raw(context as *const Shared) };
    if flags & ADDED == 0 {
        if let Ok(mut world) = shared.world.lock() {
            world.sights.retain(|sight| sight.instance != instance);
        }
        std::mem::forget(shared);
        return;
    }
    let in_flight = shared.resolves.lock().ok().is_some_and(|slots| {
        slots
            .iter()
            .any(|slot| slot.instance == instance && !slot.done)
    });
    if in_flight {
        std::mem::forget(shared);
        return;
    }
    let Ok(c_name) = CString::new(instance.clone()) else {
        std::mem::forget(shared);
        return;
    };
    let Ok(c_type) = CString::new(c_string(regtype)) else {
        std::mem::forget(shared);
        return;
    };
    let Ok(c_domain) = CString::new(c_string(domain)) else {
        std::mem::forget(shared);
        return;
    };
    let mut sd: DNSServiceRef = std::ptr::null_mut();
    let err = unsafe {
        DNSServiceResolve(
            &mut sd,
            0,
            interface_index,
            c_name.as_ptr(),
            c_type.as_ptr(),
            c_domain.as_ptr(),
            resolve_reply,
            context,
        )
    };
    if err == 0 && !sd.is_null() {
        if let Ok(mut slots) = shared.resolves.lock() {
            slots.push(Resolve {
                sd: sd as usize,
                instance,
                done: false,
                error: false,
                key: None,
                host: String::new(),
                port: 0,
            });
        }
    }
    std::mem::forget(shared);
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn resolve_reply(
    sd: DNSServiceRef,
    _flags: u32,
    _interface_index: u32,
    error: i32,
    _fullname: *const c_char,
    hosttarget: *const c_char,
    port: u16,
    txt_len: u16,
    txt: *const u8,
    context: *mut c_void,
) {
    if context.is_null() {
        return;
    }
    let shared = unsafe { Arc::from_raw(context as *const Shared) };
    if let Ok(mut slots) = shared.resolves.lock() {
        if let Some(slot) = slots.iter_mut().find(|slot| slot.sd == sd as usize) {
            slot.done = true;
            if error == 0 {
                slot.host = c_string(hosttarget);
                slot.port = u16::from_be(port);
                if !txt.is_null() {
                    let txt = unsafe { std::slice::from_raw_parts(txt, txt_len as usize) };
                    slot.key = txt_key(txt);
                }
            } else {
                slot.error = true;
            }
        }
    }
    std::mem::forget(shared);
}

#[cfg(target_os = "macos")]
fn finish_resolves(shared: &Shared) {
    let Ok(mut slots) = shared.resolves.lock() else {
        return;
    };
    let mut index = 0;
    while index < slots.len() {
        if !slots[index].done {
            index += 1;
            continue;
        }
        let slot = slots.remove(index);
        unsafe { DNSServiceRefDeallocate(slot.sd as DNSServiceRef) };
        if slot.error || slot.key.is_none() {
            continue;
        }
        let Ok(mut world) = shared.world.lock() else {
            continue;
        };
        if slot.key == Some(world.identity.public_key()) {
            continue;
        }
        let resolved = slot.key.is_some() && !slot.host.is_empty() && slot.port != 0;
        if let Some(existing) = world
            .sights
            .iter_mut()
            .find(|sight| sight.instance == slot.instance)
        {
            existing.key = slot.key;
            existing.host = slot.host;
            existing.port = slot.port;
            existing.resolved = resolved;
        } else {
            world.sights.push(Sight {
                instance: slot.instance,
                key: slot.key,
                host: slot.host,
                port: slot.port,
                resolved,
            });
        }
    }
}

#[cfg(target_os = "macos")]
fn c_string(value: *const c_char) -> String {
    if value.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(target_os = "macos")]
fn service_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if out.len() + ch.len_utf8() > 63 {
            break;
        }
        out.push(ch);
    }
    if out.is_empty() { "Hop".into() } else { out }
}

#[cfg(target_os = "macos")]
fn computer_name() -> String {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let host: *mut Object = msg_send![class!(NSHost), currentHost];
        if host.is_null() {
            return "Hop".into();
        }
        let name: *mut Object = msg_send![host, localizedName];
        if name.is_null() {
            return "Hop".into();
        }
        let bytes: *const c_char = msg_send![name, UTF8String];
        let text = c_string(bytes);
        if text.is_empty() { "Hop".into() } else { text }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::io::{ErrorKind, Write};
    use std::net::TcpStream;
    use std::path::PathBuf;
    use std::time::Duration;

    use super::{accepted, load_peers, revoke, save_peers, saved_from};
    use crate::wire::{self, Identity};

    #[test]
    fn a_socket_from_the_listener_waits_for_the_rest_of_a_frame() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = loop {
                match listener.accept() {
                    Ok(pair) => break pair,
                    Err(err) if err.kind() == ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(err) => panic!("{err}"),
                }
            };
            let mut stream = accepted(stream);
            let _ = stream.set_read_timeout(Some(Duration::from_millis(400)));
            crate::wire::read_frame(&mut stream)
                .map(|_| ())
                .map_err(|err| err.kind())
        });
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client.write_all(&8u32.to_be_bytes()).unwrap();
        let started = std::time::Instant::now();
        let result = server.join().unwrap();
        assert!(
            started.elapsed() >= Duration::from_millis(300),
            "the listener closed before the frame arrived: {result:?}"
        );
    }

    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "hop-forget-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn key(byte: u8) -> [u8; 32] {
        let mut key = [0u8; 32];
        key[31] = byte;
        key
    }

    #[test]
    fn a_trusted_peer_is_removed_without_the_other() {
        let dir = scratch();
        let path = dir.join("peers.json");
        let mac_b = key(1);
        let mac_c = key(2);
        let mut allowed =
            HashMap::from([(mac_b, "Mac B".to_string()), (mac_c, "Mac C".to_string())]);
        save_peers(&path, &saved_from(&allowed)).unwrap();
        revoke(&path, &mut allowed, &wire::hex(&mac_b)).unwrap();
        assert!(!allowed.contains_key(&mac_b));
        assert_eq!(allowed.get(&mac_c).map(String::as_str), Some("Mac C"));
        let loaded = load_peers(&path);
        assert!(!loaded.contains_key(&mac_b));
        assert_eq!(loaded.get(&mac_c).map(String::as_str), Some("Mac C"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_forget_keeps_the_trusted_peer() {
        let dir = scratch();
        let blocked = dir.join("blocked");
        fs::write(&blocked, b"x").unwrap();
        let mac_b = key(1);
        let mut allowed = HashMap::from([(mac_b, "Mac B".to_string())]);
        let err = revoke(
            &blocked.join("peers.json"),
            &mut allowed,
            &wire::hex(&mac_b),
        );
        assert!(err.is_err());
        assert_eq!(allowed.get(&mac_b).map(String::as_str), Some("Mac B"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn forgetting_a_peer_leaves_the_local_identity() {
        let dir = scratch();
        let identity_path = dir.join("identity.json");
        let identity = Identity::load_or_create(&identity_path);
        let before = fs::read(&identity_path).unwrap();
        let public = identity.public_key();
        let mac_b = key(4);
        let mut allowed = HashMap::from([(mac_b, "Mac B".to_string())]);
        let peers = dir.join("peers.json");
        save_peers(&peers, &saved_from(&allowed)).unwrap();
        revoke(&peers, &mut allowed, &wire::hex(&mac_b)).unwrap();
        assert_eq!(fs::read(&identity_path).unwrap(), before);
        assert_eq!(
            Identity::load_or_create(&identity_path).public_key(),
            public
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_forgotten_peer_can_be_trusted_again() {
        let dir = scratch();
        let path = dir.join("peers.json");
        let mac_b = key(1);
        let mut allowed = HashMap::from([(mac_b, "Mac B".to_string())]);
        save_peers(&path, &saved_from(&allowed)).unwrap();
        revoke(&path, &mut allowed, &wire::hex(&mac_b)).unwrap();
        assert!(load_peers(&path).is_empty());
        allowed.insert(mac_b, "Mac B".to_string());
        save_peers(&path, &saved_from(&allowed)).unwrap();
        assert_eq!(
            load_peers(&path).get(&mac_b).map(String::as_str),
            Some("Mac B")
        );
        let _ = fs::remove_dir_all(dir);
    }
}
