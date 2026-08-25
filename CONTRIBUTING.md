# Contributing to Glimpse

## Getting set up

[`docs/development.md`](docs/development.md#setting-up) is the single place that
lists what a machine needs.

```sh
make check-reqs   # reports what is missing before a build finds out
make check        # the gates, fastest-failing first
```

One prerequisite fails in a way that does not look like a failure: **Glimpse
needs an X11 session** — it checks the GDK backend at startup and exits with an
explanation, because the framing-window model does not survive a compositor that
mediates selection. Note that having `$DISPLAY` set is not enough: XWayland
answers it under Wayland too, which is why the check is on the backend GTK chose.

## What a pull request has to clear

- `make check` — doc-drift check, formatting, clippy with warnings-as-errors, and
  the tests. If you added an ADR or moved a module, `make docs-sync` first.
- Anything touching the geometry chain or the widget hierarchy also needs
  `make selftest`, **and you looking at** `/tmp/glimpse-selftest.png`. Say in the
  PR that you did. The suite genuinely cannot catch what that image catches.

## What this project is strict about

**Verification method, more than code style.** Two of the three bugs found so far
were confidently verified as absent by a method that could not have detected
them. [ADR 0000](docs/adr/0000-x11-framing-window-spike.md) is the record; the
short version is in [`AGENTS.md`](AGENTS.md).

If you add a check, say what would happen if the thing it tests were broken. If
the answer is "it would pass anyway", it is not a check.

## Decision records

Anything that closes off an alternative goes in [`docs/adr/`](docs/adr/), which
is append-only — supersede a record, do not edit it. Include the alternatives
rejected and what the decision costs; a record with no costs section is usually
missing the interesting half.
