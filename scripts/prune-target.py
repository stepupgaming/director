#!/usr/bin/env python3
"""Delete cargo build output that no recent build uses, from target/<profile>.

Cargo names each compiled unit `<name>-<16 hex>` and keeps every unit it has
ever built. A feature, flag or dependency change gives a crate a new hash and
leaves the old copy behind, so `target` only grows.

Each unit's `.fingerprint/<package>-<hash>` folder records the fingerprints of
the units it was built from, which makes a graph. A root is a unit that no
other unit was built from: a binary, a test binary, a bench, an example, or a
library built on its own. A root stays when it is the newest build of one of
its targets or was built in the last `--days` days. Every unit a kept root was
built from stays too. Everything else goes: its hashed files in `deps` and
`examples`, its folder in `build` and its `.fingerprint` folder.

Commands that resolve features differently build their own copies.
`cargo build -p director-cli`, `cargo test --workspace` and
`cargo test -p director-core` each have a set. A set stays while one of its
roots is the newest build of its target, or for `--days` after it compiled.

A workspace crate's root never stays when its target is gone from the package
(a test file now declared in `tests/main.rs`) or when it carries a feature the
package no longer has. `cargo metadata --no-deps` says what the workspace has
now.

    python scripts/prune-target.py                    report what would be deleted
    python scripts/prune-target.py --delete           delete it
    python scripts/prune-target.py --delete --days 0  keep only the newest build of each target

It refuses to run while cargo or rustc is running.
"""

import argparse
import glob
import json
import os
import re
import shutil
import subprocess
import sys
import time
from collections import defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HASHED = re.compile(r"^(?P<stem>.+)-(?P<hash>[0-9a-f]{16})(?P<ext>\.[^.]+)?$")
# A fingerprint file is named `[test-|doc-|run-]<target kind>-<target name>`.
TARGET = re.compile(r"^(?:test-|doc-|run-)?(?P<kind>lib|bin|integration-test|bench|example|build-script)-(?P<name>.+)$")
BUSY = {"cargo", "rustc", "clippy-driver", "rustdoc"}


def busy_processes():
    if os.name == "nt":
        out = subprocess.run(["tasklist", "/FO", "CSV", "/NH"], capture_output=True, text=True).stdout
        names = [line.split(",")[0].strip('"') for line in out.splitlines() if line]
    else:
        out = subprocess.run(["ps", "-A", "-o", "comm="], capture_output=True, text=True).stdout
        names = out.split()
    return sorted({n for n in names if os.path.splitext(os.path.basename(n))[0].lower() in BUSY})


def target_kind(metadata_kinds):
    """The word cargo uses for a `cargo metadata` target kind in fingerprint file names."""
    if {"test"} & set(metadata_kinds):
        return "integration-test"
    if {"custom-build"} & set(metadata_kinds):
        return "build-script"
    if {"bin", "bench", "example"} & set(metadata_kinds):
        return metadata_kinds[0]
    return "lib"


def workspace_members():
    """{package name: {"targets", "features"}} for each workspace member, or None."""
    try:
        out = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"],
            cwd=ROOT, capture_output=True, text=True, check=True,
        ).stdout
        packages = json.loads(out)["packages"]
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError):
        return None
    return {
        package["name"]: {
            "targets": {(target_kind(t["kind"]), t["name"]) for t in package["targets"]},
            "features": set(package["features"]),
        }
        for package in packages
    }


def retired(unit, members):
    """True when a workspace member no longer has this unit's target or features."""
    member = members.get(unit["name"]) if members else None
    if member is None:
        return False
    try:
        features = set(json.loads(unit["features"] or "[]"))
    except ValueError:
        return False
    return not unit["targets"] <= member["targets"] or not features <= member["features"]


def folder_files(folder):
    for parent, _, names in os.walk(folder):
        for name in names:
            yield os.path.join(parent, name)


def size(paths):
    """Bytes in `paths`, counting hard links (cargo's uplifted copies) once."""
    seen = set()
    total = 0
    for path in paths:
        try:
            stat = os.stat(path)
        except OSError:
            continue
        if (stat.st_dev, stat.st_ino) not in seen:
            seen.add((stat.st_dev, stat.st_ino))
            total += stat.st_size
    return total


def newest(paths):
    times = []
    for path in paths:
        try:
            times.append(os.path.getmtime(path))
        except OSError:
            pass
    return max(times, default=0.0)


def units(profile_dir):
    """{hash: unit} for every `.fingerprint` folder whose json reads."""
    files = defaultdict(list)
    for folder in ("deps", "examples"):
        if not os.path.isdir(os.path.join(profile_dir, folder)):
            continue
        for entry in os.scandir(os.path.join(profile_dir, folder)):
            match = HASHED.match(entry.name)
            if match and entry.is_file():
                files[match["hash"]].append(entry.path)
    found = {}
    for folder in glob.glob(os.path.join(profile_dir, ".fingerprint", "*-*")):
        package, unit_hash = os.path.basename(folder).rsplit("-", 1)
        # One folder can hold several targets: a package's bins share one.
        unit = {"name": package, "fingerprints": set(), "built_from": set(), "targets": set(),
                "keys": set(), "features": None}
        for path in glob.glob(os.path.join(folder, "*.json")):
            kind = os.path.basename(path)[: -len(".json")]
            target = TARGET.match(kind)
            if target:
                unit["targets"].add((target["kind"], target["name"]))
            try:
                with open(path, encoding="utf-8") as fh:
                    fingerprint = json.load(fh)
                # Cargo writes the u64 as hex of its little-endian bytes.
                with open(os.path.join(folder, kind), encoding="utf-8") as fh:
                    unit["fingerprints"].add(int.from_bytes(bytes.fromhex(fh.read().strip()), "little"))
            except (OSError, ValueError):
                continue
            unit["built_from"].update(dep[-1] for dep in fingerprint.get("deps", []))
            unit["features"] = fingerprint.get("features")
            unit["keys"].add((package, kind))
        if not unit["fingerprints"]:
            continue
        build = os.path.join(profile_dir, "build", os.path.basename(folder))
        unit["files"] = files.get(unit_hash, [])
        unit["folders"] = [folder] + ([build] if os.path.isdir(build) else [])
        unit["time"] = newest(unit["files"] + list(folder_files(folder)))
        found[unit_hash] = unit
    return found


def stale_units(profile_dir, members, days):
    found = units(profile_dir)
    # Folders can share a fingerprint (build script runs with the same inputs),
    # and nothing on disk says which one a build used, so each one counts.
    by_fingerprint = defaultdict(set)
    for unit_hash, unit in found.items():
        for fp in unit["fingerprints"]:
            by_fingerprint[fp].add(unit_hash)
    for unit in found.values():
        unit["built_from"] = set().union(*(by_fingerprint.get(fp, ()) for fp in unit["built_from"]))
    used = set()
    newest_of_key = {}
    for unit_hash, unit in found.items():
        used |= unit["built_from"]
        for key in unit["keys"]:
            best = newest_of_key.get(key)
            if best is None or unit["time"] > found[best]["time"]:
                newest_of_key[key] = unit_hash
    # A build script only serves its package's library, so it is never a root.
    roots = [unit_hash for unit_hash, unit in found.items()
             if unit_hash not in used
             and any(kind != "build-script" for kind, _ in unit["targets"])
             and not retired(unit, members)]
    cutoff = time.time() - days * 86400
    live = set()
    stack = [unit_hash for unit_hash in roots
             if found[unit_hash]["time"] >= cutoff
             or any(newest_of_key[key] == unit_hash for key in found[unit_hash]["keys"])]
    while stack:
        unit_hash = stack.pop()
        if unit_hash not in live:
            live.add(unit_hash)
            stack.extend(found[unit_hash]["built_from"])
    return [unit for unit_hash, unit in found.items() if unit_hash not in live]


def gb(n):
    return f"{n / 1e9:.2f} GB"


def remove(unit):
    for path in unit["files"]:
        try:
            os.remove(path)
        except OSError as err:
            print(f"  kept {path}: {err}")
    for folder in unit["folders"]:
        shutil.rmtree(folder, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--delete", action="store_true", help="delete what no recent build uses")
    parser.add_argument("--days", type=float, default=1, help="also keep builds this recent")
    parser.add_argument("--target-dir", default=os.path.join(ROOT, "target"))
    args = parser.parse_args()

    busy = busy_processes()
    if busy:
        sys.exit(f"cargo or rustc is running ({', '.join(busy)}); prune when it finishes.")

    members = workspace_members()
    if members is None:
        print("cargo metadata failed: keeping units of removed targets and features.")
    profiles = [os.path.dirname(d) for d in glob.glob(os.path.join(args.target_dir, "*", "deps"))]
    profiles += [os.path.dirname(d) for d in glob.glob(os.path.join(args.target_dir, "*", "*", "deps"))]
    for profile_dir in sorted(profiles):
        before = size(folder_files(profile_dir))
        stale = stale_units(profile_dir, members, args.days)
        by_name = defaultdict(lambda: [0, 0])
        for unit in stale:
            unit["size"] = size(unit["files"]) + sum(size(folder_files(f)) for f in unit["folders"])
            by_name[unit["name"]][0] += 1
            by_name[unit["name"]][1] += unit["size"]
        verb = "deleted" if args.delete else "would delete"
        freed = sum(unit["size"] for unit in stale)
        print(f"{profile_dir}: {gb(before)}, {verb} {len(stale)} units, {gb(freed)}")
        for name, (count, total) in sorted(by_name.items(), key=lambda item: -item[1][1])[:10]:
            print(f"  {name}: {count}, {gb(total)}")
        if args.delete:
            for unit in stale:
                remove(unit)
            print(f"  now {gb(size(folder_files(profile_dir)))}")


if __name__ == "__main__":
    main()
