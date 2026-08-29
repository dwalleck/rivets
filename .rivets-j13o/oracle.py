#!/usr/bin/env python3
import fcntl
import os
import pathlib
import subprocess
import sys
import tempfile
import time


def open_lock(path: pathlib.Path):
    return open(path, "a+b", buffering=0)


def try_label(path: pathlib.Path) -> str:
    with open_lock(path) as handle:
        try:
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return "BUSY"
        return "ACQUIRED"


def hold(path: pathlib.Path) -> None:
    with open_lock(path) as handle:
        fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        print("LOCKED", flush=True)
        time.sleep(60)


def parent() -> None:
    with tempfile.TemporaryDirectory(prefix="rivets-j13o-oracle-") as directory:
        root = pathlib.Path(directory)
        same = root / "workspace-a.lock"
        different = root / "workspace-b.lock"
        holder = subprocess.Popen(
            [sys.executable, __file__, "hold", os.fspath(same)],
            stdout=subprocess.PIPE,
            text=True,
        )
        assert holder.stdout is not None
        ready = holder.stdout.readline().strip()
        if ready != "LOCKED":
            raise RuntimeError(f"holder did not lock: {ready!r}")
        print(f"same_workspace={try_label(same)}")
        print(f"different_workspace={try_label(different)}")
        holder.kill()
        holder.wait()
        print(f"after_holder_exit={try_label(same)}")

        with open_lock(same) as first:
            fcntl.flock(first.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            print(f"same_process_second_fd={try_label(same)}")


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "hold":
        hold(pathlib.Path(sys.argv[2]))
    elif len(sys.argv) == 1:
        parent()
    else:
        raise SystemExit("usage: oracle.py [hold LOCK_PATH]")
