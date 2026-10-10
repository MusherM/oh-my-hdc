# oh-my-hdc (`omh`)

中文：[README_zh.md](README_zh.md)

Share a small pool of HarmonyOS phones between coding agents without interleaving
their tests. **Acquire → test exclusively → clean up → release.**

This repository contains a Rust CLI, three project-local Codex skills and a command hook,
and a four-target build/package workflow. It does not bundle hdc or a HarmonyOS SDK.

## Install and connect

For this repository, run `scripts/init-project.sh` after each code update. Set
`OMH_HDC` to the SDK executable (or add it to `PATH`); pass another project
directory as the first argument when needed. The script performs the steps below.
It stops the existing omh daemon only when devices are idle, so the next query
starts the newly installed binary. An occupied device makes the script stop safely.

Build with a current stable Rust toolchain:

```sh
cargo build --locked --release
cargo install --path . --locked
omh --version
omh --hdc /absolute/path/to/hdc devices --json
omh setup codex --project /absolute/path/to/project
```

Windows uses `omh.exe` and the SDK's `hdc.exe`. Use PowerShell-native paths and
quoting. macOS/Linux use the same subcommands. Basic scheduling needs no Python,
Node or administrator privileges; optional DevEco signing needs Node.js and the
official DevEco CLI. Put the installed binary in a stable location:
the project hook stores its absolute path; rerun setup after relocating it.

Setup installs three complete skill directories from resources embedded in the binary:

- `.agents/skills/omh`: shared-device scheduling and lifecycle.
- `.agents/skills/omh-harmonyos-app-dev`: HarmonyOS development, deployment and UI verification through omh.
- `.agents/skills/omh-harmonyos-app-optimization`: measurement protocols and omh-managed power/memory experiments.

No user-level skills or network downloads are required. Standalone skills in the
user directory remain unchanged. Setup adds a marked routing block to `AGENTS.md`
and merges a hook into `.codex/config.toml`; selection uses explicit project rules,
not same-name skill precedence. If only `CLAUDE.md` exists, setup appends there and
hard-links `AGENTS.md` to it. If neither exists, it creates that linked pair.
Existing hard links are preserved; separate instruction files are not merged.

`.codex/omh-skills.json` records installed contents. Repeated setup is idempotent;
new releases update unchanged managed files and restore missing resources. Setup
refuses conflicting user edits, malformed routing markers or symlinked destinations
before writing. Keep the install record for future upgrades. Resolve reported edits
explicitly before retrying; setup never silently overwrites them. Existing Codex
configuration is preserved and backed up before a change. `omh doctor --project .`
reports `skill_bundle_current` for the complete bundled resources and routing block;
this is a file check, not runtime protection or a skill-selection guarantee.

Long measurement jobs must include baselines, workload verification, restoration
and evidence collection before exiting. Omh cleanup does not restore power/screen
settings. The measurement skill does not authorize HIZ or similar changes without
verified cancellation, timeout and disconnect recovery; it provides no universal
recovery executor.

**Restart Codex, trust the project and review/trust its hook with `/hooks`.** In
the actual Codex task, ask it to invoke `hdc --version`: the tool call must be
denied before execution. `omh --version` must still work. Installing a skill or
running the hook manually does not verify Codex's execution path. `omh doctor
--project .` intentionally never infers runtime protection from files alone.
See [Codex hooks](https://learn.chatgpt.com/docs/hooks).

## Test a device

`omh devices --json` includes `info` for each device: `name`, `model`, `chip`,
`brand`, `os_version`, and `api_version`. The daemon reads these properties once
while the device is free. Unavailable properties are `null`.

Finish building before acquiring. Repeat `--package` for multiple tested apps.
Omit `--device` to take any available phone; `--wait` accepts `s`, `m`, or `h`.

```sh
omh acquire --package com.example.app --device DEVICE_ID --wait 30m --json
omh exec --lease TOKEN -- install /absolute/path/app.hap
omh exec --lease TOKEN -- shell aa start -b com.example.app -a EntryAbility
omh logs --lease TOKEN --output /absolute/path/new-test.log
omh exec --lease TOKEN -- file recv /data/local/tmp/result.json /absolute/path/result.json
omh release --lease TOKEN
```

`OMH_LEASE` can replace `--lease`. Do not share tokens between independent tests.
Commands preserve argument boundaries, working directory, binary stdin/stdout,
stderr and the child exit code. This is a pipe interface, not a terminal emulator;
bare interactive `hdc shell` is not supported. Device and server global options
cannot override the allocated target. `exec` allows install, uninstall, shell,
file send/recv, bugreport, jpid, and owned fport/rport creation. Use `logs` for
hilog; global service commands require the maintenance entry point.

## DevEco automatic signing

The adapter supports official `@deveco/deveco-cli` **1.2.1**. Other versions are
rejected before execution until verified. It reuses the installed CLI, SDK and
login state without editing third-party files or updating the CLI. This optional
integration requires Node.js; basic device scheduling does not. Use
`omh deveco --cli /path/to/dist/cli.js --node /path/to/node ...` for installations
outside PATH.

The app-development skill attempts this flow when the selected product has no
signing configuration or its material files are missing; existing materials are
reused. Authentication does not acquire a device:

```sh
omh deveco auth status
omh deveco auth login
```

A zero status exit does not prove login: inspect `Current user:` / `Not logged in`.
In Codex, login emits an `omh.deveco.login` event. The agent immediately opens its
URL in the current chat's browser side panel using `open_in_codex`, keeping the
login process alive for the user's authentication. Outside Codex, the default
system browser opens automatically. Override detection with `--browser codex|default`;
do not select `codex` in a terminal without an in-app browser tool. Opening the
page is not login success: wait for the official callback.

After login, acquire the target device and sign from the app root:

```sh
omh acquire --package com.example.app --device DEVICE_ID --wait 30m --json
omh deveco signature generate --lease TOKEN --product default --timeout 10m
```

Signing briefly needs the device UDID before the first signed build, an exception
to building before acquiring a deployment lease. This entry starts a managed job
and waits for both execution and cleanup. Completion, failure and timeout release
the lease automatically. Do not nest it inside `omh job`. The printed job ID works
with `omh job status/cancel`; closing the client does not cancel the managed job.
Check the materials, build a signed HAP, then acquire a new lease to install it.

Only the leased device is visible to the CLI. UDID/device-type queries use omh;
other targets, maintenance and unknown commands fail. Device-query errors cannot
be swallowed into a successful signing result. Omit `--force` by default; use
`--team-id` when needed. Huawei still manages cloud certificates/profiles, which
may include other devices already registered with the account. Validate signing,
material validity and installation separately.

This is not a general DevEco device proxy: `run/ui/log/emulator/serve` are outside
its scope. The Codex hook denies common direct DevEco device operations; it
remains a cooperative guardrail, not a system sandbox.

## Long tests and expiry

- A phone has one owner at a time. Eligible waiting requests are served FIFO;
  waiting for a busy named phone does not block a different free phone.
- Ordinary sessions expire after **10 minutes** without a device action. Each
  ordinary command also has a 10-minute limit. Status queries, stdin bytes and
  log output do not refresh the lease.
- A log collector belongs to the lease and stops on release. Its output file
  must be new; omh will not overwrite prior evidence.
- Long tests use a managed job with an explicit positive timeout (up to 7 days).
  There is no one-hour cutoff or agent heartbeat requirement.

```sh
omh job start --lease TOKEN --timeout 90m -- sh /absolute/path/test.sh
omh job status RUN_ID --json
omh job cancel RUN_ID
```

On Windows, use an installed executable such as `powershell.exe -File test.ps1`.
Jobs execute argument arrays, not shell strings. The daemon supplies `OMH_LEASE`,
`OMH_HOME`, `OMH_DEVICE`, `OMH_BIN` and an omh-prefixed PATH. Other environment
variables come from the daemon's startup environment. Scripts must invoke omh
for device access, wait for an actual business-completion signal, validate it and
save all evidence before exiting. A log stream or a bare sleep is not a test.
Do not daemonize descendants or leave detached device-side work.

The submitting CLI can exit while the job continues. Job completion, failure,
timeout or explicit cancellation triggers immediate cleanup and release. Analyze
saved results offline; acquire again if more phone access is needed. Inspect both
`result` and `lease_state`: a finished script does not prove successful cleanup.
Exit 125 denotes CLI/infrastructure failure, 124 timeout and 130 cancellation;
children can also use these numbers, so consult the structured `reason`.

## Cleanup and recovery

Cleanup stops owned host process trees and log streams, removes owned forwarding
rules, then force-stops declared apps. It preserves installed apps/data and does
not restore system settings. A confirmed cleanup is required before reassignment.
Forwards that already exist are not adopted. Cleanup checks hdc success markers,
not just its exit code. An unfamiliar SDK response fails conservatively.
The explicit `10104002` response stating that the declared app is not installed
also confirms there is no app to stop; unrelated errors still quarantine the phone.

```sh
omh status --json
omh recover --device DEVICE_ID
omh maintenance restart
omh doctor --project .
```

Disconnect invalidates the old lease. Reconnection does not resume the test or
silently substitute another phone. After reconnection, `recover` retries cleanup;
check status until the device becomes `free`. Failure leaves it `blocked` with
the reason. Recovery does not force-delete the journal or kill arbitrary PIDs.

Maintenance pauses new grants and new long jobs, waits for existing sessions and
cleanup, restarts the shared hdc service once, then rediscovers devices. The
request is asynchronous: `draining` is not success. Duplicate pending requests
coalesce. Failure keeps allocation paused; diagnose and retry explicitly.
Finish/release existing interactive sessions so maintenance can proceed.

## Architecture and limits

One automatically started daemon serves all projects under the same OS user.
Local TCP uses a random loopback port and a per-instance secret in the private
user data directory. A file lock prevents duplicate daemons for that directory.
The hdc path is saved there and reused on restart. Changing SDKs requires draining
and stopping the daemon first. Do not run competing pools using different
`--home`/`OMH_HOME` values against the same phones.

The data directory contains the crash journal, run specs, stdout/stderr and
daemon diagnostics. Its location appears in `doctor`. It is private on Unix;
on Windows use the default user profile directory or an equivalently private ACL.
Keep it private: the journal contains lease credentials. Application evidence
and run history are retained; no automatic history deletion is performed.

Each command has a supervisor; loss of the daemon's pipe cancels its process
tree. A restarted daemon quarantines previously occupied devices until recovery.
If a supervisor itself was killed or no trustworthy completion record exists,
recovery refuses to guess that descendants are gone: inspect the reported run
directory and resolve the processes before further use. Do not delete state to
force reuse. Detached/escaped descendants are outside the supported script
contract. An OS reboot stops processes but does not erase this conservative
quarantine evidence.

The Codex hook prevents common direct hdc calls, absolute SDK paths, command chains
and wrappers. It is **not a sandbox** and does not interpret arbitrary scripts,
Python subprocess calls or injected text in an existing interactive shell. IDEs,
manual terminals, unconfigured projects and deliberate bypass are outside its
protection. Avoid concurrent device use from those sources.

## Build, validate and distribute

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked -- --test-threads=1
cargo build --locked --release
```

Integration tests compile a std-only fake hdc with `rustc`; no SDK or phone is
needed. Each test uses a separate temporary state directory and daemon. Debug
builds alone accept `OMH_TEST_IDLE_MS` to shorten expiry tests; release builds
always use 10 minutes. Time-compressed tests verify the lease policy, not a
physical one-hour endurance run.

Setup tests also verify complete embedded resources, legacy migration, repeat runs,
managed upgrades, conflict preflight, routing links and instruction-file preservation.
Unix tests additionally check hard-link identity and refusal to follow symlinks.
The bundled UI-tree helper can be checked locally with
`uv run --no-project python -B skills/omh-harmonyos-app-dev/scripts/test_ui_tree_inspect.py`.

The CI workflow builds and runs tests natively on Windows x64, Linux x64, macOS
ARM64 and macOS x64, then uploads versioned ZIP/tar.gz packages with SHA-256 files.
Packages contain the binary, all three skills with their resources, license and both READMEs. They are not signed
or notarized. Workflow configuration is not evidence of a completed remote run.
See `dev/implementation/` for timestamped validation scope and unverified items.
