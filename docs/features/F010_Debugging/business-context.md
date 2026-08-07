# Business Context — F010_Debugging

## Why It Matters

Developers need to run their program under a debugger, pause it at chosen points, and inspect or
change its running state, so they can find and fix bugs faster than by reading logs alone. This
also covers attaching to a service that is already running remotely, and a troubleshooting log
viewer for when the debugger tooling itself misbehaves.

## Who Uses It

- **Developer working locally** — starts, steps through, and stops debug sessions on their own
  machine; sets breakpoints, watches expressions, and inspects or edits variables while paused.
- **Developer working on a remote (SSH) project** — attaches the debugger to a long-lived service
  already running on the remote host, without having to restart it.
- **Adapter troubleshooter / power user** — opens the raw debug-protocol log viewer to diagnose why
  a particular debugger integration isn't behaving as expected.

## What They Do

1. Developer selects a launch configuration and starts a debug session — the debugger opens with
   live console, variables, and breakpoint panes.
2. Developer sets breakpoints in their source files; when the program reaches one, execution pauses
   and the developer steps through it line by line, into calls, or back out, watching variables
   update as they go.
3. Developer adds an expression to the watch list so its value is tracked automatically on every
   pause, and can expand a variable to see its nested fields.
4. Developer edits a variable's value on the fly to try a different runtime state without
   restarting the program; if the debugger can't apply the edit, the original value stays shown.
5. Developer clears every breakpoint at once to start a clean debugging pass instead of removing
   them one by one.
6. Developer stops the session when finished, or detaches to leave the program running.
7. On a remote project, developer opens "Attach to Process," picks a running process from the live
   list, and starts debugging it in place.
8. When debugger behavior looks wrong, developer opens the protocol log viewer to see the raw
   traffic exchanged with the debug adapter, even after that session has ended.

## Unresolved Questions

- **Exception-breakpoint carryover**: it isn't confirmed from the business side whether a
  developer's chosen "ignore exception X" settings from a previous session are always expected to
  carry over automatically to the next debugging session with the same adapter, or only in some
  cases.
- **Remote-attach access control**: whether attaching to another user's process on a shared remote
  host should require any extra confirmation is not defined by any business rule found in this
  pass.
