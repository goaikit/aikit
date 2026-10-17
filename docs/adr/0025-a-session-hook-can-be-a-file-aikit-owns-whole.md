# A session hook can be a file aikit owns whole

Status: accepted. Date: 2026-10-17. Plan: [session hooks for Codex, Cursor and Pi](../plans/session-hooks-codex-cursor-pi.md).

`session_hooks` so far merged one entry into an agent's JSON settings file and recognised it by its id. Pi has no command hooks; it runs a command at session start only from a TypeScript extension. aikit therefore writes, for Pi, a whole extension file named after the hook id, rendered from a fixed template, marked with a generated-file line that names the id. It never edits a file it didn't render: a file at that path without the marker is refused, and unregistering deletes only a file whose marker names the same id.

The alternatives were a Pi package installed with npm, which needs network access and a package manager at registration time, and leaving Pi to callers, which would put agent knowledge outside aikit. A file aikit owns whole needs neither, and needs no merging, so it carries the least risk of breaking a user's settings. The cost is that aikit ships code in another language, so the template is tested as a golden file and parsed in CI.

Hooks for Pi run a program with arguments rather than a shell command, so `SessionHook` can now be built from an argument vector, and the shell command other agents need is derived from it.
