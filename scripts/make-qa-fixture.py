#!/usr/bin/env python3
"""Create a NEW disposable SQLite fixture; never overwrite an existing database.
Usage: python scripts/make-qa-fixture.py PATH [--legacy]
--legacy creates schema v5 for one-time upgrade and backup testing.
"""
import argparse
import datetime as dt
import pathlib
import sqlite3

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('path', type=pathlib.Path)
parser.add_argument('--legacy', action='store_true')
args = parser.parse_args()
args.path.parent.mkdir(parents=True, exist_ok=True)
# Exclusive creation refuses live or pre-existing data, including an empty file.
with args.path.open('xb'):
    pass
now = dt.datetime.now().astimezone()
with sqlite3.connect(args.path) as db:
    db.executescript('''
    CREATE TABLE notes (
      id INTEGER PRIMARY KEY, body TEXT NOT NULL DEFAULT '', x INTEGER, y INTEGER,
      w REAL NOT NULL, h REAL NOT NULL, color INTEGER NOT NULL DEFAULT 0,
      mode INTEGER NOT NULL DEFAULT 0, zoom INTEGER NOT NULL DEFAULT 100,
      reminder_due INTEGER, reminder_anchor INTEGER, reminder_repeat TEXT,
      deleted_at INTEGER, created_at INTEGER NOT NULL DEFAULT (unixepoch()),
      updated_at INTEGER NOT NULL DEFAULT (unixepoch()), icon TEXT NOT NULL DEFAULT '',
      list_id INTEGER NOT NULL DEFAULT 1, desktop TEXT,
      hidden INTEGER NOT NULL DEFAULT 0, view INTEGER NOT NULL DEFAULT 0);
    CREATE TABLE lists (id INTEGER PRIMARY KEY, name TEXT NOT NULL, position INTEGER NOT NULL DEFAULT 0);
    CREATE TABLE profiles (id INTEGER PRIMARY KEY, name TEXT NOT NULL, position INTEGER NOT NULL DEFAULT 0);
    CREATE TABLE profile_lists (profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
      list_id INTEGER NOT NULL REFERENCES lists(id) ON DELETE CASCADE, PRIMARY KEY(profile_id, list_id));
    CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    INSERT INTO lists VALUES (1, 'Notes', 0), (2, 'Other profile', 1);
    INSERT INTO profiles VALUES (1, 'All', 0), (2, 'QA profile', 1);
    INSERT INTO profile_lists VALUES (1, 1), (1, 2), (2, 1);
    INSERT INTO settings VALUES ('profile', '2');
    ''')
    if not args.legacy:
        db.execute('CREATE TABLE note_dates (note_id INTEGER PRIMARY KEY REFERENCES notes(id) ON DELETE CASCADE, local_day TEXT NOT NULL)')
    fixtures = [
        ('Today task\n- [x] Parent\n  - [ ] Existing child', 0, 1, 0, 0),
        ('Yesterday visible\n- [ ] Test Enter and Ctrl+Enter', 1, 1, 0, 1),
        ('Yesterday hidden\nHidden flag must survive Never Mind', 1, 1, 1, 0),
        ('Older note\nRecover me through grouped Undo', 3, 1, 0, 0),
        ('Other profile\nBulk action must leave me alone', 1, 2, 0, 0),
    ]
    for i, (body, age, note_list, hidden, mode) in enumerate(fixtures, 1):
        ts = int((now - dt.timedelta(days=age)).timestamp())
        db.execute('INSERT INTO notes (id, body, x, y, w, h, color, mode, list_id, hidden, created_at, updated_at) VALUES (?, ?, ?, ?, 280, 250, ?, ?, ?, ?, ?, ?)',
                   (i, body, 40 + i * 30, 40 + i * 30, i, mode, note_list, hidden, ts, ts))
        if not args.legacy:
            db.execute('INSERT INTO note_dates VALUES (?, ?)', (i, (now.date() - dt.timedelta(days=age)).isoformat()))
    db.execute('PRAGMA user_version=' + ('5' if args.legacy else '6'))
print(args.path.resolve())
