# Jorge's Stickitt changes — draft code review

Target fork: https://github.com/Danscap/stickitt

Pinned base: `00a26002fc7505ecf73f2d75ddbba375b1d5ce0a`.
The fork's `main` was checked against this base before editing. Changes are on the
fork-only branch `jorge/review-note-enhancements-20261113` in draft PR
https://github.com/Danscap/stickitt/pull/1. No installer dispatch, merge, tag, release,
or access to a live note database has occurred.

## Implemented

- **New notes always on top**: a checkbox in the dock's right-click menu and
  `[notes] always_on_top = false` in `stickit.ini`. Applies only to newly created
  notes. Existing modes and per-note mode controls stay independent. Save failures
  are surfaced and the in-memory setting is reverted.
- **Checklist child tasks**: Ctrl+Enter (Cmd+Enter on macOS, following existing app
  conventions) inserts a new unchecked child two spaces deeper, using the existing
  Markdown parser and a single undo step. Enter keeps its existing same-level
  continuation and empty-item exit. Checkbox clicks still toggle completion.
  Non-checklist shortcuts are unchanged; selections, marker-prefix carets, fenced
  code and active IME composition do not use the new child-task operation.
- **Dates**: All / Today / Yesterday / Calendar controls, month navigation,
  Monday-first calendar, Tab/Shift+Tab focus and Enter activation. In icon view,
  right-click offers Calendar/date navigation and selection.
- **Desktop preview**: date selection previews actual note windows in the current
  profile and desktop. Repeated choices retain the initial rollback snapshot.
  Accept keeps the selected windows; Never Mind/Escape restores visibility and
  minimization. Wheel and Up/Down/Page keys scroll without accepting. Focus,
  caret placement, word/line/drag selection, Left/Right/Home/End, Select All, Copy,
  and table cell navigation stay in preview. Keyboard editing (including paste,
  cut, undo/redo, formatting and checklist commands), nonempty IME composition or
  commit, checkbox toggles, table + buttons and creating a paragraph below a table
  accept before the original edit is dispatched exactly once. Pointer edits retain
  the captured hit-tested target. Read-only clicks place the caret for subsequent
  typing; IME preedit is not inserted as committed text.
  Close requests, note chrome/resize/drag controls, links, property shortcuts and
  compact-note activation are inert until explicit Accept. Escape always cancels.
  Synthetic keys, modifier keys and empty/cancelled IME events do not accept.
  Existing popovers/drag state are cleared on each date choice so they cannot consume
  the first edit. Scope-changing app actions and due reminders cancel previews first.
  Preview never changes persisted hidden flags until acceptance.
- **Selection and Trash**: Select All covers currently displayed note cards under
  the active date/search/profile filters and expanded groups. “Move all except
  today to Trash” ignores dock date/search filters, includes hidden notes and other
  desktops in the current profile, and excludes other profiles' lists and existing
  Trash. The confirmation shows an exact count, current-profile scope, and that
  today is kept. Crossing midnight requires a refreshed confirmation. Bulk Trash
  is atomic and recoverable through existing grouped Undo/Trash, not a purge.
  Existing retention still applies (normally 30 days).

Date filtering is session-only. Effective dates are fixed local calendar dates;
changing timezone later does not reclassify old notes. An accepted date filter
remains active in the current session. Note bodies, window modes, and raw timestamp
columns retain their existing format.

## Upgrade and rollback

SQLite schema goes from 5 to 6 with an additive `note_dates(note_id, local_day)` table.
Every pre-existing note, including Trash, receives the local upgrade day once.
New notes receive their local creation day. Original `created_at` and `updated_at`
values are not rewritten by the migration.

Before opening an existing v1–v5 file for migration, the app uses SQLite
`VACUUM INTO` to create a consistent `*.pre-dates-<timestamp>.db` backup beside it,
including committed WAL data. Failure to create that backup stops the upgrade.
The date migration and schema-version change commit together. Restart does not
reassign dates. Restoring from Trash retains its date.

Do not use an older binary against the upgraded database. To roll back, close the
app and use a separate copy of the pre-upgrade backup with the older binary.
Keep the upgraded database as well: the backup cannot include edits made afterward.
Manual Windows verification below uses fresh fixtures only.

## Apply the patch

Run in PowerShell from the folder containing this bundle:

```powershell
git clone https://github.com/Danscap/stickitt.git stickitt-jorge-test
Set-Location stickitt-jorge-test
git switch --detach 00a26002fc7505ecf73f2d75ddbba375b1d5ce0a
git apply --check ..\stickitt-jorge.patch
git apply ..\stickitt-jorge.patch
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

The patch was also applied to a fresh detached checkout of this exact base and
compared byte-for-byte with all changed source files. No commit is required.

## Build the requested Windows installer

The repository's supported installer route is Windows x64, Rust stable with the
MSVC C++ build tools, PowerShell, and official Inno Setup 6. The existing packaging
script can use a different compatible Inno installation through `ISCC`:

```powershell
# Optional if ISCC is not found automatically:
# $env:ISCC = 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe'
.\scripts\package-windows.ps1 -Version '0.1.0-jorge-test.1'
Get-FileHash .\dist\Stick-itt-0.1.0-jorge-test.1-windows-setup.exe -Algorithm SHA256
```

The desired output is **`dist\Stick-itt-0.1.0-jorge-test.1-windows-setup.exe`**.
It would be an unsigned test installer. The packaging script also stages a portable
executable, but no portable executable is supplied as this request's deliverable.
Do not call the installer verified until its install, launch, upgrade and uninstall
behavior has been checked on a disposable Windows VM/account.

**No installer is in this bundle.** The cloud is Linux. The official Inno Setup
7.1.1 x64 compiler installer was downloaded from the publisher's immutable GitHub
release and its SHA-256 matched the published digest. Debian Wine 10 aborted with
`free(): invalid pointer` before the compiler setup could run, both with workspace
and host runtime libraries. No installer compiler execution/build succeeded.
A supported Windows builder is still needed. The fork's existing GitHub workflow
is available for a later authorized installer step. Source is now in draft PR
https://github.com/Danscap/stickitt/pull/1 for review. No installer workflow has been
dispatched; merge, tags, releases and installer work remain paused.

## Validation evidence

See `validation/` in the delivery bundle for exact logs.

- Untouched pinned baseline: **76 tests passed** (fresh baseline compilation).
- Initial implementation: **83 tests passed**.
- Preview correction: **90 tests passed**, including 7 input-routing regressions.
- Linux `cargo build --locked`: passed.
- Linux `cargo clippy --all-targets --locked -- -D warnings`: passed.
- Windows GNU cross-target `cargo clippy --all-targets --locked --target
  x86_64-pc-windows-gnu -- -D warnings`: passed. This checks production/test code;
  it is not a Windows runtime test or a linked Windows installer build.
- `cargo fmt --all --check` and `git diff --check`: passed.
- Fixture generation: v5/v6 schema, SQLite integrity, and refusal to overwrite
  existing fixtures checked.
- New tests cover setting defaults/round-trip, Unicode child insertion and undo/
  redo, repeated date choices/empty dates/original visibility, profile/date bulk
  scope, atomic bulk failure/recovery, one-time migration/restart/raw metadata,
  and a reopenable pre-upgrade backup.

Toolchain: Rust/Cargo 1.100.0 stable. Standard Debian GUI and MinGW packages were
extracted into the task workspace because system package installation was not
available. The inherited redundant `homepage`/`repository` manifest warning is
non-blocking. No dependencies or lockfile versions were changed.

Visual/runtime validation is **not claimed**: no display was exposed to the shell,
and a workspace Xvfb attempt could not open display sockets. Windows focus, window
stacking, virtual desktops, scaling, and installer behavior still need real runtime
checks. Unit tests do not replace these checks.

## Disposable Windows smoke test

Python 3 is used only to create new fixtures; the helper refuses to overwrite any
existing path. Keep each run in a fresh temp directory:

```powershell
$qa = Join-Path $env:TEMP ('stickitt-qa-' + [guid]::NewGuid())
New-Item -ItemType Directory $qa
python .\scripts\make-qa-fixture.py "$qa\calendar.db"
python .\scripts\make-qa-fixture.py "$qa\legacy.db" --legacy
cargo run --locked -- --db "$qa\calendar.db" --config "$qa\stickit.ini"
```

Explicit `--db` runs now keep logs/default configuration beside that fixture and
skip normal user-data-directory migration. Do not launch without the fixture flags
for QA. Close one run before starting the next.

1. Toggle **New notes always on top**, create a note, restart using the same fixture
   paths, and create another. Verify persistence, off by default, and unchanged
   normal/on-top modes of existing fixture notes.
2. Test Enter, empty Enter, nested/completed tasks, Ctrl+Enter in the middle/end,
   Unicode, undo/redo, checkbox click, ordinary text, selected text, IME and AltGr.
3. Preview yesterday, today, another day, and an empty day. Never Mind/Escape must
   restore the original visible/hidden/minimized set after repeated choices.
4. Focus, scroll, click to place the caret, double/triple-click/select/Copy, use
   navigation keys, click note chrome or request close, then Never Mind. The original
   windows must return. Repeat preview, click inside Unicode text, then type: only
   typing accepts, and the first character replaces/inserts at that exact target once.
   Check checkbox toggles, table + buttons, table cell Tab/Enter vs row extension,
   keyboard formatting and Ctrl+Enter with one Undo. Start IME composition: it accepts
   before display composition; commit appears once. Empty composition does not accept.
   Compact notes require explicit Accept before activation. Check AltGr and remapped
   note shortcuts; non-edit shortcuts must not accept.
5. Accept yesterday. Reopen Calendar and cancel a different date. Verify the
   accepted view returns. Check reminders, profile changes and dock focus.
6. Test Tab/Shift+Tab/Enter, month boundaries, both themes, compact/detailed/icon
   entry points, narrow dock widths, 100%/150%/200% scaling and multiple monitors.
7. In the fixture's **QA profile**, Select All with date/search filters and collapsed
   groups. Cancel Trash, then confirm it and use grouped Undo. Bodies/dates survive.
8. “Move all except today” in QA profile should propose **3 notes** initially,
   irrespective of the dock filter. Today's note and the other-profile note stay.
   Restore all affected notes; verify restart recovery and retention messaging.
9. Launch `legacy.db` with its own config. All five old notes receive the upgrade
   day. Verify a v5 backup exists, raw timestamps/body/mode/hidden values survive,
   and restart creates no new upgrade assignment or extra migration backup.
10. On a disposable Windows VM, verify per-user installer paths, shortcuts, optional
    autostart, upgrade and uninstall. Do not let the installer launch against a real
    database during QA; run the installed executable with the explicit fixture flags.
