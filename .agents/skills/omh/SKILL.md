---
name: omh
description: Coordinate HarmonyOS application tests on shared real devices through the omh CLI. Use for device installation, launching, test execution, logs and evidence collection in an omh-enabled project.
---

# Shared-device testing with omh

In an enabled project, run every device operation through `omh`. Never invoke raw
`hdc`, SDK absolute paths, another device connector, or a script that bypasses omh.
This is a cooperation rule and guardrail, not an OS security boundary.

## Before testing

Use `omh doctor --project .` to inspect setup. Installation is not proof that
Codex trusts or runs the hook. On first setup, follow its runtime verification
instructions. Do not report protection as active without an actual denied tool
call in this Codex session. Missing/untrusted hooks require setup, not a raw-hdc
fallback.

Build before acquiring a deployment device. Automatic signing is the exception:
it needs a short device lease to read UDID before the first signed build (see below).
Use `omh devices --json`, then:

```sh
omh acquire --package com.example.app --wait 30m --json
# Optionally add --device ID; repeat --package for multiple tested apps.
omh exec --lease TOKEN -- install /absolute/path/app.hap
omh exec --lease TOKEN -- shell aa start -b com.example.app -a EntryAbility
omh logs --lease TOKEN --output /absolute/path/new-test.log
omh exec --lease TOKEN -- shell snapshot_display -f /data/local/tmp/test.png
omh exec --lease TOKEN -- file recv /data/local/tmp/test.png /absolute/path/test.png
omh release --lease TOKEN
```

Keep the returned lease token private to the test owner. Child agents must acquire
their own leases unless explicitly coordinating one test. `OMH_LEASE` can replace
`--lease`, but do not rely on shell environment surviving separate tool calls.

## Occupancy rules

- One device belongs to one test session; never override its device/server target.
- Ordinary sessions expire after 10 minutes without device actions. Reading
  status or saved logs does not renew them. A log stream never protects a lease.
- Ordinary commands have a 10-minute limit. Use a managed job for longer tests.
- Capture all evidence before releasing; analyze saved artifacts offline.
- Release stops declared apps and owned host processes/forwards, but preserves
  installed apps and data. Restore any test-modified settings in the test itself.
- A released/expired token cannot be reused. Reacquire instead of retrying it.

## DevEco automatic signing

Use `omh deveco auth status` before acquiring a signing lease. A zero exit code
does not mean logged in: check for `Current user:` versus `Not logged in`.
If login is needed, run `omh deveco auth login` and keep that process alive.
In Codex, it emits an `omh.deveco.login` JSON event: immediately open its exact
`url` using `open_in_codex` with a browser target in the current chat's side panel
(or the available in-app browser tool). Do not merely print the link or use the
OS browser in Codex. Outside Codex, the default browser opens automatically.
`--browser codex|default` overrides detection for hosts without Codex environment
markers. Wait for the official login-success callback; the page opening is not
authentication success. Do not capture credentials or commit the login URL.

From the HarmonyOS app root, acquire a lease declaring its real bundleName, then:

```sh
omh deveco signature generate --lease TOKEN --product default --timeout 10m
```

This is a managed signing job, not a command to nest inside another job. It
automatically cleans up and releases the lease on completion/failure/timeout;
reacquire for installation after building. Its HDC queries can only reach the
leased device. Never use raw `devecocli signature generate`, including when
signing material already exists: the CLI still queries connected devices.
Reuse login/certificates, omit `--force` unless replacement is explicitly needed,
and preserve cloud account behavior (the profile may include other devices
already registered with Huawei). For missing-signature detection and artifact
validation, read the app-dev skill's build-deploy reference.

This adapter requires Node.js and official DevEco CLI 1.2.1. Other versions fail
before execution until compatibility is verified; do not silently upgrade,
patch the SDK/CLI, or fall back to unscheduled HDC. `--cli` (dist/cli.js) and
`--node` can be supplied after `omh deveco` when they are not on PATH.

## Long tests

Write a script that performs the actual test, waits for its real completion,
checks business success and saves evidence **before exiting**. The script must
use `omh exec` for device operations. The daemon supplies `OMH_LEASE`, `OMH_HOME`,
`OMH_DEVICE` and `OMH_BIN`, and adds omh to PATH.

```sh
omh job start --lease TOKEN --timeout 90m -- sh /absolute/path/test.sh
omh job status RUN_ID --json
omh job cancel RUN_ID
```

Use the platform's available interpreter (for example PowerShell on Windows);
omh executes argument arrays, not shell strings. A managed job survives CLI exit
and requires no agent heartbeat. It immediately releases the phone on completion,
failure or timeout. Logs are attachments, never the job's completion condition.
Do not use indefinite `hilog`, a bare sleep, or meaningless polling as a test.
Do not daemonize descendants or create detached device-side tasks. For a long
device-side workload, use bounded omh commands to start and observe it until a
real completion marker appears; do not hold one exec call beyond 10 minutes.

## Failures and maintenance

Inspect `omh status` and `omh job status` before retrying. Distinguish hdc exit
status from application success, timeout, infrastructure failure and cleanup.
CLI infrastructure errors use exit 125; timeout 124; cancellation 130. A child
may itself use those codes: consult the structured result's reason.

Disconnected or uncleared devices are not assignable. Reconnect, inspect the
reported evidence, then use `omh recover --device ID`. Never delete state files
or kill unrelated processes to force a handoff.

`omh maintenance restart` queues an hdc service restart. It pauses new grants
and new long tests, waits for current sessions and cleanup, then restarts once.
Finish and release existing sessions. Inspect status; a queued request is not a
completed restart. Failed maintenance needs diagnosis and an explicit retry.

Manual terminals, IDEs, unconfigured projects and intentionally concealed calls
are outside the guardrail. Do not claim universal prevention of hdc access.
