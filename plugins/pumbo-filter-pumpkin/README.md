# PumboFilter

PumboFilter checks that a new connection is a real Minecraft client before it plays on a [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin) server: a gravity check, a CAPTCHA, a client check and an attack mode for floods. It is written in Rust and runs as a WebAssembly plugin on the official Pumpkin releases. The rules live in `pumbo-filter-core`, so the same checks will run on the PumboProx proxy later.

PumboFilter works on its own. When [PumboAuth](../pumbo-auth-pumpkin/README.md) is installed too, a player first passes PumboFilter and then logs in with PumboAuth, on the same connection.

## Features

- **Gravity check**: the player falls for 128 ticks; the reported heights must follow the free fall of the vanilla client.
- **CAPTCHA**: a distorted code on a map in the player's hand (or in a title), answered in chat or with `/captcha <code>`.
- **Client check**: real clients send their brand and settings; brands can be refused.
- **Attack mode**: stricter checks when many players connect at once, and a "join twice" gate for unknown players during heavy floods.
- **Verified cache**: a nickname that passed from an address is let in directly for `verified-ttl-hours`.
- **Premium and Bedrock**: players authenticated by Mojang skip the checks (except during an attack), Bedrock players skip the checks they cannot pass.
- **Nothing changes in the player's data**: the player never changes worlds, and the only thing the check changes on the server is the position during the fall (see below).
- English and Polish messages, the shared Pumbo look.

## Compatibility

| File | Pumpkin | Minecraft |
| --- | --- | --- |
| `PumboFilter-26.3.wasm` | release `0.2.0+26.3-26.51` | 26.3 |
| `PumboFilter-26.2.wasm` | release `0.1.0-dev+26.2-26.45` | 26.2 |

Pumpkin checks the plugin API strictly: a file only loads on the server version it was built for. No source build or server hooks are needed.

## How the check works

1. The player joins where the server puts them (their saved place, or the spawn). From the first moment the screen is dark: a blindness effect is sent to their client only, the server does not have it. Other players do not see them; they cannot be hurt and cannot use, move or drop anything.
2. When the client has confirmed its spawn (its first movement), PumboFilter stores the player's real place in its database and moves them 1000 blocks above the build limit of their own world, straight above their place. There are no blocks up there, so they fall freely for about seven seconds while the check compares the movement with the vanilla physics. The action bar shows the progress.
3. Afterwards they are put back on their real place. If the check failed, or the client did not move, they answer a CAPTCHA there; walking away from that place puts them back.
4. Then the screen gets light again and they play, where they joined. With PumboAuth the screen stays dark and PumboAuth asks them to log in.

### The player's data file

Pumpkin writes the player's data file when they leave, at every autosave and at `save-all`/`stop`. The check never changes the player's world, game mode, experience, abilities, effects, health or inventory. The position is the only thing that is different while they fall, and:

- when a player leaves during the fall (they quit, are kicked, the server stops, a proxy moves them to another server), PumboFilter puts them back on their place in the leave event, before the server writes the file;
- when the server crashes after an autosave caught a player in the air, the place stored in PumboFilter's database brings them back at their next join, whether or not they have to be checked again. The record is removed once a join finds the player at a normal height.

So the data file never stays in the air after a normal leave, and a crash costs nothing either. The way back is always written to the database before the player is moved; without a working database the gravity check is skipped (the CAPTCHA is used instead).

### Limitations

- The player's real inventory stays visible during the check (the hotbar and the inventory screen). Released Pumpkin lets plugins send no inventory packets, so it cannot be hidden. Nothing in it can be used, moved or dropped, and nothing is changed, so nothing is lost or copied (not by leaving, autosaves, crashes or a second connection, which is refused).
- `mode = "map"`: Pumpkin sends map pixels to every online player (about 16 KB per CAPTCHA; only the holder sees them), and the chunk where a player answered keeps an invisible map block entity (16 KB, at the bottom of the chunk, one per chunk). The map only goes into an empty main hand; players holding something get the code as a title. `mode = "title"` avoids both, but a title is easier for a bot to read.
- A player whose client never moves after joining (some bots, a player in a vehicle) gets the CAPTCHA instead of the gravity check.
- PumboFilter sees connections only after Pumpkin has finished the login handshake; floods of handshakes have to be stopped by a proxy or a firewall.
- If the plugin is unloaded (`plugin unload`), Pumpkin lets it neither move nor disconnect players. Players in the check are released where they are and asked to join again; a player in the air stops falling on their own screen (and cannot get hurt). Their data file then has the position in the air, and their next join puts them back from the database, so load PumboFilter again before that (`plugin load`) or restart the server. Stopping the server is not affected: players leave before plugins are unloaded. If the plugin crashes, the players it holds stay in the dark until they reconnect.

## Installation

1. Put the matching `.wasm` file into the `plugins/` folder of the server.
2. Start the server. PumboFilter writes `plugins/data/pumbofilter/config.yml`, `lang/en.yml` and `lang/pl.yml`, and keeps its data in `plugins/data/pumbofilter/filter.redb`.

The plugin needs the `fs.read.data` and `fs.write.data` permissions of Pumpkin's plugin sandbox and nothing else.

Behind a proxy (PumboProx, Velocity), install it on the server players join first. The real IP and the premium profile come from the forwarding.

Note: Pumpkin 0.2.0 and 0.1.0-dev hang on "Starting save." when they get SIGINT (also without plugins); stop them with the `stop` command.

## Building

```sh
./build.sh          # dist/PumboFilter-26.3.wasm and dist/PumboFilter-26.2.wasm (at the workspace root)
cargo test -p pumbo-filter-core -p pumbo-filter-pumpkin
```

Rust stable with the `wasm32-wasip2` target is required.

## Commands and permissions

| Command | Description | Permission | Default |
| --- | --- | --- | --- |
| `/captcha <code>` | answer the CAPTCHA (chat works too) | `pumbofilter:captcha` | everyone |
| `/pumbofilter help [page]` | list of the commands you may use | `pumbofilter:command` | operators (3) |

`/pf` is short for `/pumbofilter`.
| `/pumbofilter reload` | reload the config and messages | `pumbofilter:reload` | operators |
| `/pumbofilter stats` | players held now, checks, attack mode, database | `pumbofilter:stats` | operators |
| `/pumbofilter verify <nick> <ip>` | add a nickname + address to the verified cache | `pumbofilter:verify` | operators |
| `/pumbofilter unverify <nick>` | check a nickname again next time | `pumbofilter:unverify` | operators |
| `/pumbofilter attack <on\|off\|auto>` | force attack mode | `pumbofilter:attack` | operators |
| `/pumbofilter version` | version and platform | `pumbofilter:version` | operators |
| (no command) | skip the checks | `pumbofilter:bypass` | nobody |

## Configuration

`plugins/data/pumbofilter/config.yml` (reload with `/pumbofilter reload`). The file is written with short comments on first start, the most used options at the top; the details are here.

A player who has to be checked is held: the screen is dark (on the client only), they are hidden from others and cannot be hurt or use anything. They fall high above their own place for the gravity check (never in another world) and answer a CAPTCHA at their real place. After the checks they play where they joined. PumboAuth, when installed, takes over for the login.

| Option | Default | Meaning |
| --- | --- | --- |
| `language` | `en` | language of the messages: `en`, `pl` or any code with a `lang/<code>.yml` file |
| `filter.check-mode` | `GRAVITY_THEN_CAPTCHA` | checks of a new player: `ALWAYS` (gravity and CAPTCHA), `ONLY_GRAVITY`, `ONLY_CAPTCHA`, `GRAVITY_THEN_CAPTCHA` (the CAPTCHA only after a failed or impossible gravity check) or `NEVER` |
| `filter.attack-check-mode` | `ALWAYS` | the same choice during an attack (many connections at once) |
| `filter.skip-premium` | `true` | players authenticated by Mojang skip the checks, except during an attack. They are recognised by a version 4 UUID with a signed skin, which only a server in online mode or authenticated proxy forwarding (Velocity modern, PumboProx, BungeeGuard, Vine) gives. Turn it off behind BungeeCord forwarding without BungeeGuard |
| `filter.verified-cache` / `verified-ttl-hours` | `true` / `12` | remember players who passed (nickname + address) and let them in directly for this long |
| `filter.whitelist` | none | `"Nick@1.2.3.4"` pairs that are never checked |
| `captcha.mode` | `map` | `map`: the code is drawn on a map in the player's empty main hand (players holding something get the title); `title`: the code is shown as a title. Map pixels reach every online player (only the holder sees them), and the chunk where a player answered keeps an invisible map block entity (16 KB) |
| `captcha.attempts` / `timeout-seconds` | `2` / `30` | wrong answers and time before the player is disconnected |
| `captcha.length`, `alphabet`, `ignore-case` | `3`, letters that are easy to tell apart, `true` | the codes |
| `captcha.pool-size` / `regenerate-minutes` | `200` / `60` | images drawn in advance and how often they are replaced |
| `captcha.curves` / `noise-dots` | `3` / `220` | lines and dots drawn over the code |
| `gravity.falling-check-ticks` | `128` | ticks of free fall that must match the vanilla client (128 = 6.4 s) |
| `gravity.falling-grace-ticks` | `100` | extra ticks for reports lost at the start; then the check times out |
| `gravity.max-y-difference` | `0.01` | allowed difference from the expected height, in blocks |
| `gravity.max-y-errors` / `max-xz-errors` | `10` / `10` | how many differences are tolerated |
| `gravity.debug` | `false` | log the result of every gravity check |
| `client-check.check-brand` / `check-settings` | `true` / `true` | real clients send their brand (`vanilla`, `fabric`, ...) and their settings |
| `client-check.check-ticks` | `60` | when to check, in ticks after joining |
| `client-check.blocked-brands` | none | brands that are refused (case-insensitive parts of the name) |
| `attack.window-seconds` / `threshold` | `5` / `25` | from this many connections within the window: attack mode (`attack-check-mode`) |
| `attack.reconnect-threshold` | `80` | from this many: unknown players also have to join twice, the second time between `reconnect-min-seconds` (2) and `reconnect-max-seconds` (120) after the first |
| `attack.cooldown-seconds` | `60` | attack mode ends this long after the rate drops |
| `attack.log-interval-seconds` | `10` | how often the attack is logged |

Every section has `enabled` (default `true`) to turn its check off. A wrong value is reported in the server log with the option name and replaced by the default.

## Working with other plugins

- **PumboAuth**: both plugins ask each other over Pumpkin's `ipc` whether they still hold a player (`pumbo_common::gate`). PumboAuth waits until the check is done, then asks the player to log in; whichever plugin finishes last releases the player. Each works on its own when the other is missing or unloaded.
- **PumboBans**: banned players are refused before they enter the world, so they never reach the check.

## License

GPL-3.0-only, see [LICENSE](../../LICENSE).
