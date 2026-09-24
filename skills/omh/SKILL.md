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

Build before acquiring a device. Use `omh devices --json`, then:

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
