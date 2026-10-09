# PumboFilter for PumboProx

PumboFilter on the PumboProx proxy: anti-bot checks before any server behind the proxy sees the player. The rules are the same as in PumboFilter for Pumpkin (`pumbo-filter-core`); here the checks happen in the proxy's virtual world, so no server world, player file or inventory is ever touched.

## What happens where

| When | What PumboFilter does |
| --- | --- |
| Login, before encryption (`on-pre-login`) | counts connections (attack mode), asks unknown players to join again during a heavy attack, refuses addresses that failed too often (`auto-ban`) |
| After login (gate `filter`, priority 100) | premium players (outside an attack), verified nickname + address pairs, the whitelist and `pumbo.filter.bypass` go on; the others enter the virtual world |
| In the virtual world | the player stands on an invisible platform in the void, falls for the gravity check and, when needed, answers a CAPTCHA on a map in hand (or in a title); the client's brand and settings, as sent to the proxy, are checked |
| Passed | the player goes to the next gate (PumboAuth) or to a server in the same connection, without a disconnect screen |
| Commands of a held player | only `/captcha <code>`; every other command, `/server` too, is refused (the proxy runs no proxy command in the virtual world) |

Messages follow the language of the player's game (`en`, `pl` and files in `lang/`), with `per-player-language = true`.

## Installation

1. Copy `pumbo-filter.wasm` into the proxy's `plugins/` folder (under any name, e.g. with the version): one file, the manifest is built in. The first start creates `plugins/pumbo-filter/config.yml` (the defaults with comments), `plugins/pumbo-filter/lang/en.yml` and `lang/pl.yml` (every message) and `plugins/data/pumbo-filter/`; a file you changed is never overwritten (one you did not change gets the new texts of an update), and options or messages a newer version adds or changes are named in the log. A `pumbo-filter.yml` left from an older version overrides the built-in manifest (the log says so): delete it.
2. Optional: edit `plugins/pumbo-filter/config.yml` (see [Configuration](#configuration)) and the messages in `plugins/pumbo-filter/lang/<code>.yml`; another language is one more file there.
3. Recommended in `pumboprox.yml`, so that nobody gets in while the filter is not running:

   ```yaml
   plugins:
     required-gates: [filter]
   ```

4. `pumbo.filter.bypass` belongs in `permissions.yml` of the proxy: gates see only the file layer of permissions.

The database is `plugins/data/pumbo-filter/filter.redb` (verified players). Without it the filter still checks everyone, only without the verified cache.

## Clients through ViaProxy

Clients older than 1.21 come through ViaProxy, which translates their movement; `viaproxy.check-mode` (default `ONLY_CAPTCHA`) sets their checks: their movement is translated, so the gravity check may not fit them. Clients 1.21–26.2 connect to the proxy in their own version and get the normal checks; the version translator only works between the proxy and the server.

## With PumboBans

With `auto-ban.enabled: true` an address whose connections fail the checks `failures` times within `window-minutes` gets a temporary IP ban through PumboBans (`pumbobans:punish@1.0`, author PumboFilter, reason from the config, lift it with `/unbanip`). PumboBans has to allow it: `ipc.allow-punish: [pumbo-filter]` in its config. Without PumboBans the address is refused from memory for `ban-minutes`.

## For other plugins

The topic `pumbo:attack-state@1.0` (`active`, `level`: 0 off, 1 attack, 2 attack with reconnect) is published when attack mode starts or ends.

## Configuration

`plugins/pumbo-filter/config.yml` (reload with `/pumbo filter reload`; a file that is not valid YAML is reported with the line and column and the current settings stay). A new player is held in the proxy's virtual world before any server sees them: they fall in the void for the gravity check and answer a CAPTCHA on a map in their hand. No server, no world file and no inventory is touched. PumboAuth, when installed, takes over for the login in the same connection.

| Option | Default | Meaning |
| --- | --- | --- |
| `language` | `en` | language of the messages: `en`, `pl` or any code with a `lang/<code>.yml` file |
| `per-player-language` | `true` | players whose game is set to another language with messages get that language |
| `filter.check-mode` | `GRAVITY_THEN_CAPTCHA` | checks of a new player: `ALWAYS` (gravity and CAPTCHA), `ONLY_GRAVITY`, `ONLY_CAPTCHA`, `GRAVITY_THEN_CAPTCHA` (the CAPTCHA only after a failed or impossible gravity check) or `NEVER` |
| `filter.attack-check-mode` | `ALWAYS` | the same choice during an attack (many connections at once) |
| `filter.skip-premium` | `true` | players the proxy logged in with Mojang (premium) skip the checks, except during an attack |
| `filter.verified-cache` / `verified-ttl-hours` | `true` / `12` | remember players who passed (nickname + address) and let them in directly for this long |
| `filter.whitelist` | none | `"Nick@1.2.3.4"` pairs that are never checked |
| `captcha.mode` | `map` | `map`: the code is drawn on a map in the player's hand (only that player gets the map); `title`: the code is shown as a title |
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
| `viaproxy.check-mode` | `ONLY_CAPTCHA` | checks of clients that come through ViaProxy, see [Clients through ViaProxy](#clients-through-viaproxy) |
| `auto-ban.*` | off | automatic IP bans, see [With PumboBans](#with-pumbobans); `auto-ban.whitelist` lists networks never banned, e.g. `["10.0.0.0/8", "2001:db8::/32"]` |

Every section has `enabled` (default `true`) to turn its check off. A wrong value is reported in the proxy log with the option name and replaced by the default.

## Commands and permissions

| Command | Permission |
| --- | --- |
| `/pumbofilter stats` | `pumbo.filter.stats` |
| `/pumbofilter verify <nick> <ip>` | `pumbo.filter.verify` |
| `/pumbofilter unverify <nick>` | `pumbo.filter.unverify` |
| `/pumbofilter attack <on\|off\|auto>` | `pumbo.filter.attack` |
| `/pumbofilter reload`, `version`, `debug` (run by the proxy) | `pumbo.filter.reload`, `.version`, `.debug` |
| (no command) skip the checks | `pumbo.filter.bypass` |

Everything also works as `/pf <command>` (short for `/pumbofilter`), `/pumbo filter <command>` and from the proxy console. `/pf` alone shows the help.

## Building

`./build.sh` builds `dist/`.
