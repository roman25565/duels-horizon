// Duels in the server's game: the open world's state, its files, and the
// edges between the game's types and the rules (`duels_core`). Added to the
// game as `game::duels_glue`; the small helpers (register.rs, perform.rs,
// console.rs, tick.rs, died.rs, login.rs, lifecycle.rs) call it from the
// hook points.
//
// Inside a player's command the other players are out of the game's reach,
// so everything a command does to someone else (a line, a teleport) waits
// in OUTBOX for the next server tick. Deaths and logins are heard as events
// and handled by the tick too.

use super::Game;
use super::duels_core::{self as core, Actor, Body, Config, Duels, Out, State};
use horizon_client::mod_api::{NativeCommand, NativeServerPlayer};
use horizon_client::text::Text;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

const MOD_ID: &str = "duels";

struct Open {
    /// `world/horizon-duels/`.
    dir: PathBuf,
    duels: Duels,
    /// Players who logged in, until they are placed (at most a minute).
    arrivals: Vec<(u128, String, i64)>,
}

enum Event {
    Died(u128),
    LoggedIn(u128, String),
}

static STATE: Mutex<Option<Open>> = Mutex::new(None);
static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());
static OUTBOX: Mutex<Vec<Out>> = Mutex::new(Vec::new());
static TICKS: AtomicU64 = AtomicU64::new(0);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn strip(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

// -- files ---------------------------------------------------------------------

/// Writes `bytes` so that a crash leaves either the old file or the new one.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let temp = dir.join(format!(".{name}.tmp"));
    {
        let mut file = File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&temp, path)
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) {
    match serde_json::to_vec_pretty(value) {
        Ok(mut bytes) => {
            bytes.push(b'\n');
            if let Err(e) = write_atomic(path, &bytes) {
                log::error!("[duels] can't write {}: {e}", path.display());
            }
        }
        Err(e) => log::error!("[duels] can't encode {}: {e}", path.display()),
    }
}

/// A JSON file: its default when missing. A damaged file is kept aside as
/// `<name>.bad-<time>` (never overwritten) and the default is used.
fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(e) => {
                let aside = path.with_extension(format!("json.bad-{}", now()));
                log::error!(
                    "[duels] {} is damaged ({e}); kept as {} and starting from defaults",
                    path.display(),
                    aside.display()
                );
                let _ = fs::rename(path, &aside);
                T::default()
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => T::default(),
        Err(e) => {
            log::error!("[duels] can't read {}: {e}", path.display());
            T::default()
        }
    }
}

/// The config, written back with any keys a newer version adds.
fn load_config(dir: &Path) -> Config {
    let path = dir.join(core::CONFIG_FILE);
    let config: Config = read_json(&path);
    write_json(&path, &config);
    config
}

/// The open world's duels, loaded the first time (or for another world).
fn open<'a>(guard: &'a mut Option<Open>, world: &Path) -> &'a mut Open {
    let dir = world.join(core::DIR);
    if guard.as_ref().is_none_or(|o| o.dir != dir) {
        if let Some(old) = guard.take() {
            write_json(&old.dir.join(core::STATE_FILE), &old.duels.state);
        }
        let config = load_config(&dir);
        let state: State = read_json(&dir.join(core::STATE_FILE));
        let (duels, out) = Duels::new(config, state, now());
        for o in out {
            if let Out::Log(l) = o {
                log::info!("[duels] {}", strip(&l));
            }
        }
        log::info!(
            "[duels] {} players, {} arenas ({})",
            duels.state.players.len(),
            duels.state.arenas.len(),
            dir.display()
        );
        *guard = Some(Open {
            dir,
            duels,
            arrivals: Vec::new(),
        });
    }
    guard.as_mut().expect("opened above")
}

fn save(o: &mut Open) {
    if o.duels.dirty {
        write_json(&o.dir.join(core::STATE_FILE), &o.duels.state);
        o.duels.dirty = false;
    }
}

// -- hook points -------------------------------------------------------------------

pub(super) fn register(commands: &mut Vec<NativeCommand>) {
    for (usage, level) in core::COMMANDS {
        commands.push(if *level == 0 {
            NativeCommand::new(usage)
        } else {
            NativeCommand::op(usage, *level)
        });
    }
}

fn body(p: &NativeServerPlayer) -> Body {
    Body {
        uuid: p.uuid,
        name: p.name.clone(),
        dimension: p.dimension.clone(),
        pos: p.position,
        yaw: p.yaw,
        pitch: p.pitch,
        alive: p.health > 0.0,
    }
}

/// A player's command: true when it was ours.
pub(super) fn perform(game: &mut Game<'_>, player: &NativeServerPlayer, command: &str) -> bool {
    let actor = Actor {
        body: Some(body(player)),
        permission: player.permission,
    };
    command_as(game, actor, command)
}

/// The dedicated server's console: true when it was ours.
pub(super) fn console(game: &mut Game<'_>, command: &str) -> bool {
    let actor = Actor {
        body: None,
        permission: 4,
    };
    command_as(game, actor, command)
}

fn command_as(game: &mut Game<'_>, actor: Actor, command: &str) -> bool {
    if !game.native_on(MOD_ID) {
        return false;
    }
    let Some(cmd) = core::parse_command(command) else {
        return false;
    };
    let Some(world) = game.native_world_dir() else {
        game.native_feedback(Text::legacy("§cСвіт не відкритий."));
        return true;
    };
    let me = actor.body.as_ref().map(|b| b.uuid);
    let out = {
        let mut guard = lock(&STATE);
        let o = open(&mut guard, &world);
        let out = if cmd == core::Cmd::Reload {
            if actor.permission < core::ADMIN_LEVEL {
                vec![Out::Tell(me.unwrap_or_default(), "§cЦя команда лише для операторів.".into())]
            } else {
                o.duels.config = load_config(&o.dir);
                vec![Out::Log("§aКонфіг дуелей перечитано.".into())]
            }
        } else {
            o.duels.command(&actor, cmd, now())
        };
        save(o);
        out
    };
    let mut later = Vec::new();
    for o in out {
        match o {
            Out::Tell(u, l) if Some(u) == me => game.native_feedback(Text::legacy(&l)),
            Out::Log(l) => game.native_feedback(Text::legacy(&l)),
            other => later.push(other),
        }
    }
    lock(&OUTBOX).extend(later);
    true
}

pub(super) fn died(player: u128) {
    lock(&EVENTS).push(Event::Died(player));
}

pub(super) fn logged_in(player: u128, name: &str) {
    lock(&EVENTS).push(Event::LoggedIn(player, name.to_owned()));
}

/// The world closed: the state is written and let go.
pub(super) fn closed() {
    if let Some(mut o) = lock(&STATE).take() {
        o.duels.dirty = true;
        save(&mut o);
    }
    lock(&EVENTS).clear();
    lock(&OUTBOX).clear();
}

/// The end of each server tick: what commands left for it, and once a
/// second the rules' own tick.
pub(super) fn tick(game: &mut Game<'_>) {
    if !game.native_on(MOD_ID) {
        return;
    }
    let n = TICKS.fetch_add(1, Ordering::Relaxed);
    let queued = std::mem::take(&mut *lock(&OUTBOX));
    if !queued.is_empty() {
        deliver(game, queued);
    }
    if n % 20 != 0 {
        return;
    }
    let Some(world) = game.native_world_dir() else {
        return;
    };
    let players = game.native_server_players();
    let bodies: Vec<Body> = players.iter().map(body).collect();
    let events = std::mem::take(&mut *lock(&EVENTS));
    let t = now();
    let out = {
        let mut guard = lock(&STATE);
        let o = open(&mut guard, &world);
        let mut died: Vec<u128> = Vec::new();
        for e in events {
            match e {
                Event::Died(u) => died.push(u),
                Event::LoggedIn(u, name) => o.arrivals.push((u, name, t)),
            }
        }
        // A death the event missed: a duelist at 0 health.
        died.extend(bodies.iter().filter(|b| !b.alive).map(|b| b.uuid));
        let mut out = Vec::new();
        // Players who arrived: their name, the trip back from a duel they
        // left, and the lines they missed.
        let arrivals = std::mem::take(&mut o.arrivals);
        for (u, name, when) in arrivals {
            if !bodies.iter().any(|b| b.uuid == u && b.alive) {
                if t - when < 60_000 {
                    o.arrivals.push((u, name, when));
                }
                continue;
            }
            o.duels.seen(u, &name, t);
            let key = core::uuid_string(u);
            if let Some(spot) = o.duels.state.returns.remove(&key) {
                o.duels.dirty = true;
                out.push(Out::Teleport(u, spot));
                out.push(Out::Tell(u, "§7Тебе повернуто туди, де ти був до дуелі.".into()));
            }
            if let Some(lines) = o.duels.state.notices.remove(&key) {
                o.duels.dirty = true;
                out.push(Out::Tell(u, "§6⚔ Поки тебе не було:".into()));
                out.extend(lines.into_iter().map(|l| Out::Tell(u, l)));
            }
            let incoming = o.duels.state.challenges.iter().filter(|c| c.to == key).count();
            if incoming > 0 {
                out.push(Out::Tell(
                    u,
                    format!("§c⚔ На тебе чекає викликів: {incoming}. §e/duel list"),
                ));
            }
        }
        out.extend(o.duels.tick(&bodies, &died, t));
        save(o);
        out
    };
    deliver(game, out);
}

/// Carries out the rules' effects; lines for players who are offline are
/// kept for when they come back.
fn deliver(game: &mut Game<'_>, out: Vec<Out>) {
    let mut missed = Vec::new();
    for o in out {
        match o {
            Out::Tell(u, l) => {
                if !game.native_tell(u, Text::legacy(&l)) {
                    missed.push((u, l));
                }
            }
            Out::ActionBar(u, l) => {
                game.native_action_bar(u, Text::legacy(&l));
            }
            Out::Broadcast(l) => game.native_broadcast(Text::legacy(&l)),
            Out::Log(l) => log::info!("[duels] {}", strip(&l)),
            Out::Teleport(u, s) => {
                // `.3`: a whole number would be a block's centre to `tp`.
                let line = format!(
                    "execute in {} run tp {} {:.3} {:.3} {:.3} {:.1} {:.1}",
                    s.dimension,
                    core::uuid_string(u),
                    s.pos[0],
                    s.pos[1],
                    s.pos[2],
                    s.yaw,
                    s.pitch
                );
                if !game.native_run_command(&line) {
                    log::warn!("[duels] a teleport failed: {line}");
                }
            }
        }
    }
    if !missed.is_empty()
        && let Some(o) = lock(&STATE).as_mut()
    {
        for (u, l) in missed {
            o.duels.notice(u, l);
        }
    }
}
