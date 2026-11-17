# Duels: a headband ladder for Horizon

A native Horizon Rust mod (`.hnmod`), server-side only. Players install nothing:
both ordinary Minecraft 26.3 clients and Horizon clients work.

Inspired by "Afro Samurai": the **Number One** and **Number Two** headbands.

## Rules

- The ranking is a ladder of places: `#1` wears the Number One headband, `#2` the Number Two headband.
- **Number One is untouchable**: only Number Two can challenge them.
- **Anyone can challenge Number Two.** A player who isn't ranked yet joins at the bottom after challenging.
- The challenged player has **3 real days** (72 h) to accept or decline. A challenge waits even if
  the player is offline: they see a message when they join.
- Once both have accepted (both must be online and alive), they are teleported to a **random free arena**:
  the challenger to point 1, the challenged to point 2. A 5 s countdown, then a **fight to the death**.
- Whoever leaves the arena bounds is put back on their point. Whoever leaves the server loses.
- **The challenger beats Number Two:** they become the new Number Two, and the former Number Two becomes `#3`
  (everyone in between moves down one place).
- **Number Two beats Number One:** they swap headbands.
- **The defender wins:** places don't change. The loser can challenge again after 30 min.
- The winner stays in the arena for another 15 s (to collect the loot), then is returned to where they were.
  The one who died respawns as usual (bed/spawn), **and their items drop in the arena**.
- Points: Elo (start 1000, K=32). They are for the table; only duels (and operators) change places.
- When a headband changes hands, old challenges for it are cancelled (both are notified).
- The first two players to run `/duel challenge` in an empty ranking take the free headbands without a fight.
  An operator can set places manually (`/duelsadmin setrank`).

## Commands

For everyone:

| Command | What it does |
|---|---|
| `/duel` or `/duel top [page]` | ranking table with points, wins/losses/draws |
| `/duel challenge` | challenge Number Two (if you are Number Two, Number One) |
| `/duel accept [player]` / `/duel decline [player]` | answer a challenge (the name is needed only when there are several) |
| `/duel cancel` | withdraw your challenge |
| `/duel list` | your incoming and outgoing challenges and the time left |
| `/duel stats [player]` | stats, best place, how long they were Number One |
| `/duel join` | join the ranking without a challenge |
| `/duel help` | help |

For operators (level 2+; `duelsadmin` also works in the server console, except commands that need a position):

| Command | What it does |
|---|---|
| `/duelsadmin arena create <name> <radius>` | an arena centered where you stand; size: a cube of `2·radius+1` (radius 3–256) |
| `/duelsadmin arena spawn <name> <1\|2>` | a spawn point where you stand (with your facing), inside the arena |
| `/duelsadmin arena enable\|disable <name>` | enable/disable an arena (e.g. for repairs) |
| `/duelsadmin arena delete <name>`, `arena list`, `arena tp <name>` | delete, list, teleport to the center |
| `/duelsadmin setrank <player> <place>` | put a player in a place |
| `/duelsadmin remove <player>` | remove from the ranking |
| `/duelsadmin stop <player>` | stop their duel (draw) |
| `/duelsadmin reload` | reload the config |

An arena is ready when both points are set. Build walls/barriers yourself: the mod only returns players who leave the bounds.

## Storage

Everything lives in the world folder, so server backups include it:

- `world/horizon-duels/state.json`: players, points, places, challenges, arenas, ongoing duels;
- `world/horizon-duels/config.json`: settings (created with defaults on first start).

The file is written atomically (temp file + rename) at most once per second and when the world closes.
A damaged file is never overwritten: it is moved aside as `state.json.bad-<time>`.
If the server stops in the middle of a duel: it is a draw, and the players are returned to where they were when they next join.

## Configuration (`config.json`)

| Key | Default | Meaning |
|---|---|---|
| `challenge_expiry_hours` | 72 | how long a challenge waits for an answer |
| `expired_challenge_is_forfeit` | false | `true`: no answer within 3 days = a loss (the challenger takes the headband without a fight) |
| `decline_penalty_points` | 0 | points taken for declining |
| `start_points` | 1000 | a newcomer's points |
| `elo_k` | 32 | the most points one duel can give/take |
| `loser_placement` | `"shift"` | `"shift"`: the former #2 becomes #3; `"swap"`: they swap places with the winner |
| `countdown_seconds` | 5 | countdown after the teleport |
| `max_duel_minutes` | 10 | fight time limit, then a draw (0: no limit) |
| `return_delay_seconds` | 15 | how long the winner stays in the arena |
| `disconnect_forfeits` | true | leaving the server during a fight = a loss (`false`: a draw) |
| `keep_in_arena` | true | return players who leave the arena bounds |
| `rematch_cooldown_minutes` | 30 | pause before challenging again the player you lost to |
| `arena_wait_minutes` | 5 | how long an accepted challenge waits for a free arena |
| `table_page_size` | 10 | rows per `/duel top` page |
| `announce` | true | results and headband changes in public chat |
| `number_one.bonuses_enabled`, `number_one.milestone_days` | false, `[7, 30, 100]` | bonuses for a long reign (see below) |

## Bonuses for a long-reigning Number One (for the future)

The mod already tracks how long Number One's reign lasts (and every player's total time). When the reign
reaches a number of days from `milestone_days`, an announcement goes to chat and
`Duels::number_one_rewards(player, days)` in `duels_core.rs` is called. Currently it gives nothing: rewards
(items, effects, a title) can be added there later.

## Building and a test server (Windows)

You need `horizon-launcher` and `horizon-server` and an installed Horizon build (as for other mods).

```powershell
powershell -ExecutionPolicy Bypass -File .\build-test-server.ps1 -Op <your nickname>
```

The script builds an engine with the mod into `test-server-engine\`, creates `test-server\` (port **25572**, offline, you as op)
and starts it. Another Horizon version: `-Version nightly-...` (`horizon-launcher versions --installed`).
`-NoRun`: only build and configure. The first build is slow (20–30 min).

Testing needs 2–3 players (offline mode: a second client under another nickname). What to check:

1. Op: `/duelsadmin arena create pit 12`, stand at one end `/duelsadmin arena spawn pit 1`, at the other `... spawn pit 2`.
2. Player A `/duel challenge` (becomes #1), B `/duel challenge` (becomes #2), C `/duel challenge` (challenges #2).
3. B `/duel accept`: B and C in the arena, countdown, fight. Kill one: the `/duel` table changed, the winner is returned after 15 s.
4. Restart the server: `/duel` shows the same.

For your own server: `horizon-server --dir YOUR_SERVER config set engine.path=...\test-server-engine\horizon-engine.exe`.

## Rule tests

```sh
cargo test
```

26 tests: challenges and permissions, 72 hours, declining, duel start and teleports, wins/losses/draws, shifting and swapping places,
leaving the server, leaving the arena bounds, the time limit, a restart mid-duel, arenas, `setrank`, Number One milestones, config.

## Files

```
mixins/duels/horizon-mixin.toml   manifest: 2 modules, 7 hooks
mixins/duels/duels_core.rs        rules, ranking, state, texts + tests (no game types)
mixins/duels/duels_glue.rs        glue to the game: files, commands, teleports, messages
mixins/duels/*.rs                 small hook helpers (register, perform, console, tick, died, login, lifecycle)
dist/duels-0.1.0.hnmod            ready-made package
build-test-server.ps1             builds an engine with the mod and a test server
tools/package.py                  packs dist/*.hnmod
Cargo.toml                        only for `cargo test`
```

The mod is pinned to Horizon Nightly 2026-11-03 (`2944480`).

## Limitations

- The dead player's items drop in the arena (vanilla death). This Horizon version has no hook to keep the inventory only in duels.
- The mod doesn't stop anyone from hitting Number One outside a duel: "untouchable" applies to challenges.
- The mod doesn't determine the killer: any death of a duelist during the fight (by the opponent, lava, falling) = a loss.

## License

MIT, see [LICENSE](LICENSE). A mod for [Horizon](https://github.com/horizon-rust/horizon) (MIT, Ludovic Marie and the Horizon contributors).

Unofficial project, not affiliated with Mojang Studios or Microsoft. Minecraft is a trademark of Mojang Synergies AB.
