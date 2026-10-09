# PumboFilter

Anti-bot checks for Pumpkin servers. A new player falls for a few seconds high above their spawn while PumboFilter compares the movement with vanilla physics, and answers a CAPTCHA when that is not enough. Then they are put back where they were. A player who passed is let in directly next time.

## Features

- **Gravity check.** 6.4 seconds of free fall that must match the vanilla client.
- **CAPTCHA** on a map in the player's hand or in a title, answered in chat or with `/captcha <code>`.
- **Client check.** The brand and settings of the game. Block brands by name.
- **Attack mode.** From 25 connections in 5 seconds everyone gets the full checks. From 80, unknown players also have to join twice.
- **Verified players** are let in directly for 12 hours.
- **Whitelist and bypass permission** for players who should never be checked.
- **Languages.** English and Polish built in, any other from `lang/<code>.yml`.

## Commands

| Command | What it does |
| --- | --- |
| `/captcha <code>` | Answer the CAPTCHA (chat works too) |
| `/pf stats` | Players held now, checks, attack mode |
| `/pf verify <nick> <ip>` | Let a nickname from an address skip the checks |
| `/pf unverify <nick>` | Check a nickname again next time |
| `/pf attack <on\|off\|auto>` | Force attack mode on or off |
| `/pf reload` / `version` | Reload the config, show the version |

`/pf help` shows every command you may use. `/pf` is short for `/pumbofilter`. Permissions are named `pumbofilter:<name>` (`pumbofilter:bypass` skips the checks) and default to operators.

## Installation

Drop the file into `plugins/` and start the server. The config is created in `plugins/data/pumbofilter/`. Works with Pumpkin 0.2.0 (Minecraft 26.3).

## Running a network?

PumboFilter also runs on [PumboProx](https://github.com/PumboMC/PumboProx): bots are stopped before they reach any of your servers, and addresses that keep failing are banned automatically.

---

PumboFilter is in beta. Try it on a test server before you put players on it.
Source, documentation and issues: https://github.com/PumboMC/PumboFilter (GPL-3.0)

[![PumboProx: everything you need to run a network on Pumpkin](assets/pumboprox.webp)](https://github.com/PumboMC/PumboProx)
