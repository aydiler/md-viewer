# Fix: Snap file picker never opened (#206)

**Status:** ✅ Complete
**Branch:** `fix/snap-file-picker`
**Date:** 2026-11-11
**Lines Changed:** `src/main.rs`, `Cargo.toml`, `Cargo.lock` (+1 line)

## Summary

In the snap, File › Open and File › Open Folder failed with
`Tkinter file picker failed: … No module named 'tkinter'` and no dialog opened.
Reported, with the root cause already identified, by HartmutLeister in #206.

Two links in a chain both broke under strict confinement:

1. `portal_file_chooser_available()` shelled out to `gdbus introspect`. Neither
   the snap nor the core22 base ships `gdbus`, so the probe always returned
   `false`, even though the host's portal was reachable.
2. The fallback then ran `python3 -c "import tkinter…"` from core22, whose Python
   has no `tkinter`.

## Fix

The probe now runs in-process. It reads the `version` property of
`org.freedesktop.portal.FileChooser` over the session bus with a 5 s method
timeout (`portal_file_chooser_version`). `zbus` was already in the dependency
graph through `accesskit_unix`, so the only additions are a direct Linux-only
dependency and the `blocking-api` feature. The Tkinter fallback stays for hosts
that really have no portal.

`Properties.Get` replaces `Introspect` for two reasons. It checks the exact
interface rfd needs, not a substring of an XML dump. And snapd's `desktop`
interface allows it explicitly: `interface=org.freedesktop.DBus.Properties`,
`path=/org/freedesktop/portal/{desktop,documents}{,/**}`, `peer=(label=unconfined)`
(`interfaces/builtin/desktop.go`). The snap already plugs `desktop`.

## Verification

Each case was observed, not inferred, using a throwaway ignored test that printed
the probe result:

| environment | result |
|---|---|
| real KDE session | `Ok(4)`: the portal path is used |
| `DBUS_SESSION_BUS_ADDRESS` pointing nowhere | `Err(Connection …)`: falls back |
| private `dbus-daemon` with no service dirs | `Err(ServiceUnknown)`: falls back |

Not verified: an actual run inside the snap. That needs a CI-built snap on a
snapd host, so the reporter is asked to confirm on the next revision.

## Key Discoveries

- A probe that depends on an external binary inherits the sandbox's binary
  inventory. In a strict snap that inventory is the snap plus core22, not the
  host. Prefer in-process IPC when the crate is already linked.
- `dbus-run-session --config-file` with `unix:tmpdir=<long path>` fails with
  `Socket name too long`, and `grep`ping the output for a result then shows
  nothing at all. Use `unix:abstract=<name>` for test buses.

## Future Improvements

- Stage `python3-tk` in the snap, or drop the Tkinter fallback there. It can
  only be reached in a snap if the portal is absent, and then it still fails.
