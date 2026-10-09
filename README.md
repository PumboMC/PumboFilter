<p align="center">
  <img src="assets/logo.png" alt="Pumbo logo" width="160">
</p>

<h1 align="center">PumboFilter</h1>

<p align="center">Anti-bot checks for Pumpkin servers and PumboProx networks.</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0-blue" alt="License: GPL-3.0"></a>
  <img src="https://img.shields.io/badge/built%20with-Rust-orange?logo=rust" alt="Built with Rust">
  <img src="https://img.shields.io/badge/plugin-WebAssembly-654FF0?logo=webassembly&logoColor=white" alt="WebAssembly plugin">
  <a href="https://github.com/Pumpkin-MC/Pumpkin"><img src="https://img.shields.io/badge/Pumpkin-0.2.0%20%2826.3%29-F28C28" alt="Pumpkin 0.2.0 (26.3)"></a>
  <a href="https://github.com/Pumpkin-MC/Pumpkin"><img src="https://img.shields.io/badge/Pumpkin-0.1.0--dev%20%2826.2%29-F28C28" alt="Pumpkin 0.1.0-dev (26.2)"></a>
  <a href="https://github.com/PumboMC/PumboProx"><img src="https://img.shields.io/badge/PumboProx-supported-62B47A" alt="PumboProx: supported"></a>
  <img src="https://img.shields.io/badge/status-beta-yellow" alt="Status: beta">
</p>

<p align="center">
  <a href="#features">Features</a> ·
  <a href="#two-builds">Two builds</a> ·
  <a href="#installation">Installation</a> ·
  <a href="#configuration">Configuration</a> ·
  <a href="#commands-and-permissions">Commands</a> ·
  <a href="#building">Building</a>
</p>

---

<p align="center">
  <a href="https://github.com/PumboMC/PumboProx"><img src="assets/pumboprox.webp" alt="PumboProx: everything you need to run a network on Pumpkin" width="100%"></a>
</p>

<p align="center"><b>Running more than one server?</b> <a href="https://github.com/PumboMC/PumboProx">PumboProx</a> is the proxy for Pumpkin networks, with plugins in WebAssembly.<br>PumboFilter runs on it too: bots are stopped before they reach any of your servers, and a player checked once is not checked again on the next server.</p>

> [!NOTE]
> PumboFilter is in **beta** (0.1.1-beta). Try it on a test server before you put players on it.

## What it does

PumboFilter checks that a new player is a real Minecraft client before they play. The player falls for a few seconds while the plugin compares their movement with vanilla physics, and answers a CAPTCHA when that is not enough. When a flood of connections starts, the checks get stricter. A player who passed once is let in directly next time.

## How a player gets in

On PumboProx, PumboFilter and PumboAuth work as two gates in a row. Each also works alone, and on a plain Pumpkin server.

```
Player joins
   │
   ▼
Login on the proxy ──────────────────────── before encryption
   ├── PumboFilter   counts connections, attack mode, auto-ban of addresses that keep failing
   └── PumboAuth     refuses bad nicknames, locked accounts, new players while registrations are closed
   │
   ▼
Gates in a virtual world (PumboVir) ─────── no server sees the player yet
   │
   ├── 1. PumboFilter                       gate "filter"
   │      ├── let through: premium (outside an attack), verified in the last 12 h,
   │      │                whitelist, pumbo.filter.bypass
   │      ├── gravity check      falls for 6.4 s, compared with vanilla physics
   │      ├── client check       brand and settings of the game
   │      ├── CAPTCHA if needed  a code on a map in hand → /captcha <code>
   │      └── passed → next gate in the same connection, no reconnect screen
   │
   └── 2. PumboAuth                         gate "auth"
          ├── premium            logged in by Mojang, no password
          ├── session            joined from here in the last 60 min
          ├── new player         /register <password> <password>
          ├── known player       /login <password>   (+ /2fa <code> with 2FA on)
          ├── countdown bar, lockout after wrong passwords, argon2id
          └── logged in → a server
   │
   ▼
lobby (Pumpkin) ─────────────────────────── /changepassword, /logout, /2fa, /premium
                                            are taken by the proxy and never reach a server
```

## Features

| | Feature | |
| --- | --- | --- |
| 🤖 | **Anti-bot checks** | Every new connection is checked before it plays: movement, a CAPTCHA, the client's brand and settings. |
| 🪂 | **Gravity check** | The player falls for 128 ticks (6.4 s). The heights the client reports must match the free fall of the vanilla client. |
| 🔤 | **CAPTCHA** | A distorted code on a map in the player's hand, or in a title. The player types it in chat or with `/captcha <code>`. |
| 🌌 | **Checks in the void** | On PumboProx the player falls in the proxy's virtual world, so no server, world or inventory is touched. On Pumpkin they fall high above their own place and are put back afterwards. |
| 🧾 | **Client check** | Real clients send their brand (`vanilla`, `fabric`, ...) and settings. You can block brands by name. |
| 🚨 | **Attack mode** | From 25 connections in 5 seconds (by default) everyone gets the full checks. From 80, unknown players also have to join twice. |
| 🔨 | **Auto-ban** | On PumboProx, an address that keeps failing the checks gets a temporary IP ban through PumboBans, or is refused from memory without it. |
| 📋 | **Whitelist** | `Nick@1.2.3.4` pairs that are never checked. |
| 🎫 | **Bypass** | Players with the bypass permission skip the checks. So do premium players, except during an attack. |
| ✅ | **Verified players** | A nickname that passed from an address is let in directly for 12 hours (you can change it). |
| ➡️ | **No second join** | After the check the player goes on to the login or to the server in the same connection, without a disconnect screen. |
| 🌍 | **Multilingual** | English and Polish built in, any other language from `lang/<code>.yml`. On PumboProx each player gets the language of their game. |

## Two builds

The checks live in one shared core. You pick the build that fits your setup.

| Build | File | Where it goes | What is different |
| --- | --- | --- | --- |
| 🌐 **PumboProx** (whole network) | `PumboFilter-Proxy-<version>.wasm` | `plugins/` of the proxy | Checks run in the proxy's virtual world, before any server sees the player. Adds auto-ban, per-player language and a separate check mode for ViaProxy clients. |
| 🎃 **Pumpkin** (one server) | `PumboFilter-Pumpkin-26.3-<version>.wasm` or `PumboFilter-Pumpkin-26.2-<version>.wasm` | `plugins/` of the server | Checks run on the server. The held player is hidden, cannot be hurt and cannot use anything. Their world, inventory and data file never change. |

## Installation

> [!TIP]
> Download the files from [Releases](https://github.com/PumboMC/PumboFilter/releases/latest), or [build from source](#building).

**On PumboProx**

1. Put `PumboFilter-Proxy-<version>.wasm` into the proxy's `plugins/` folder.
2. Start the proxy. The first start creates `plugins/pumbo-filter/config.yml` with comments.
3. Recommended in `pumboprox.yml`, so nobody gets in while the filter is not running:

   ```yaml
   plugins:
     required-gates: [filter]
   ```

**On Pumpkin**

1. Put the file that matches your Pumpkin version into the server's `plugins/` folder.
2. Start the server. The first start creates `plugins/data/pumbofilter/config.yml` and the language files.

The plugin only needs to read and write its own data folder. It has no network access.

## Configuration

The config file is written on the first start, with a short comment on every option. Reload it with `/pumbofilter reload`. A wrong value is reported in the log with the option name and replaced by the default. The most used options:

| Option | Default | What it does |
| --- | --- | --- |
| `language` | `en` | Language of the messages: `en`, `pl` or any code with a `lang/<code>.yml` file. |
| `filter.check-mode` | `GRAVITY_THEN_CAPTCHA` | Checks for a new player: `ALWAYS`, `ONLY_GRAVITY`, `ONLY_CAPTCHA`, `GRAVITY_THEN_CAPTCHA` or `NEVER`. |
| `filter.attack-check-mode` | `ALWAYS` | The same choice during an attack. |
| `filter.skip-premium` | `true` | Premium players skip the checks, except during an attack. |
| `filter.verified-ttl-hours` | `12` | How long a player who passed is let in directly. |
| `filter.whitelist` | none | `"Nick@1.2.3.4"` pairs that are never checked. |
| `captcha.mode` | `map` | `map` draws the code on a map in the player's hand, `title` shows it as a title. |
| `captcha.attempts` / `timeout-seconds` | `2` / `30` | Wrong answers and time before the player is disconnected. |
| `client-check.blocked-brands` | none | Client brands that are refused. |
| `attack.threshold` / `window-seconds` | `25` / `5` | This many connections within the window start attack mode. |
| `attack.reconnect-threshold` | `80` | From this many, unknown players also have to join twice. |
| `auto-ban.enabled` | `false` | PumboProx only: temporary IP bans for addresses that keep failing. |
| `viaproxy.check-mode` | `ONLY_CAPTCHA` | PumboProx only: checks for clients that come through ViaProxy. |

Every section also has `enabled` to turn its check off.

## Commands and permissions

| Command | What it does | Permission |
| --- | --- | --- |
| `/captcha <code>` | Answer the CAPTCHA (chat works too) | everyone |
| `/pumbofilter help [page]` | The commands you may use | none |
| `/pumbofilter stats` | Players held now, checks, attack mode | `pumbo.filter.stats` |
| `/pumbofilter verify <nick> <ip>` | Add a nickname and address to the verified list | `pumbo.filter.verify` |
| `/pumbofilter unverify <nick>` | Check a nickname again next time | `pumbo.filter.unverify` |
| `/pumbofilter attack <on\|off\|auto>` | Force attack mode on or off | `pumbo.filter.attack` |
| `/pumbofilter reload` | Reload the config and messages | `pumbo.filter.reload` |
| `/pumbofilter version` | Version and platform | `pumbo.filter.version` |
| (no command) | Skip the checks | `pumbo.filter.bypass` |

`/pf` is short for `/pumbofilter`, on PumboProx and on Pumpkin. On PumboProx every command also works as `/pumbo filter <command>` (`/pumbo filter` alone shows the help) and from the proxy console. On Pumpkin the permissions are named `pumbofilter:<name>` (for example `pumbofilter:bypass`) and the admin commands default to operators.

## Works with other Pumbo plugins

PumboFilter works on its own. When it finds other Pumbo plugins, it works with them:

| Plugin | Together |
| --- | --- |
| 🔒 [PumboAuth](https://github.com/PumboMC/PumboAuth) | The player passes the checks first, then logs in, on the same connection. |
| 🚫 [PumboBans](https://github.com/PumboMC/PumboBans) | Banned players are refused before they reach the check. On PumboProx, auto-ban places its IP bans through PumboBans (allow it with `ipc.allow-punish: [pumbo-filter]` in PumboBans). |
| 🌐 [PumboProx](https://github.com/PumboMC/PumboProx) | Runs the checks for the whole network in a virtual world. Other proxy plugins can listen for attack mode (`pumbo:attack-state@1.0`). |

## Building

You need Rust stable with the WebAssembly target:

```sh
rustup target add wasm32-wasip2
```

Cargo fetches the shared Pumbo libraries (`pumbo-common`, `pumbo-sdk`) from the [PumboProx](https://github.com/PumboMC/PumboProx) repository on the first build.

Pumpkin build, both versions (`dist/PumboFilter-26.3.wasm` and `dist/PumboFilter-26.2.wasm`):

```sh
plugins/pumbo-filter-pumpkin/build.sh
```

PumboProx build (`plugins/pumbo-filter-prox/dist/pumbo-filter.wasm`):

```sh
plugins/pumbo-filter-prox/build.sh
```

Tests of the core and both builds:

```sh
cargo test
```

## License

PumboFilter (the core and both builds) is licensed under the [GNU General Public License v3.0](LICENSE). The shared library for Pumbo plugins (`pumbo-common`) is dual-licensed under MIT and Apache-2.0.

PumboFilter is not affiliated with Mojang, Microsoft or the Pumpkin project.

---

<p align="center">
  Part of <a href="https://github.com/PumboMC/PumboProx"><b>PumboProx</b></a>. Everything you need to run a network on Pumpkin.
</p>
