// Duels: the rules of the headband ladder, with no game types, so that they
// are tested on their own (`cargo test` in the mod's folder). Added to the
// game as `game::duels_core`; `duels_glue` joins it to the hook points.
//
// The ladder is a list of players in rank order: index 0 wears the Number
// One headband, index 1 the Number Two. Anybody may challenge Number Two;
// only Number Two may challenge Number One. The challenged player has
// `challenge_expiry_hours` (72: three real days) to accept or decline. An
// accepted challenge sends both to a free arena, where they fight to the
// death. Points are Elo points, for the table; the headbands move only by
// duels (and by an operator).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The mod's folder in the world's folder: `world/horizon-duels/`.
pub const DIR: &str = "horizon-duels";
pub const CONFIG_FILE: &str = "config.json";
pub const STATE_FILE: &str = "state.json";
/// The operator level for `/duelsadmin`.
pub const ADMIN_LEVEL: u8 = 2;

const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

// ---------------------------------------------------------------------------
// Config

/// Where the defeated Number Two goes when a lower player beats them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    /// The winner takes #2, the loser becomes #3 and everyone between moves
    /// down one place.
    Shift,
    /// The winner and the loser swap places.
    Swap,
}

/// `world/horizon-duels/config.json`, written with its defaults the first
/// time and again with the keys a newer version adds (values kept).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// How long a challenge waits for an answer (72 h: three real days).
    pub challenge_expiry_hours: u32,
    /// An unanswered challenge counts as a loss for the challenged player
    /// (the challenger takes the headband without a fight).
    pub expired_challenge_is_forfeit: bool,
    /// Points the challenged player loses for declining (0: none).
    pub decline_penalty_points: i32,
    /// Points a new player starts with.
    pub start_points: i32,
    /// The Elo K factor: the most points one duel moves.
    pub elo_k: f64,
    /// `shift` or `swap`: see [`Placement`].
    pub loser_placement: Placement,
    /// Seconds between the teleport and the start of the fight.
    pub countdown_seconds: u32,
    /// A fight longer than this ends in a draw (0: no limit).
    pub max_duel_minutes: u32,
    /// Seconds the winner stays in the arena (to take the loot) before
    /// being sent back where they were.
    pub return_delay_seconds: u32,
    /// A duelist who leaves the server during a duel loses it (false: draw).
    pub disconnect_forfeits: bool,
    /// A duelist who walks out of the arena is put back on their spawn
    /// point.
    pub keep_in_arena: bool,
    /// After a lost duel the challenger waits this long before challenging
    /// the same player again.
    pub rematch_cooldown_minutes: u32,
    /// How long an accepted duel waits for a free arena before it goes back
    /// to waiting for an answer.
    pub arena_wait_minutes: u32,
    /// Lines in one page of `/duel top`.
    pub table_page_size: u32,
    /// Duel results and headband changes in everyone's chat.
    pub announce: bool,
    /// Rewards for a long reign as Number One. Not used yet.
    pub number_one: NumberOneConfig,
}

/// The extension point for Number One's rewards. Nothing is given yet:
/// `milestone_days` are recorded in the player's data when reached
/// (`/duel stats` shows them) and [`Duels::number_one_rewards`] is where a
/// later version hands out the bonuses.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NumberOneConfig {
    pub bonuses_enabled: bool,
    pub milestone_days: Vec<u32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            challenge_expiry_hours: 72,
            expired_challenge_is_forfeit: false,
            decline_penalty_points: 0,
            start_points: 1000,
            elo_k: 32.0,
            loser_placement: Placement::Shift,
            countdown_seconds: 5,
            max_duel_minutes: 10,
            return_delay_seconds: 15,
            disconnect_forfeits: true,
            keep_in_arena: true,
            rematch_cooldown_minutes: 30,
            arena_wait_minutes: 5,
            table_page_size: 10,
            announce: true,
            number_one: NumberOneConfig {
                bonuses_enabled: false,
                milestone_days: vec![7, 30, 100],
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Saved state

/// A place: a level and a position with a view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spot {
    pub dimension: String,
    pub pos: [f64; 3],
    #[serde(default)]
    pub yaw: f32,
    #[serde(default)]
    pub pitch: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerRec {
    pub name: String,
    pub points: i32,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
    pub declines: u32,
    /// Milliseconds spent as Number One, reigns that ended.
    pub number_one_ms: i64,
    /// The best rank reached (1 is the best).
    pub best_rank: Option<u32>,
    /// `number_one.milestone_days` reached in the current reign.
    pub milestones: Vec<u32>,
    pub first_seen: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// A challenge to the Number Two headband (anyone may send one).
    NumberTwo,
    /// Number Two's challenge to Number One.
    NumberOne,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Challenge {
    pub id: u64,
    pub from: String,
    pub to: String,
    pub target: Target,
    pub created: i64,
    pub expires: i64,
    /// Accepted, waiting for both players and a free arena (since then).
    #[serde(default)]
    pub accepted: Option<i64>,
    /// The players were told no arena is free.
    #[serde(default)]
    pub told_waiting: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Arena {
    pub dimension: String,
    pub center: [f64; 3],
    /// Half the side of the arena's box, in blocks, from its centre.
    pub radius: f64,
    pub spawns: [Option<Spot>; 2],
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl Arena {
    pub fn ready(&self) -> bool {
        self.enabled && self.spawns.iter().all(Option::is_some)
    }
    pub fn contains(&self, dimension: &str, p: [f64; 3]) -> bool {
        dimension == self.dimension
            && (0..3).all(|i| (p[i] - self.center[i]).abs() <= self.radius + 0.5)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "phase")]
pub enum Phase {
    Countdown { until: i64 },
    Fight { since: i64 },
    /// Over; the survivors go back at `until`.
    Ending { until: i64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Duel {
    pub id: u64,
    pub arena: String,
    pub challenger: String,
    pub defender: String,
    pub target: Target,
    pub phase: Phase,
    /// Where each was before the duel, to send them back.
    pub back: BTreeMap<String, Spot>,
    /// Who died in it (they respawn on their own and are not sent back).
    #[serde(default)]
    pub dead: Vec<String>,
    pub started: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub players: BTreeMap<String, PlayerRec>,
    /// UUIDs in rank order: [0] is Number One.
    pub ladder: Vec<String>,
    pub challenges: Vec<Challenge>,
    pub duels: Vec<Duel>,
    pub arenas: BTreeMap<String, Arena>,
    /// When the current Number One took the headband.
    pub number_one_since: Option<i64>,
    /// Players to send back when they log in (they left during a duel, or
    /// the server stopped during one).
    pub returns: BTreeMap<String, Spot>,
    /// Lines for players who were offline.
    pub notices: BTreeMap<String, Vec<String>>,
    /// (loser challenger, defender) -> when the challenger lost.
    pub lost_to: BTreeMap<String, i64>,
    pub next_id: u64,
}

// ---------------------------------------------------------------------------
// Effects

/// What the game should do: the glue turns these into chat lines, action
/// bars and teleports.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    Tell(u128, String),
    ActionBar(u128, String),
    Broadcast(String),
    Teleport(u128, Spot),
    Log(String),
}

/// A player online, as the game sees them.
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub uuid: u128,
    pub name: String,
    pub dimension: String,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub alive: bool,
}

impl Body {
    pub fn spot(&self) -> Spot {
        Spot {
            dimension: self.dimension.clone(),
            pos: self.pos,
            yaw: self.yaw,
            pitch: self.pitch,
        }
    }
}

// ---------------------------------------------------------------------------
// UUIDs

/// `UUID.toString()`: 8-4-4-4-12 lowercase hex digits.
pub fn uuid_string(u: u128) -> String {
    let h = format!("{u:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

pub fn parse_uuid(s: &str) -> Option<u128> {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    (hex.len() == 32)
        .then(|| u128::from_str_radix(&hex, 16).ok())
        .flatten()
}

// ---------------------------------------------------------------------------
// Commands

/// Every usage, with the operator level it needs.
pub const COMMANDS: &[(&str, u8)] = &[
    ("duel", 0),
    ("duel help", 0),
    ("duel top [page:integer]", 0),
    ("duel join", 0),
    ("duel challenge", 0),
    ("duel accept [player:word]", 0),
    ("duel decline [player:word]", 0),
    ("duel cancel", 0),
    ("duel list", 0),
    ("duel stats [player:word]", 0),
    ("duelsadmin arena create <name:word> <radius:integer>", ADMIN_LEVEL),
    ("duelsadmin arena spawn <name:word> <point:integer>", ADMIN_LEVEL),
    ("duelsadmin arena delete <name:word>", ADMIN_LEVEL),
    ("duelsadmin arena enable <name:word>", ADMIN_LEVEL),
    ("duelsadmin arena disable <name:word>", ADMIN_LEVEL),
    ("duelsadmin arena tp <name:word>", ADMIN_LEVEL),
    ("duelsadmin arena list", ADMIN_LEVEL),
    ("duelsadmin setrank <player:word> <rank:integer>", ADMIN_LEVEL),
    ("duelsadmin remove <player:word>", ADMIN_LEVEL),
    ("duelsadmin stop <player:word>", ADMIN_LEVEL),
    ("duelsadmin reload", ADMIN_LEVEL),
];

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Help,
    Top(u32),
    Join,
    Challenge,
    Accept(Option<String>),
    Decline(Option<String>),
    Cancel,
    List,
    Stats(Option<String>),
    ArenaCreate(String, i64),
    ArenaSpawn(String, i64),
    ArenaDelete(String),
    ArenaEnable(String, bool),
    ArenaTp(String),
    ArenaList,
    SetRank(String, i64),
    Remove(String),
    Stop(String),
    Reload,
    /// Our command, typed wrong: its usage.
    Usage(&'static str),
}

impl Cmd {
    pub fn admin(&self) -> bool {
        matches!(
            self,
            Cmd::ArenaCreate(..)
                | Cmd::ArenaSpawn(..)
                | Cmd::ArenaDelete(_)
                | Cmd::ArenaEnable(..)
                | Cmd::ArenaTp(_)
                | Cmd::ArenaList
                | Cmd::SetRank(..)
                | Cmd::Remove(_)
                | Cmd::Stop(_)
                | Cmd::Reload
        )
    }
}

const ADMIN_USAGE: &str = "§eВикористання: /duelsadmin arena create <назва> <радіус> | arena spawn <назва> <1|2> | \
arena delete|enable|disable|tp <назва> | arena list | setrank <гравець> <місце> | remove <гравець> | \
stop <гравець> | reload";

/// A command line without its slash: Some when it is one of ours.
pub fn parse_command(line: &str) -> Option<Cmd> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let (&root, rest) = words.split_first()?;
    let num = |s: &str| s.parse::<i64>().ok();
    let opt = |i: usize| rest.get(i).map(|s| (*s).to_owned());
    match root.to_ascii_lowercase().as_str() {
        "duel" | "duels" => Some(match rest.first().map(|s| s.to_ascii_lowercase()) {
            None => Cmd::Top(1),
            Some(w) => match w.as_str() {
                "help" => Cmd::Help,
                "top" => match rest.get(1) {
                    None => Cmd::Top(1),
                    Some(p) => match num(p) {
                        Some(p) if p >= 1 => Cmd::Top(p.min(u32::MAX as i64) as u32),
                        _ => Cmd::Usage("§eВикористання: /duel top [сторінка]"),
                    },
                },
                "join" => Cmd::Join,
                "challenge" => Cmd::Challenge,
                "accept" => Cmd::Accept(opt(1)),
                "decline" => Cmd::Decline(opt(1)),
                "cancel" => Cmd::Cancel,
                "list" => Cmd::List,
                "stats" => Cmd::Stats(opt(1)),
                _ => Cmd::Help,
            },
        }),
        "duelsadmin" => {
            let w = |i: usize| rest.get(i).map(|s| s.to_ascii_lowercase());
            Some(match (w(0).as_deref(), w(1).as_deref()) {
                (Some("arena"), Some("create")) => {
                    match (opt(2), rest.get(3).and_then(|s| num(s))) {
                        (Some(n), Some(r)) => Cmd::ArenaCreate(n.to_ascii_lowercase(), r),
                        _ => Cmd::Usage("§eВикористання: /duelsadmin arena create <назва> <радіус>"),
                    }
                }
                (Some("arena"), Some("spawn")) => {
                    match (opt(2), rest.get(3).and_then(|s| num(s))) {
                        (Some(n), Some(p)) => Cmd::ArenaSpawn(n.to_ascii_lowercase(), p),
                        _ => Cmd::Usage("§eВикористання: /duelsadmin arena spawn <назва> <1|2>"),
                    }
                }
                (Some("arena"), Some(op @ ("delete" | "enable" | "disable" | "tp"))) => {
                    match opt(2) {
                        Some(n) => {
                            let n = n.to_ascii_lowercase();
                            match op {
                                "delete" => Cmd::ArenaDelete(n),
                                "enable" => Cmd::ArenaEnable(n, true),
                                "disable" => Cmd::ArenaEnable(n, false),
                                _ => Cmd::ArenaTp(n),
                            }
                        }
                        None => Cmd::Usage(ADMIN_USAGE),
                    }
                }
                (Some("arena"), Some("list")) => Cmd::ArenaList,
                (Some("setrank"), _) => match (opt(1), rest.get(2).and_then(|s| num(s))) {
                    (Some(p), Some(r)) => Cmd::SetRank(p, r),
                    _ => Cmd::Usage("§eВикористання: /duelsadmin setrank <гравець> <місце>"),
                },
                (Some("remove"), _) => match opt(1) {
                    Some(p) => Cmd::Remove(p),
                    None => Cmd::Usage("§eВикористання: /duelsadmin remove <гравець>"),
                },
                (Some("stop"), _) => match opt(1) {
                    Some(p) => Cmd::Stop(p),
                    None => Cmd::Usage("§eВикористання: /duelsadmin stop <гравець>"),
                },
                (Some("reload"), _) => Cmd::Reload,
                _ => Cmd::Usage(ADMIN_USAGE),
            })
        }
        _ => None,
    }
}

/// A valid arena name: 1 to 32 of `a-z 0-9 _ -`.
pub fn valid_arena_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// The one who typed a command: a player, or the server's console.
#[derive(Clone, Debug)]
pub struct Actor {
    /// None for the console.
    pub body: Option<Body>,
    pub permission: u8,
}

// ---------------------------------------------------------------------------
// Elo

/// The winner's gain (and the loser's loss) for a win, or the first
/// player's change for a draw (`draw`), rounded.
pub fn elo_change(first: i32, second: i32, k: f64, draw: bool) -> i32 {
    let expected = 1.0 / (1.0 + 10f64.powf(f64::from(second - first) / 400.0));
    let score = if draw { 0.5 } else { 1.0 };
    (k * (score - expected)).round() as i32
}

// ---------------------------------------------------------------------------
// The rules

/// How a duel ended.
#[derive(Clone, Debug, PartialEq)]
pub enum End {
    Won { winner: String, reason: &'static str },
    Draw(&'static str),
}

pub struct Duels {
    pub config: Config,
    pub state: State,
    /// The state changed since it was last written.
    pub dirty: bool,
    /// A random number's state, for the arena's pick.
    seed: u64,
}

fn title(rank: usize) -> &'static str {
    match rank {
        1 => "§6§lНомер Один§r",
        2 => "§c§lНомер Два§r",
        _ => "",
    }
}

impl Duels {
    pub fn new(config: Config, mut state: State, now: i64) -> (Self, Vec<Out>) {
        let mut out = Vec::new();
        // Duels the server stopped in the middle of: a draw, and everyone in
        // them goes back where they were when they log in.
        for duel in std::mem::take(&mut state.duels) {
            let survivors: Vec<(String, Spot)> = duel
                .back
                .iter()
                .filter(|(u, _)| !duel.dead.contains(u))
                .map(|(u, s)| (u.clone(), s.clone()))
                .collect();
            state.returns.extend(survivors);
            if matches!(duel.phase, Phase::Countdown { .. } | Phase::Fight { .. }) {
                for u in [&duel.challenger, &duel.defender] {
                    state
                        .notices
                        .entry(u.clone())
                        .or_default()
                        .push("§eСервер перезапустився під час твоєї дуелі: нічия, рейтинг не змінився.".into());
                }
                out.push(Out::Log(format!(
                    "duel {} on arena {} interrupted by a restart: draw",
                    duel.id, duel.arena
                )));
            }
        }
        for c in &mut state.challenges {
            c.accepted = None;
            c.told_waiting = false;
        }
        if state.number_one_since.is_none() && !state.ladder.is_empty() {
            state.number_one_since = Some(now);
        }
        let seed = (now as u64) ^ 0x9e37_79b9_7f4a_7c15;
        (
            Self {
                config,
                state,
                dirty: true,
                seed,
            },
            out,
        )
    }

    fn random(&mut self, n: usize) -> usize {
        // xorshift64*
        let mut x = self.seed | 1;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.seed = x;
        (x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 33) as usize % n.max(1)
    }

    fn next_id(&mut self) -> u64 {
        self.state.next_id += 1;
        self.state.next_id
    }

    // -- players and ranks -------------------------------------------------

    /// Records a player seen online under `name`.
    pub fn seen(&mut self, uuid: u128, name: &str, now: i64) {
        let key = uuid_string(uuid);
        let start = self.config.start_points;
        let rec = self.state.players.entry(key).or_insert_with(|| PlayerRec {
            points: start,
            first_seen: now,
            ..Default::default()
        });
        if rec.name != name {
            rec.name = name.to_owned();
            self.dirty = true;
        }
    }

    pub fn name(&self, key: &str) -> String {
        self.state
            .players
            .get(key)
            .map(|p| p.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| key[..8.min(key.len())].to_owned())
    }

    /// 1-based rank.
    pub fn rank(&self, key: &str) -> Option<usize> {
        self.state.ladder.iter().position(|k| k == key).map(|i| i + 1)
    }

    pub fn holder(&self, rank: usize) -> Option<&String> {
        rank.checked_sub(1).and_then(|i| self.state.ladder.get(i))
    }

    pub fn find_player(&self, name: &str) -> Option<String> {
        if let Some(u) = parse_uuid(name) {
            let key = uuid_string(u);
            if self.state.players.contains_key(&key) {
                return Some(key);
            }
        }
        self.state
            .players
            .iter()
            .find(|(_, p)| p.name.eq_ignore_ascii_case(name))
            .map(|(k, _)| k.clone())
    }

    fn in_duel(&self, key: &str) -> Option<usize> {
        self.state
            .duels
            .iter()
            .position(|d| d.challenger == key || d.defender == key)
    }

    /// The ladder changed: records the best ranks, Number One's reign and
    /// drops the challenges that no longer hold. `before` is Number One
    /// before the change.
    fn ladder_changed(&mut self, before: Option<String>, now: i64, out: &mut Vec<Out>) {
        self.dirty = true;
        for (i, key) in self.state.ladder.iter().enumerate() {
            if let Some(p) = self.state.players.get_mut(key) {
                let r = i as u32 + 1;
                if p.best_rank.is_none_or(|b| r < b) {
                    p.best_rank = Some(r);
                }
            }
        }
        let after = self.state.ladder.first().cloned();
        if before != after {
            if let (Some(old), Some(since)) = (&before, self.state.number_one_since)
                && let Some(p) = self.state.players.get_mut(old)
            {
                p.number_one_ms += (now - since).max(0);
                p.milestones.clear();
            }
            self.state.number_one_since = after.as_ref().map(|_| now);
        }
        // Challenges whose headband changed hands.
        let ladder = self.state.ladder.clone();
        let rank = |k: &str| ladder.iter().position(|x| x == k).map(|i| i + 1);
        let mut dropped = Vec::new();
        self.state.challenges.retain(|c| {
            let holds = match c.target {
                Target::NumberTwo => rank(&c.to) == Some(2) && rank(&c.from) != Some(1),
                Target::NumberOne => rank(&c.to) == Some(1) && rank(&c.from) == Some(2),
            };
            if !holds {
                dropped.push(c.clone());
            }
            holds
        });
        for c in dropped {
            let line = format!(
                "§7Виклик {} → {} скасовано: пов'язка змінила власника.",
                self.name(&c.from),
                self.name(&c.to)
            );
            self.tell(&c.from, &line, out);
            self.tell(&c.to, &line, out);
        }
    }

    /// A line for that player (kept for later by the glue if offline).
    fn tell(&self, key: &str, line: &str, out: &mut Vec<Out>) {
        if let Some(u) = parse_uuid(key) {
            out.push(Out::Tell(u, line.to_owned()));
        }
    }

    fn announce(&self, line: String, out: &mut Vec<Out>) {
        if self.config.announce {
            out.push(Out::Broadcast(line));
        } else {
            out.push(Out::Log(line));
        }
    }

    /// Keeps a line for a player who is offline.
    pub fn notice(&mut self, uuid: u128, line: String) {
        let list = self.state.notices.entry(uuid_string(uuid)).or_default();
        list.push(line);
        // Only the latest 20.
        let n = list.len();
        if n > 20 {
            list.drain(..n - 20);
        }
        self.dirty = true;
    }

    /// Puts a player on the ladder (at the bottom) if they aren't on it;
    /// the first two fill the empty headbands.
    fn enter(&mut self, key: &str, now: i64, out: &mut Vec<Out>) -> usize {
        if let Some(r) = self.rank(key) {
            return r;
        }
        let before = self.state.ladder.first().cloned();
        self.state.ladder.push(key.to_owned());
        let r = self.state.ladder.len();
        self.ladder_changed(before, now, out);
        if r <= 2 {
            self.announce(
                format!(
                    "§6⚔ {} забирає вільну пов'язку {}§6!",
                    self.name(key),
                    title(r)
                ),
                out,
            );
        }
        r
    }

    // -- player commands ---------------------------------------------------

    /// A player's or the console's command. Lines to the actor are
    /// `Out::Tell` to their UUID (the glue answers those as the command's
    /// feedback); for the console, `Out::Log`.
    pub fn command(&mut self, actor: &Actor, cmd: Cmd, now: i64) -> Vec<Out> {
        let mut out = Vec::new();
        let me = actor.body.as_ref().map(|b| b.uuid);
        let reply = |out: &mut Vec<Out>, line: String| match me {
            Some(u) => out.push(Out::Tell(u, line)),
            None => out.push(Out::Log(line)),
        };
        if cmd.admin() && actor.permission < ADMIN_LEVEL {
            reply(&mut out, "§cЦя команда лише для операторів.".into());
            return out;
        }
        if let Some(b) = &actor.body {
            self.seen(b.uuid, &b.name, now);
        }
        let key = me.map(uuid_string);
        let need_player = |out: &mut Vec<Out>| {
            reply(out, "§cЦю команду може виконати лише гравець у грі.".into());
        };
        match cmd {
            Cmd::Usage(u) => reply(&mut out, u.into()),
            Cmd::Help => {
                for l in [
                    "§6§l⚔ Дуелі пов'язок",
                    "§e/duel top [сторінка]§7 — таблиця рейтингу",
                    "§e/duel challenge§7 — викликати Номера Два (а Номер Два викликає Номера Один)",
                    "§e/duel accept [гравець]§7, §e/duel decline [гравець]§7 — відповісти на виклик",
                    "§e/duel cancel§7 — забрати свій виклик",
                    "§e/duel list§7 — твої виклики",
                    "§e/duel stats [гравець]§7 — статистика",
                    "§e/duel join§7 — стати в рейтинг",
                ] {
                    reply(&mut out, l.into());
                }
            }
            Cmd::Top(page) => {
                for l in self.table(page, key.as_deref(), now) {
                    reply(&mut out, l);
                }
            }
            Cmd::Join => match &key {
                None => need_player(&mut out),
                Some(k) => match self.rank(k) {
                    Some(r) => reply(&mut out, format!("§7Ти вже в рейтингу: §e#{r}")),
                    None => {
                        let r = self.enter(k, now, &mut out);
                        reply(&mut out, format!("§aТи в рейтингу: §e#{r}"));
                    }
                },
            },
            Cmd::Challenge => match &key {
                None => need_player(&mut out),
                Some(k) => self.challenge(k, now, &mut out),
            },
            Cmd::Accept(who) => match &key {
                None => need_player(&mut out),
                Some(k) => self.answer(k, who.as_deref(), true, now, &mut out),
            },
            Cmd::Decline(who) => match &key {
                None => need_player(&mut out),
                Some(k) => self.answer(k, who.as_deref(), false, now, &mut out),
            },
            Cmd::Cancel => match &key {
                None => need_player(&mut out),
                Some(k) => {
                    let mine: Vec<Challenge> = self
                        .state
                        .challenges
                        .iter()
                        .filter(|c| &c.from == k)
                        .cloned()
                        .collect();
                    if mine.is_empty() {
                        reply(&mut out, "§7У тебе немає викликів.".into());
                    }
                    for c in mine {
                        if c.accepted.is_some() {
                            reply(&mut out, "§cВиклик уже прийнято, бій ось-ось почнеться.".into());
                            continue;
                        }
                        self.state.challenges.retain(|x| x.id != c.id);
                        self.dirty = true;
                        reply(&mut out, format!("§7Виклик до {} забрано.", self.name(&c.to)));
                        let line = format!("§7{} забрав свій виклик.", self.name(k));
                        self.tell(&c.to, &line, &mut out);
                    }
                }
            },
            Cmd::List => match &key {
                None => need_player(&mut out),
                Some(k) => {
                    let mut any = false;
                    for c in self.state.challenges.clone() {
                        let left = format_left(c.expires - now);
                        if &c.to == k {
                            any = true;
                            reply(
                                &mut out,
                                format!(
                                    "§e← {} §7викликає тебе (ще {left}){}",
                                    self.name(&c.from),
                                    if c.accepted.is_some() { ", §aприйнято" } else { "" }
                                ),
                            );
                        } else if &c.from == k {
                            any = true;
                            reply(
                                &mut out,
                                format!("§e→ {} §7(ще {left})", self.name(&c.to)),
                            );
                        }
                    }
                    if !any {
                        reply(&mut out, "§7Викликів немає.".into());
                    }
                }
            },
            Cmd::Stats(who) => {
                let target = match who {
                    Some(n) => self.find_player(&n),
                    None => key.clone(),
                };
                match target {
                    None => reply(&mut out, "§cНемає такого гравця.".into()),
                    Some(t) => {
                        for l in self.stats(&t, now) {
                            reply(&mut out, l);
                        }
                    }
                }
            }
            // -- operators --
            Cmd::ArenaCreate(name, radius) => match &actor.body {
                None => need_player(&mut out),
                Some(b) => {
                    if !valid_arena_name(&name) {
                        reply(&mut out, "§cНазва арени: 1–32 символи a-z, 0-9, _ або -.".into());
                    } else if !(3..=256).contains(&radius) {
                        reply(&mut out, "§cРадіус арени: від 3 до 256 блоків.".into());
                    } else if self.state.duels.iter().any(|d| d.arena == name) {
                        reply(&mut out, "§cНа цій арені зараз дуель.".into());
                    } else {
                        let center = [b.pos[0].floor() + 0.5, b.pos[1].floor(), b.pos[2].floor() + 0.5];
                        let old = self.state.arenas.insert(
                            name.clone(),
                            Arena {
                                dimension: b.dimension.clone(),
                                center,
                                radius: radius as f64,
                                spawns: [None, None],
                                enabled: true,
                            },
                        );
                        self.dirty = true;
                        reply(
                            &mut out,
                            format!(
                                "§aАрену §e{name}§a {} з центром тут ({:.0} {:.0} {:.0}), розмір {}x{}x{}. \
                                 Тепер стань на точки й виконай §e/duelsadmin arena spawn {name} 1§a і §e2§a.",
                                if old.is_some() { "перестворено" } else { "створено" },
                                center[0],
                                center[1],
                                center[2],
                                radius * 2 + 1,
                                radius * 2 + 1,
                                radius * 2 + 1
                            ),
                        );
                    }
                }
            },
            Cmd::ArenaSpawn(name, point) => match &actor.body {
                None => need_player(&mut out),
                Some(b) => {
                    let busy = self.state.duels.iter().any(|d| d.arena == name);
                    match self.state.arenas.get_mut(&name) {
                        None => reply(&mut out, format!("§cНемає арени §e{name}")),
                        Some(_) if busy => reply(&mut out, "§cНа цій арені зараз дуель.".into()),
                        Some(_) if !(1..=2).contains(&point) => {
                            reply(&mut out, "§cТочка спавну: 1 або 2.".into())
                        }
                        Some(a) if !a.contains(&b.dimension, b.pos) => reply(
                            &mut out,
                            "§cТи поза межами арени: точка спавну має бути всередині.".into(),
                        ),
                        Some(a) => {
                            a.spawns[point as usize - 1] = Some(b.spot());
                            let ready = a.ready();
                            self.dirty = true;
                            reply(
                                &mut out,
                                format!(
                                    "§aТочку {point} арени §e{name}§a поставлено.{}",
                                    if ready { " Арена готова." } else { "" }
                                ),
                            );
                        }
                    }
                }
            },
            Cmd::ArenaDelete(name) => {
                if self.state.duels.iter().any(|d| d.arena == name) {
                    reply(&mut out, "§cНа цій арені зараз дуель.".into());
                } else if self.state.arenas.remove(&name).is_some() {
                    self.dirty = true;
                    reply(&mut out, format!("§aАрену §e{name}§a видалено."));
                } else {
                    reply(&mut out, format!("§cНемає арени §e{name}"));
                }
            }
            Cmd::ArenaEnable(name, on) => match self.state.arenas.get_mut(&name) {
                None => reply(&mut out, format!("§cНемає арени §e{name}")),
                Some(a) => {
                    a.enabled = on;
                    self.dirty = true;
                    reply(
                        &mut out,
                        format!("§aАрена §e{name}§a {}.", if on { "увімкнена" } else { "вимкнена" }),
                    );
                }
            },
            Cmd::ArenaTp(name) => match (me, self.state.arenas.get(&name)) {
                (None, _) => need_player(&mut out),
                (_, None) => reply(&mut out, format!("§cНемає арени §e{name}")),
                (Some(u), Some(a)) => out.push(Out::Teleport(
                    u,
                    Spot {
                        dimension: a.dimension.clone(),
                        pos: a.center,
                        yaw: 0.0,
                        pitch: 0.0,
                    },
                )),
            },
            Cmd::ArenaList => {
                if self.state.arenas.is_empty() {
                    reply(&mut out, "§7Арен немає. §e/duelsadmin arena create <назва> <радіус>".into());
                }
                for (name, a) in &self.state.arenas {
                    let busy = self.state.duels.iter().any(|d| &d.arena == name);
                    let status = if busy {
                        "§cйде дуель"
                    } else if !a.enabled {
                        "§7вимкнена"
                    } else if a.ready() {
                        "§aготова"
                    } else {
                        "§eнемає точок спавну"
                    };
                    reply(
                        &mut out,
                        format!(
                            "§e{name}§7: {} {:.0} {:.0} {:.0}, радіус {:.0}, точки {}/2 — {status}",
                            a.dimension,
                            a.center[0],
                            a.center[1],
                            a.center[2],
                            a.radius,
                            a.spawns.iter().filter(|s| s.is_some()).count()
                        ),
                    );
                }
            }
            Cmd::SetRank(name, rank) => match self.find_player(&name) {
                None => reply(&mut out, "§cНемає такого гравця (він має хоч раз зайти на сервер).".into()),
                Some(k) if self.in_duel(&k).is_some() => {
                    reply(&mut out, "§cГравець зараз у дуелі.".into())
                }
                Some(k) => {
                    let before = self.state.ladder.first().cloned();
                    self.state.ladder.retain(|x| x != &k);
                    let i = (rank.max(1) as usize - 1).min(self.state.ladder.len());
                    self.state.ladder.insert(i, k.clone());
                    self.ladder_changed(before, now, &mut out);
                    let line = format!("§6⚔ Оператор поставив {} на місце §e#{}§6.", self.name(&k), i + 1);
                    self.announce(line.clone(), &mut out);
                    if !self.config.announce {
                        reply(&mut out, line);
                    }
                }
            },
            Cmd::Remove(name) => match self.find_player(&name) {
                None => reply(&mut out, "§cНемає такого гравця.".into()),
                Some(k) if self.in_duel(&k).is_some() => {
                    reply(&mut out, "§cГравець зараз у дуелі.".into())
                }
                Some(k) => {
                    let before = self.state.ladder.first().cloned();
                    self.state.ladder.retain(|x| x != &k);
                    self.state.challenges.retain(|c| c.from != k && c.to != k);
                    self.ladder_changed(before, now, &mut out);
                    reply(&mut out, format!("§a{} прибрано з рейтингу.", self.name(&k)));
                }
            },
            Cmd::Stop(name) => match self.find_player(&name).and_then(|k| self.in_duel(&k)) {
                None => reply(&mut out, "§cЦей гравець не в дуелі.".into()),
                Some(i) => {
                    self.finish(i, End::Draw("зупинено оператором"), now, &mut out);
                    reply(&mut out, "§aДуель зупинено: нічия.".into());
                }
            },
            // Reloading is the glue's: it reads the file.
            Cmd::Reload => {}
        }
        out
    }

    fn challenge(&mut self, me: &str, now: i64, out: &mut Vec<Out>) {
        let my_name = self.name(me);
        let id = parse_uuid(me).unwrap_or_default();
        if self.in_duel(me).is_some() {
            out.push(Out::Tell(id, "§cТи вже в дуелі.".into()));
            return;
        }
        if let Some(c) = self.state.challenges.iter().find(|c| c.from == me) {
            out.push(Out::Tell(
                id,
                format!(
                    "§cТи вже викликав {}. §7Забрати виклик: §e/duel cancel",
                    self.name(&c.to)
                ),
            ));
            return;
        }
        let rank = self.rank(me);
        // Empty headbands are taken without a fight.
        if rank.is_none() && self.state.ladder.len() < 2 {
            self.enter(me, now, out);
            let r = self.rank(me).unwrap_or(0);
            out.push(Out::Tell(
                id,
                format!("§aПов'язка {} §aвільна — тепер вона твоя.", title(r)),
            ));
            return;
        }
        let (target, to) = match rank {
            Some(1) => {
                out.push(Out::Tell(
                    id,
                    "§6Ти — Номер Один. Ніхто не може напасти на тебе, окрім Номера Два: чекай на його виклик."
                        .into(),
                ));
                return;
            }
            Some(2) => match self.holder(1) {
                Some(one) => (Target::NumberOne, one.clone()),
                None => return,
            },
            _ => match self.holder(2) {
                Some(two) => (Target::NumberTwo, two.clone()),
                None => {
                    self.enter(me, now, out);
                    return;
                }
            },
        };
        let lost_key = format!("{me}>{to}");
        if let Some(&when) = self.state.lost_to.get(&lost_key) {
            let wait = i64::from(self.config.rematch_cooldown_minutes) * MINUTE - (now - when);
            if wait > 0 {
                out.push(Out::Tell(
                    id,
                    format!(
                        "§cТи нещодавно програв {}. Новий виклик можна через {}.",
                        self.name(&to),
                        format_left(wait)
                    ),
                ));
                return;
            }
        }
        // A challenger joins the ladder at the bottom.
        self.enter(me, now, out);
        let expires = now + i64::from(self.config.challenge_expiry_hours) * HOUR;
        let cid = self.next_id();
        self.state.challenges.push(Challenge {
            id: cid,
            from: me.to_owned(),
            to: to.clone(),
            target,
            created: now,
            expires,
            accepted: None,
            told_waiting: false,
        });
        self.dirty = true;
        let band = match target {
            Target::NumberOne => title(1),
            Target::NumberTwo => title(2),
        };
        let left = format_left(expires - now);
        out.push(Out::Tell(
            id,
            format!(
                "§aТи викликав {} ({band}§a). У нього є {left}, щоб прийняти або відхилити.",
                self.name(&to)
            ),
        ));
        self.tell(
            &to,
            &format!(
                "§c⚔ {my_name} викликає тебе на дуель за пов'язку {band}§c! §e/duel accept {my_name}§7 або §e/duel decline {my_name}§7 (є {left})."
            ),
            out,
        );
        if target == Target::NumberOne {
            self.announce(
                format!("§6⚔ Номер Два {my_name} кинув виклик Номеру Один {}!", self.name(&to)),
                out,
            );
        }
    }

    fn answer(&mut self, me: &str, who: Option<&str>, accept: bool, now: i64, out: &mut Vec<Out>) {
        let id = parse_uuid(me).unwrap_or_default();
        let from = match who {
            Some(n) => match self.find_player(n) {
                Some(k) => Some(k),
                None => {
                    out.push(Out::Tell(id, "§cНемає такого гравця.".into()));
                    return;
                }
            },
            None => None,
        };
        let mine: Vec<Challenge> = self
            .state
            .challenges
            .iter()
            .filter(|c| c.to == me && from.as_ref().is_none_or(|f| &c.from == f))
            .cloned()
            .collect();
        let c = match mine.as_slice() {
            [] => {
                out.push(Out::Tell(id, "§7Тебе ніхто не викликав.".into()));
                return;
            }
            [c] => c.clone(),
            many => {
                out.push(Out::Tell(
                    id,
                    "§eТебе викликали кілька гравців, вкажи кого: §f/duel accept <гравець>".into(),
                ));
                for c in many {
                    out.push(Out::Tell(
                        id,
                        format!("§7 - {} (ще {})", self.name(&c.from), format_left(c.expires - now)),
                    ));
                }
                return;
            }
        };
        if !accept {
            if c.accepted.is_some() {
                out.push(Out::Tell(id, "§cТи вже прийняв цей виклик.".into()));
                return;
            }
            self.state.challenges.retain(|x| x.id != c.id);
            let penalty = self.config.decline_penalty_points;
            if let Some(p) = self.state.players.get_mut(me) {
                p.declines += 1;
                p.points -= penalty;
            }
            self.dirty = true;
            out.push(Out::Tell(id, format!("§7Ти відхилив виклик {}.", self.name(&c.from))));
            self.tell(
                &c.from,
                &format!("§7{} відхилив твій виклик.", self.name(me)),
                out,
            );
            return;
        }
        if c.accepted.is_some() {
            out.push(Out::Tell(id, "§7Ти вже прийняв цей виклик, чекаємо на арену.".into()));
            return;
        }
        if self.in_duel(me).is_some() {
            out.push(Out::Tell(id, "§cТи вже в дуелі.".into()));
            return;
        }
        if let Some(x) = self.state.challenges.iter_mut().find(|x| x.id == c.id) {
            x.accepted = Some(now);
            x.told_waiting = false;
        }
        self.dirty = true;
        out.push(Out::Tell(
            id,
            format!("§aТи прийняв виклик {}. Готуйся!", self.name(&c.from)),
        ));
        self.tell(
            &c.from,
            &format!("§a{} прийняв твій виклик. Готуйся!", self.name(me)),
            out,
        );
    }

    // -- the tick ------------------------------------------------------------

    /// Once a second: challenges expire, accepted ones start, duels go on.
    /// `bodies` are the players online; `died` who died since the last call.
    pub fn tick(&mut self, bodies: &[Body], died: &[u128], now: i64) -> Vec<Out> {
        let mut out = Vec::new();
        for b in bodies {
            self.seen(b.uuid, &b.name, now);
        }
        self.expire(now, &mut out);
        self.start_accepted(bodies, now, &mut out);
        self.run_duels(bodies, died, now, &mut out);
        self.reign(now, &mut out);
        out
    }

    fn expire(&mut self, now: i64, out: &mut Vec<Out>) {
        let (gone, kept): (Vec<Challenge>, Vec<Challenge>) = std::mem::take(&mut self.state.challenges)
            .into_iter()
            .partition(|c| c.accepted.is_none() && c.expires <= now);
        self.state.challenges = kept;
        for c in gone {
            self.dirty = true;
            if self.config.expired_challenge_is_forfeit {
                self.tell(
                    &c.to,
                    &format!("§cТи не відповів на виклик {} — це поразка.", self.name(&c.from)),
                    out,
                );
                self.tell(
                    &c.from,
                    &format!("§a{} не відповів на твій виклик — перемога за неявкою.", self.name(&c.to)),
                    out,
                );
                self.result(&c.from, &c.to, c.target, Some("неявка"), now, out);
            } else {
                self.tell(
                    &c.to,
                    &format!("§7Виклик {} минув без відповіді.", self.name(&c.from)),
                    out,
                );
                self.tell(
                    &c.from,
                    &format!("§7{} не відповів на твій виклик, він минув.", self.name(&c.to)),
                    out,
                );
            }
        }
    }

    fn start_accepted(&mut self, bodies: &[Body], now: i64, out: &mut Vec<Out>) {
        let accepted: Vec<Challenge> = self
            .state
            .challenges
            .iter()
            .filter(|c| c.accepted.is_some())
            .cloned()
            .collect();
        for c in accepted {
            let body = |k: &str| {
                let u = parse_uuid(k)?;
                bodies.iter().find(|b| b.uuid == u && b.alive)
            };
            let (Some(a), Some(b)) = (body(&c.from), body(&c.to)) else {
                // Both must be on and alive: back to waiting for an answer.
                if let Some(x) = self.state.challenges.iter_mut().find(|x| x.id == c.id) {
                    x.accepted = None;
                }
                self.dirty = true;
                let line = "§eДуель не почалась: обидва гравці мають бути на сервері й живі. Прийміть виклик ще раз.";
                self.tell(&c.from, line, out);
                self.tell(&c.to, line, out);
                continue;
            };
            let (a, b) = (a.clone(), b.clone());
            if self.in_duel(&c.from).is_some() || self.in_duel(&c.to).is_some() {
                continue;
            }
            let free: Vec<String> = self
                .state
                .arenas
                .iter()
                .filter(|(n, ar)| ar.ready() && !self.state.duels.iter().any(|d| &d.arena == *n))
                .map(|(n, _)| n.clone())
                .collect();
            if free.is_empty() {
                let waited = now - c.accepted.unwrap_or(now);
                if waited >= i64::from(self.config.arena_wait_minutes) * MINUTE {
                    if let Some(x) = self.state.challenges.iter_mut().find(|x| x.id == c.id) {
                        x.accepted = None;
                        x.told_waiting = false;
                    }
                    let line = "§eВільна арена так і не з'явилась. Виклик знову чекає на відповідь.";
                    self.tell(&c.from, line, out);
                    self.tell(&c.to, line, out);
                } else if !c.told_waiting {
                    if let Some(x) = self.state.challenges.iter_mut().find(|x| x.id == c.id) {
                        x.told_waiting = true;
                    }
                    let line = if self.state.arenas.values().any(Arena::ready) {
                        "§eУсі арени зайняті, чекаємо на вільну..."
                    } else {
                        "§eЖодної готової арени. Попросіть оператора створити її (§f/duelsadmin arena create§e)."
                    };
                    self.tell(&c.from, line, out);
                    self.tell(&c.to, line, out);
                }
                self.dirty = true;
                continue;
            }
            let arena_name = free[self.random(free.len())].clone();
            let arena = self.state.arenas[&arena_name].clone();
            self.state.challenges.retain(|x| x.id != c.id);
            let id = self.next_id();
            let mut back = BTreeMap::new();
            back.insert(c.from.clone(), a.spot());
            back.insert(c.to.clone(), b.spot());
            let countdown = i64::from(self.config.countdown_seconds) * 1000;
            self.state.duels.push(Duel {
                id,
                arena: arena_name.clone(),
                challenger: c.from.clone(),
                defender: c.to.clone(),
                target: c.target,
                phase: if countdown > 0 {
                    Phase::Countdown { until: now + countdown }
                } else {
                    Phase::Fight { since: now }
                },
                back,
                dead: Vec::new(),
                started: now,
            });
            self.dirty = true;
            if let [Some(s1), Some(s2)] = &arena.spawns {
                out.push(Out::Teleport(a.uuid, s1.clone()));
                out.push(Out::Teleport(b.uuid, s2.clone()));
            }
            let band = match c.target {
                Target::NumberOne => title(1),
                Target::NumberTwo => title(2),
            };
            self.announce(
                format!(
                    "§c⚔ Дуель за пов'язку {band}§c: {} проти {} на арені §e{arena_name}§c!",
                    a.name, b.name
                ),
                out,
            );
            let line = "§cБій до смерті. Вихід з арени неможливий.";
            out.push(Out::Tell(a.uuid, line.into()));
            out.push(Out::Tell(b.uuid, line.into()));
            if countdown == 0 {
                out.push(Out::ActionBar(a.uuid, "§c§lБІЙ!".into()));
                out.push(Out::ActionBar(b.uuid, "§c§lБІЙ!".into()));
            }
        }
    }

    fn run_duels(&mut self, bodies: &[Body], died: &[u128], now: i64, out: &mut Vec<Out>) {
        let mut i = 0;
        while i < self.state.duels.len() {
            let d = self.state.duels[i].clone();
            let pair = [d.challenger.clone(), d.defender.clone()];
            let uuids = pair.clone().map(|k| parse_uuid(&k).unwrap_or_default());
            let on = uuids.map(|u| bodies.iter().find(|b| b.uuid == u));
            if let Phase::Ending { until } = d.phase {
                if now >= until {
                    // The survivors still in the arena go back.
                    let arena = self.state.arenas.get(&d.arena).cloned();
                    for (k, (u, body)) in pair.iter().zip(uuids.iter().zip(on)) {
                        if d.dead.contains(k) {
                            continue;
                        }
                        let Some(spot) = d.back.get(k) else { continue };
                        match body {
                            Some(b) if b.alive => {
                                if arena.as_ref().is_none_or(|a| a.contains(&b.dimension, b.pos)) {
                                    out.push(Out::Teleport(*u, spot.clone()));
                                }
                            }
                            Some(_) => {}
                            None => {
                                self.state.returns.insert(k.clone(), spot.clone());
                            }
                        }
                    }
                    self.state.duels.remove(i);
                    self.dirty = true;
                    continue;
                }
                i += 1;
                continue;
            }
            // Deaths.
            let dead = uuids.map(|u| died.contains(&u));
            if dead[0] || dead[1] {
                for (k, x) in pair.iter().zip(dead) {
                    if x {
                        self.state.duels[i].dead.push(k.clone());
                    }
                }
                let end = match dead {
                    [true, true] => End::Draw("обидва загинули"),
                    [true, false] => End::Won {
                        winner: pair[1].clone(),
                        reason: "у бою",
                    },
                    _ => End::Won {
                        winner: pair[0].clone(),
                        reason: "у бою",
                    },
                };
                self.finish(i, end, now, out);
                i += 1;
                continue;
            }
            // Leaving the server.
            let gone = on.map(|b| b.is_none());
            if gone[0] || gone[1] {
                let end = match (gone, self.config.disconnect_forfeits) {
                    ([true, true], _) | (_, false) => End::Draw("гравець вийшов з сервера"),
                    ([true, false], true) => End::Won {
                        winner: pair[1].clone(),
                        reason: "суперник утік з сервера",
                    },
                    _ => End::Won {
                        winner: pair[0].clone(),
                        reason: "суперник утік з сервера",
                    },
                };
                self.finish(i, end, now, out);
                i += 1;
                continue;
            }
            match d.phase {
                Phase::Countdown { until } => {
                    if now >= until {
                        self.state.duels[i].phase = Phase::Fight { since: now };
                        self.dirty = true;
                        for u in uuids {
                            out.push(Out::ActionBar(u, "§c§lБІЙ!".into()));
                        }
                    } else {
                        let s = (until - now + 999) / 1000;
                        for u in uuids {
                            out.push(Out::ActionBar(u, format!("§eБій через §c§l{s}")));
                        }
                    }
                }
                Phase::Fight { since } => {
                    let limit = i64::from(self.config.max_duel_minutes) * MINUTE;
                    if limit > 0 && now - since >= limit {
                        self.finish(i, End::Draw("час вийшов"), now, out);
                        i += 1;
                        continue;
                    }
                }
                Phase::Ending { .. } => {}
            }
            // Out of the arena: back on their own point (in the fight, once
            // the teleport to the arena has surely landed).
            let settled = matches!(d.phase, Phase::Fight { .. }) && now - d.started >= 2000;
            if self.config.keep_in_arena
                && settled
                && let Some(arena) = self.state.arenas.get(&d.arena)
            {
                for (n, body) in on.iter().enumerate() {
                    if let Some(b) = body
                        && b.alive
                        && !arena.contains(&b.dimension, b.pos)
                        && let Some(s) = &arena.spawns[n]
                    {
                        out.push(Out::Teleport(b.uuid, s.clone()));
                        out.push(Out::Tell(b.uuid, "§cЗ арени не втекти!".into()));
                    }
                }
            }
            i += 1;
        }
    }

    /// Ends duel `i`: the result, and the survivors stay a moment.
    fn finish(&mut self, i: usize, end: End, now: i64, out: &mut Vec<Out>) {
        let d = self.state.duels[i].clone();
        let delay = i64::from(self.config.return_delay_seconds) * 1000;
        self.state.duels[i].phase = Phase::Ending { until: now + delay };
        self.dirty = true;
        match end {
            End::Won { winner, reason } => {
                let loser = if winner == d.challenger { d.defender.clone() } else { d.challenger.clone() };
                let challenger_won = winner == d.challenger;
                let (ch, de) = (d.challenger.clone(), d.defender.clone());
                if challenger_won {
                    self.result(&ch, &de, d.target, Some(reason), now, out);
                } else {
                    self.result(&de, &ch, d.target, Some(reason), now, out);
                    self.state.lost_to.insert(format!("{ch}>{de}"), now);
                }
                if let Some(u) = parse_uuid(&winner)
                    && delay > 0
                    && !d.dead.contains(&winner)
                {
                    out.push(Out::Tell(
                        u,
                        format!("§7Через {} с тебе поверне туди, де ти був.", delay / 1000),
                    ));
                }
                let _ = loser;
            }
            End::Draw(reason) => {
                let (a, b) = (d.challenger.clone(), d.defender.clone());
                let pa = self.points(&a);
                let pb = self.points(&b);
                let change = elo_change(pa, pb, self.config.elo_k, true);
                if let Some(p) = self.state.players.get_mut(&a) {
                    p.draws += 1;
                    p.points += change;
                }
                if let Some(p) = self.state.players.get_mut(&b) {
                    p.draws += 1;
                    p.points -= change;
                }
                self.announce(
                    format!("§7⚔ Дуель {} і {}: нічия ({reason}).", self.name(&a), self.name(&b)),
                    out,
                );
            }
        }
    }

    fn points(&self, key: &str) -> i32 {
        self.state
            .players
            .get(key)
            .map_or(self.config.start_points, |p| p.points)
    }

    /// `winner` beat `loser` in a duel for `target`'s headband (or by a
    /// forfeit): points, wins, and the ladder.
    fn result(
        &mut self,
        winner: &str,
        loser: &str,
        target: Target,
        reason: Option<&str>,
        now: i64,
        out: &mut Vec<Out>,
    ) {
        let gain = elo_change(self.points(winner), self.points(loser), self.config.elo_k, false);
        if let Some(p) = self.state.players.get_mut(winner) {
            p.wins += 1;
            p.points += gain;
        }
        if let Some(p) = self.state.players.get_mut(loser) {
            p.losses += 1;
            p.points -= gain;
        }
        self.dirty = true;
        let before = self.state.ladder.first().cloned();
        let (rw, rl) = (self.rank(winner), self.rank(loser));
        // Only the challenger climbs: a defender who wins keeps their band.
        let climbed = match (target, rw, rl) {
            (Target::NumberOne, Some(2), Some(1)) => {
                self.state.ladder.swap(0, 1);
                Some(1)
            }
            (Target::NumberTwo, Some(w), Some(2)) if w > 2 => {
                match self.config.loser_placement {
                    Placement::Swap => self.state.ladder.swap(1, w - 1),
                    Placement::Shift => {
                        let k = self.state.ladder.remove(w - 1);
                        self.state.ladder.insert(1, k);
                    }
                }
                Some(2)
            }
            _ => None,
        };
        let why = reason.map(|r| format!(" ({r})")).unwrap_or_default();
        match climbed {
            Some(r) => {
                self.ladder_changed(before, now, out);
                self.announce(
                    format!(
                        "§6⚔ {} переміг {}{why} і забирає пов'язку {}§6! (+{gain} оч.)",
                        self.name(winner),
                        self.name(loser),
                        title(r)
                    ),
                    out,
                );
            }
            None => {
                let band = self.rank(winner).map(title).unwrap_or("");
                self.announce(
                    format!(
                        "§6⚔ {} {}переміг {}{why} (+{gain} оч.).",
                        self.name(winner),
                        if band.is_empty() { String::new() } else { format!("{band}§6 ") },
                        self.name(loser)
                    ),
                    out,
                );
            }
        }
    }

    /// Number One's reign: the milestones reached, and the rewards (none
    /// yet).
    fn reign(&mut self, now: i64, out: &mut Vec<Out>) {
        let (Some(one), Some(since)) = (self.state.ladder.first().cloned(), self.state.number_one_since)
        else {
            return;
        };
        let days = ((now - since).max(0) / DAY) as u32;
        let reached: Vec<u32> = self
            .config
            .number_one
            .milestone_days
            .iter()
            .copied()
            .filter(|m| *m <= days)
            .collect();
        let Some(p) = self.state.players.get_mut(&one) else {
            return;
        };
        let new: Vec<u32> = reached.into_iter().filter(|m| !p.milestones.contains(m)).collect();
        if new.is_empty() {
            return;
        }
        p.milestones.extend(&new);
        self.dirty = true;
        for m in new {
            self.announce(
                format!("§6⚔ {} носить пов'язку Номер Один уже {m} дн.!", self.name(&one)),
                out,
            );
            let rewards = self.number_one_rewards(&one, m);
            out.extend(rewards);
        }
    }

    /// The extension point for Number One's bonuses: called once for each
    /// `number_one.milestone_days` the current Number One reaches. Nothing
    /// is given yet; a later version returns its rewards here (items,
    /// effects, a title) when `number_one.bonuses_enabled` is on.
    pub fn number_one_rewards(&self, _player: &str, _days: u32) -> Vec<Out> {
        if !self.config.number_one.bonuses_enabled {
            return Vec::new();
        }
        Vec::new()
    }

    // -- the table -----------------------------------------------------------

    pub fn table(&self, page: u32, me: Option<&str>, now: i64) -> Vec<String> {
        let size = self.config.table_page_size.max(1) as usize;
        let total = self.state.ladder.len();
        let pages = total.div_ceil(size).max(1);
        let page = (page as usize).clamp(1, pages);
        let mut lines = vec![format!("§6§l⚔ Рейтинг пов'язок §r§7(стор. {page}/{pages})")];
        if total == 0 {
            lines.push("§7Рейтинг порожній. Перший, хто виконає §e/duel challenge§7, забере пов'язку Номер Один.".into());
            return lines;
        }
        for (i, key) in self.state.ladder.iter().enumerate().skip((page - 1) * size).take(size) {
            lines.push(self.row(i + 1, key, me));
        }
        if let (Some(since), Some(_)) = (self.state.number_one_since, self.state.ladder.first()) {
            lines.push(format!("§7Номер Один носить пов'язку вже {}.", format_left(now - since)));
        }
        if let Some(me) = me {
            match self.rank(me) {
                Some(r) if (r - 1) / size + 1 != page => lines.push(self.row(r, me, Some(me))),
                None => lines.push("§7Тебе ще немає в рейтингу: §e/duel join§7 або §e/duel challenge".into()),
                _ => {}
            }
        }
        lines
    }

    fn row(&self, rank: usize, key: &str, me: Option<&str>) -> String {
        let p = self.state.players.get(key).cloned().unwrap_or_default();
        let colour = match rank {
            1 => "§6",
            2 => "§c",
            _ => "§e",
        };
        let band = match rank {
            1 => " §6[Пов'язка №1]",
            2 => " §c[Пов'язка №2]",
            _ => "",
        };
        let you = if me == Some(key) { " §a← ти" } else { "" };
        format!(
            "{colour}#{rank} §f{} §7— §b{} оч. §7(В {} / П {} / Н {}){band}{you}",
            self.name(key),
            p.points,
            p.wins,
            p.losses,
            p.draws
        )
    }

    pub fn stats(&self, key: &str, now: i64) -> Vec<String> {
        let p = self.state.players.get(key).cloned().unwrap_or_default();
        let mut lines = vec![format!("§6§l⚔ {}", self.name(key))];
        lines.push(match self.rank(key) {
            Some(r) => format!(
                "§7Місце: §e#{r}{}",
                match r {
                    1 => " §6(Номер Один)",
                    2 => " §c(Номер Два)",
                    _ => "",
                }
            ),
            None => "§7Не в рейтингу".into(),
        });
        lines.push(format!(
            "§7Очки: §b{}§7, перемоги: §a{}§7, поразки: §c{}§7, нічиї: §f{}§7, відмови: §f{}",
            p.points, p.wins, p.losses, p.draws, p.declines
        ));
        if let Some(b) = p.best_rank {
            lines.push(format!("§7Найкраще місце: §e#{b}"));
        }
        let mut reign = p.number_one_ms;
        if self.rank(key) == Some(1)
            && let Some(since) = self.state.number_one_since
        {
            reign += (now - since).max(0);
            lines.push(format!("§6Носить пов'язку Номер Один уже {}.", format_left(now - since)));
        }
        if reign > 0 {
            lines.push(format!("§7Усього часу Номером Один: {}", format_left(reign)));
        }
        lines
    }
}

/// `3 дн. 4 год.`, `5 год. 2 хв.`, `40 с`.
pub fn format_left(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    let (d, h, m) = (s / 86_400, s % 86_400 / 3600, s % 3600 / 60);
    if d > 0 {
        format!("{d} дн. {h} год.")
    } else if h > 0 {
        format!("{h} год. {m} хв.")
    } else if m > 0 {
        format!("{m} хв.")
    } else {
        format!("{s} с")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_800_000_000_000;

    fn body(n: u128, name: &str) -> Body {
        Body {
            uuid: n,
            name: name.into(),
            dimension: "minecraft:overworld".into(),
            pos: [1000.0 + n as f64, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            alive: true,
        }
    }
    fn actor(b: &Body, permission: u8) -> Actor {
        Actor {
            body: Some(b.clone()),
            permission,
        }
    }
    fn k(n: u128) -> String {
        uuid_string(n)
    }
    fn arena() -> Arena {
        let spot = |x: f64| Spot {
            dimension: "minecraft:overworld".into(),
            pos: [x, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
        };
        Arena {
            dimension: "minecraft:overworld".into(),
            center: [0.5, 64.0, 0.5],
            radius: 10.0,
            spawns: [Some(spot(-5.0)), Some(spot(5.0))],
            enabled: true,
        }
    }
    /// A ladder of `n` players, 1..=n in order, and one arena.
    fn world(n: u128) -> (Duels, Vec<Body>) {
        let (mut d, _) = Duels::new(Config::default(), State::default(), T0);
        let bodies: Vec<Body> = (1..=n).map(|i| body(i, &format!("P{i}"))).collect();
        for b in &bodies {
            d.seen(b.uuid, &b.name, T0);
            d.state.ladder.push(k(b.uuid));
        }
        d.state.arenas.insert("a".into(), arena());
        d.state.number_one_since = Some(T0);
        (d, bodies)
    }
    fn run(d: &mut Duels, a: &Body, cmd: &str, now: i64) -> Vec<Out> {
        let p = if cmd.starts_with("duelsadmin") { 4 } else { 0 };
        d.command(&actor(a, p), parse_command(cmd).unwrap(), now)
    }

    #[test]
    fn uuids_round_trip() {
        let u = 0x0123_4567_89ab_cdef_0011_2233_4455_6677u128;
        assert_eq!(uuid_string(u), "01234567-89ab-cdef-0011-223344556677");
        assert_eq!(parse_uuid(&uuid_string(u)), Some(u));
        assert_eq!(parse_uuid("nope"), None);
    }

    #[test]
    fn parses_commands() {
        assert_eq!(parse_command("duel"), Some(Cmd::Top(1)));
        assert_eq!(parse_command("duel top 3"), Some(Cmd::Top(3)));
        assert!(matches!(parse_command("duel top x"), Some(Cmd::Usage(_))));
        assert_eq!(parse_command("duel accept Steve"), Some(Cmd::Accept(Some("Steve".into()))));
        assert_eq!(parse_command("duel accept"), Some(Cmd::Accept(None)));
        assert_eq!(
            parse_command("duelsadmin arena create Pit 12"),
            Some(Cmd::ArenaCreate("pit".into(), 12))
        );
        assert_eq!(parse_command("duelsadmin arena spawn pit 2"), Some(Cmd::ArenaSpawn("pit".into(), 2)));
        assert_eq!(parse_command("duelsadmin setrank Bob 1"), Some(Cmd::SetRank("Bob".into(), 1)));
        assert!(matches!(parse_command("duelsadmin"), Some(Cmd::Usage(_))));
        assert_eq!(parse_command("tp Bob"), None);
        assert!(Cmd::Reload.admin() && !Cmd::Challenge.admin());
    }

    #[test]
    fn every_usage_parses() {
        for (usage, _) in COMMANDS {
            let line: String = usage
                .split_whitespace()
                .map(|w| match w {
                    w if w.contains(":integer") => "1",
                    w if w.starts_with('<') || w.starts_with('[') => "x",
                    w => w,
                })
                .collect::<Vec<_>>()
                .join(" ");
            let cmd = parse_command(&line).unwrap();
            assert!(!matches!(cmd, Cmd::Usage(_)), "{line}");
        }
    }

    #[test]
    fn first_two_claim_empty_headbands() {
        let (mut d, _) = Duels::new(Config::default(), State::default(), T0);
        let (a, b, c) = (body(1, "A"), body(2, "B"), body(3, "C"));
        run(&mut d, &a, "duel challenge", T0);
        assert_eq!(d.rank(&k(1)), Some(1));
        run(&mut d, &b, "duel challenge", T0);
        assert_eq!(d.rank(&k(2)), Some(2));
        assert!(d.state.challenges.is_empty());
        // The third challenges Number Two.
        run(&mut d, &c, "duel challenge", T0);
        assert_eq!(d.state.challenges.len(), 1);
        assert_eq!(d.state.challenges[0].to, k(2));
        assert_eq!(d.state.challenges[0].target, Target::NumberTwo);
        assert_eq!(d.rank(&k(3)), Some(3));
    }

    #[test]
    fn number_one_cannot_be_challenged_except_by_two() {
        let (mut d, b) = world(4);
        let out = run(&mut d, &b[0], "duel challenge", T0);
        assert!(d.state.challenges.is_empty());
        assert!(matches!(&out[0], Out::Tell(1, l) if l.contains("Номер Один")));
        // #4 can only reach #2.
        run(&mut d, &b[3], "duel challenge", T0);
        assert_eq!(d.state.challenges[0].to, k(2));
        // #2 reaches #1.
        run(&mut d, &b[1], "duel challenge", T0);
        assert_eq!(d.state.challenges[1].to, k(1));
        assert_eq!(d.state.challenges[1].target, Target::NumberOne);
        // One outgoing challenge at a time.
        let out = run(&mut d, &b[3], "duel challenge", T0);
        assert_eq!(d.state.challenges.len(), 2);
        assert!(matches!(&out[0], Out::Tell(4, l) if l.contains("вже викликав")));
    }

    #[test]
    fn challenge_expires_after_three_days() {
        let (mut d, b) = world(3);
        run(&mut d, &b[2], "duel challenge", T0);
        d.tick(&b, &[], T0 + 71 * HOUR);
        assert_eq!(d.state.challenges.len(), 1);
        let out = d.tick(&b, &[], T0 + 72 * HOUR);
        assert!(d.state.challenges.is_empty());
        assert!(out.iter().any(|o| matches!(o, Out::Tell(3, l) if l.contains("минув"))));
        assert_eq!(d.rank(&k(3)), Some(3));
    }

    #[test]
    fn expiry_as_forfeit_moves_the_band() {
        let (mut d, b) = world(3);
        d.config.expired_challenge_is_forfeit = true;
        run(&mut d, &b[2], "duel challenge", T0);
        d.tick(&b, &[], T0 + 73 * HOUR);
        assert_eq!(d.state.ladder, vec![k(1), k(3), k(2)]);
    }

    #[test]
    fn decline_removes_the_challenge() {
        let (mut d, b) = world(3);
        d.config.decline_penalty_points = 5;
        run(&mut d, &b[2], "duel challenge", T0);
        run(&mut d, &b[1], "duel decline", T0);
        assert!(d.state.challenges.is_empty());
        assert_eq!(d.state.players[&k(2)].declines, 1);
        assert_eq!(d.state.players[&k(2)].points, 995);
    }

    /// #3 challenges #2, #2 accepts: they go to the arena and fight.
    fn start(d: &mut Duels, b: &[Body]) -> Vec<Out> {
        run(d, &b[2], "duel challenge", T0);
        run(d, &b[1], "duel accept", T0);
        d.tick(b, &[], T0 + 1000)
    }

    #[test]
    fn accepted_duel_teleports_both_to_the_arena() {
        let (mut d, b) = world(3);
        let out = start(&mut d, &b);
        assert!(d.state.challenges.is_empty());
        assert_eq!(d.state.duels.len(), 1);
        let tps: Vec<&Out> = out.iter().filter(|o| matches!(o, Out::Teleport(..))).collect();
        assert_eq!(tps.len(), 2);
        assert!(matches!(tps[0], Out::Teleport(3, s) if s.pos[0] == -5.0));
        assert!(matches!(tps[1], Out::Teleport(2, s) if s.pos[0] == 5.0));
        // Where they were, to go back.
        assert_eq!(d.state.duels[0].back[&k(3)].pos, b[2].pos);
    }

    #[test]
    fn no_arena_waits_then_gives_up() {
        let (mut d, b) = world(3);
        d.state.arenas.clear();
        run(&mut d, &b[2], "duel challenge", T0);
        run(&mut d, &b[1], "duel accept", T0);
        let out = d.tick(&b, &[], T0 + 1000);
        assert!(d.state.duels.is_empty());
        assert!(out.iter().any(|o| matches!(o, Out::Tell(_, l) if l.contains("Жодної"))));
        // Told once.
        let out = d.tick(&b, &[], T0 + 2000);
        assert!(!out.iter().any(|o| matches!(o, Out::Tell(..))));
        d.tick(&b, &[], T0 + 6 * MINUTE);
        assert_eq!(d.state.challenges[0].accepted, None);
    }

    #[test]
    fn offline_challenger_puts_the_challenge_back() {
        let (mut d, b) = world(3);
        run(&mut d, &b[2], "duel challenge", T0);
        run(&mut d, &b[1], "duel accept", T0);
        d.tick(&b[..2], &[], T0 + 1000);
        assert!(d.state.duels.is_empty());
        assert_eq!(d.state.challenges[0].accepted, None);
    }

    #[test]
    fn lower_player_beating_two_takes_two_shift() {
        let (mut d, b) = world(5);
        // #5 challenges.
        run(&mut d, &b[4], "duel challenge", T0);
        run(&mut d, &b[1], "duel accept", T0);
        d.tick(&b, &[], T0 + 1000);
        d.tick(&b, &[], T0 + 7000); // fight
        let out = d.tick(&b, &[2], T0 + 8000); // #2 dies
        assert_eq!(d.state.ladder, vec![k(1), k(5), k(2), k(3), k(4)]);
        assert!(out.iter().any(|o| matches!(o, Out::Broadcast(l) if l.contains("Номер Два"))));
        assert_eq!(d.state.players[&k(5)].wins, 1);
        assert_eq!(d.state.players[&k(2)].losses, 1);
        assert_eq!(d.state.players[&k(5)].points, 1016);
        assert_eq!(d.state.players[&k(2)].points, 984);
        assert_eq!(d.state.players[&k(5)].best_rank, Some(2));
        // The winner goes back after the delay; the dead loser doesn't.
        let mut inside = b.clone();
        inside[4].pos = [-5.0, 64.0, 0.0];
        let out = d.tick(&inside, &[], T0 + 8000 + 15_000);
        assert!(d.state.duels.is_empty());
        let tps: Vec<&Out> = out.iter().filter(|o| matches!(o, Out::Teleport(..))).collect();
        assert_eq!(tps.len(), 1);
        assert!(matches!(tps[0], Out::Teleport(5, s) if s.pos == b[4].pos));
    }

    #[test]
    fn swap_placement() {
        let (mut d, b) = world(5);
        d.config.loser_placement = Placement::Swap;
        run(&mut d, &b[4], "duel challenge", T0);
        run(&mut d, &b[1], "duel accept", T0);
        d.tick(&b, &[], T0 + 1000);
        d.tick(&b, &[2], T0 + 2000);
        assert_eq!(d.state.ladder, vec![k(1), k(5), k(3), k(4), k(2)]);
    }

    #[test]
    fn defender_win_keeps_the_ladder_and_blocks_rematch() {
        let (mut d, b) = world(3);
        start(&mut d, &b);
        d.tick(&b, &[3], T0 + 2000);
        assert_eq!(d.state.ladder, vec![k(1), k(2), k(3)]);
        assert_eq!(d.state.players[&k(2)].wins, 1);
        d.tick(&b, &[], T0 + 30_000);
        assert!(d.state.duels.is_empty());
        let out = run(&mut d, &b[2], "duel challenge", T0 + 60_000);
        assert!(d.state.challenges.is_empty());
        assert!(matches!(&out[0], Out::Tell(3, l) if l.contains("нещодавно")));
        run(&mut d, &b[2], "duel challenge", T0 + 31 * MINUTE);
        assert_eq!(d.state.challenges.len(), 1);
    }

    #[test]
    fn two_beats_one_and_reign_moves() {
        let (mut d, b) = world(3);
        run(&mut d, &b[1], "duel challenge", T0);
        // #3 challenges #2 too.
        run(&mut d, &b[2], "duel challenge", T0);
        run(&mut d, &b[0], "duel accept", T0);
        d.tick(&b, &[], T0 + 1000);
        assert_eq!(d.state.duels[0].target, Target::NumberOne);
        d.tick(&b, &[1], T0 + 3 * DAY);
        assert_eq!(d.state.ladder, vec![k(2), k(1), k(3)]);
        assert_eq!(d.state.number_one_since, Some(T0 + 3 * DAY));
        assert!(d.state.players[&k(1)].number_one_ms >= 3 * DAY - 1000);
        // #3's challenge was to the old Number Two (P2), who is now Number
        // One: it is called off.
        assert!(d.state.challenges.is_empty());
    }

    #[test]
    fn both_die_is_a_draw() {
        let (mut d, b) = world(3);
        start(&mut d, &b);
        d.tick(&b, &[2, 3], T0 + 2000);
        assert_eq!(d.state.ladder, vec![k(1), k(2), k(3)]);
        assert_eq!(d.state.players[&k(2)].draws, 1);
        // Nobody to send back.
        let out = d.tick(&b, &[], T0 + 60_000);
        assert!(!out.iter().any(|o| matches!(o, Out::Teleport(..))));
    }

    #[test]
    fn leaving_forfeits_and_returns_on_login() {
        let (mut d, b) = world(3);
        start(&mut d, &b);
        let without_two = vec![b[0].clone(), b[2].clone()];
        d.tick(&without_two, &[], T0 + 7000);
        assert_eq!(d.state.ladder, vec![k(1), k(3), k(2)]);
        d.tick(&without_two, &[], T0 + 30_000);
        assert_eq!(d.state.returns[&k(2)].pos, b[1].pos);
    }

    #[test]
    fn walking_out_is_put_back() {
        let (mut d, b) = world(3);
        start(&mut d, &b);
        let mut moved = b.clone();
        moved[2].pos = [-5.0, 64.0, 0.0];
        moved[1].pos = [50.0, 64.0, 0.0];
        // Not during the countdown.
        let out = d.tick(&moved, &[], T0 + 2000);
        assert!(!out.iter().any(|o| matches!(o, Out::Teleport(..))));
        d.tick(&moved, &[], T0 + 7000);
        let out = d.tick(&moved, &[], T0 + 8000);
        assert!(out.iter().any(|o| matches!(o, Out::Teleport(2, s) if s.pos[0] == 5.0)));
        assert!(!out.iter().any(|o| matches!(o, Out::Teleport(3, _))));
    }

    #[test]
    fn time_limit_is_a_draw() {
        let (mut d, b) = world(3);
        start(&mut d, &b);
        let mut inside = b.clone();
        inside[1].pos = [5.0, 64.0, 0.0];
        inside[2].pos = [-5.0, 64.0, 0.0];
        d.tick(&inside, &[], T0 + 7000);
        d.tick(&inside, &[], T0 + 7000 + 10 * MINUTE);
        assert!(matches!(d.state.duels[0].phase, Phase::Ending { .. }));
        assert_eq!(d.state.players[&k(3)].draws, 1);
        let out = d.tick(&inside, &[], T0 + 7000 + 11 * MINUTE);
        assert_eq!(out.iter().filter(|o| matches!(o, Out::Teleport(..))).count(), 2);
    }

    #[test]
    fn restart_mid_duel_is_a_draw_and_sends_back() {
        let (mut d, b) = world(3);
        start(&mut d, &b);
        let json = serde_json::to_string(&d.state).unwrap();
        let state: State = serde_json::from_str(&json).unwrap();
        let (d2, out) = Duels::new(Config::default(), state, T0 + HOUR);
        assert!(d2.state.duels.is_empty());
        assert_eq!(d2.state.returns.len(), 2);
        assert_eq!(d2.state.notices[&k(2)].len(), 1);
        assert!(matches!(&out[0], Out::Log(_)));
        assert_eq!(d2.state.ladder, vec![k(1), k(2), k(3)]);
    }

    #[test]
    fn arenas_by_operators() {
        let (mut d, _) = Duels::new(Config::default(), State::default(), T0);
        let mut op = body(9, "Op");
        op.pos = [100.3, 70.0, -20.7];
        let out = d.command(&actor(&op, 0), parse_command("duelsadmin arena create pit 10").unwrap(), T0);
        assert!(matches!(&out[0], Out::Tell(9, l) if l.contains("операторів")));
        run(&mut d, &op, "duelsadmin arena create pit 10", T0);
        assert_eq!(d.state.arenas["pit"].center, [100.5, 70.0, -20.5]);
        run(&mut d, &op, "duelsadmin arena spawn pit 1", T0);
        assert!(!d.state.arenas["pit"].ready());
        op.pos = [200.0, 70.0, 0.0];
        run(&mut d, &op, "duelsadmin arena spawn pit 2", T0);
        assert!(!d.state.arenas["pit"].ready(), "outside");
        op.pos = [105.0, 70.0, -25.0];
        run(&mut d, &op, "duelsadmin arena spawn pit 2", T0);
        assert!(d.state.arenas["pit"].ready());
        run(&mut d, &op, "duelsadmin arena disable pit", T0);
        assert!(!d.state.arenas["pit"].ready());
        run(&mut d, &op, "duelsadmin arena delete pit", T0);
        assert!(d.state.arenas.is_empty());
        let out = run(&mut d, &op, "duelsadmin arena create Bad!Name 10", T0);
        assert!(matches!(&out[0], Out::Tell(9, l) if l.contains("Назва")));
    }

    #[test]
    fn setrank_and_remove() {
        let (mut d, b) = world(4);
        run(&mut d, &b[3], "duel challenge", T0);
        run(&mut d, &b[0], "duelsadmin setrank P4 1", T0);
        assert_eq!(d.state.ladder, vec![k(4), k(1), k(2), k(3)]);
        // P4's challenge to P2 no longer holds.
        assert!(d.state.challenges.is_empty());
        run(&mut d, &b[0], "duelsadmin remove p4", T0);
        assert_eq!(d.state.ladder, vec![k(1), k(2), k(3)]);
    }

    #[test]
    fn milestones_and_table() {
        let (mut d, b) = world(12);
        let out = d.tick(&b, &[], T0 + 8 * DAY);
        assert!(out.iter().any(|o| matches!(o, Out::Broadcast(l) if l.contains("7 дн"))));
        assert_eq!(d.state.players[&k(1)].milestones, vec![7]);
        let out = d.tick(&b, &[], T0 + 8 * DAY + 1000);
        assert!(!out.iter().any(|o| matches!(o, Out::Broadcast(_))));
        let t = d.table(1, Some(&k(12)), T0);
        assert!(t[0].contains("1/2"));
        assert!(t[1].contains("#1") && t[1].contains("P1"));
        assert!(t.last().unwrap().contains("#12") && t.last().unwrap().contains("ти"));
        assert_eq!(d.table(9, None, T0)[0], d.table(2, None, T0)[0]);
    }

    #[test]
    fn config_fills_missing_keys() {
        let c: Config = serde_json::from_str(r#"{"countdown_seconds": 3, "loser_placement": "swap"}"#).unwrap();
        assert_eq!(c.countdown_seconds, 3);
        assert_eq!(c.loser_placement, Placement::Swap);
        assert_eq!(c.challenge_expiry_hours, 72);
    }

    #[test]
    fn elo() {
        assert_eq!(elo_change(1000, 1000, 32.0, false), 16);
        assert_eq!(elo_change(1000, 1000, 32.0, true), 0);
        assert!(elo_change(1400, 1000, 32.0, false) < 4);
    }

    #[test]
    fn formats_time() {
        assert_eq!(format_left(3 * DAY), "3 дн. 0 год.");
        assert_eq!(format_left(2 * HOUR + 5 * MINUTE), "2 год. 5 хв.");
        assert_eq!(format_left(40_000), "40 с");
    }
}
