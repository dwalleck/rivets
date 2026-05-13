"""
Cheapest falsifier for rivets-714v falsifiable-design claim C1:
  The pure URI-formatting function (the proposed extraction from path_to_uri)
  strips the `\\\\?\\` extended-length prefix when present.

Mechanism: implement the proposed Rust logic in Python (synthetic, no I/O,
no Rust compile cycle), feed it the bug-trigger input, check the output
against the hand-computed expected URI.

Independent oracle: hand-computed expected strings (no shared code with the
function under test — the oracle is "what would a human spell out for this
input?"). If the function produces something different, the claim is false.
"""

import sys


def proposed_path_to_uri_str(path_str: str, platform: str = sys.platform) -> str:
    """Python equivalent of the proposed path_to_uri pure-formatter logic.

    Encodes the fix shape:
    1. On Windows, strip a leading `\\\\?\\` extended-length prefix if present.
    2. On Windows, replace backslashes with forward slashes.
    3. On Windows, prepend `file:///` (three slashes total — drive letter follows).
    4. On Unix, prepend `file://` (two slashes — absolute path starts with /).
    """
    if platform == "win32":
        # Strip \\?\ prefix
        if path_str.startswith("\\\\?\\"):
            stripped = path_str[4:]
            # \\?\UNC\ → \\ (UNC handling — out of scope for rivets-714v; see rivets-276h)
            # We don't strip this here, but we also don't claim it's correct.
            if stripped.startswith("UNC\\"):
                # Document the gap: this branch is NOT in scope for the
                # rivets-714v fix. Output something defensible but mark it.
                return f"<UNC out-of-scope per rivets-276h: would be file://{stripped[3:]}>"
            path_str = stripped
        return f"file:///{path_str.replace(chr(92), '/')}"
    else:
        return f"file://{path_str}"


CASES = [
    # (label, input_path, platform, expected_output)
    (
        "C1: strip \\\\?\\ prefix on canonical drive-letter path",
        "\\\\?\\C:\\Users\\dwall\\repos\\rivets\\file.rs",
        "win32",
        "file:///C:/Users/dwall/repos/rivets/file.rs",
    ),
    (
        "C2: regular drive-letter path (no \\\\?\\ prefix)",
        "C:\\Users\\dwall\\repos\\rivets\\file.rs",
        "win32",
        "file:///C:/Users/dwall/repos/rivets/file.rs",
    ),
    (
        "C2 variant: short drive-letter path",
        "C:\\foo.rs",
        "win32",
        "file:///C:/foo.rs",
    ),
    (
        "C3: Unix absolute path",
        "/home/user/rivets/file.rs",
        "linux",
        "file:///home/user/rivets/file.rs",
    ),
]


failures = []
for label, inp, plat, expected in CASES:
    got = proposed_path_to_uri_str(inp, platform=plat)
    status = "PASS" if got == expected else "FAIL"
    if status == "FAIL":
        failures.append((label, inp, expected, got))
    print(f"  [{status}] {label}")
    print(f"           input:    {inp!r}")
    print(f"           expected: {expected!r}")
    print(f"           got:      {got!r}")
    print()


print("=" * 60)
if failures:
    print(f"Result: FAIL — {len(failures)} of {len(CASES)} cases falsify the design.")
    for label, inp, expected, got in failures:
        print(f"  - {label}")
        print(f"    input={inp!r}")
        print(f"    expected={expected!r}")
        print(f"    got={got!r}")
    sys.exit(1)
else:
    print(f"Result: PASS — all {len(CASES)} cases produce expected URIs.")
    print()
    print("C1 (strip \\\\?\\ prefix), C2 (drive-letter path), C3 (Unix path)")
    print("survive their cheapest falsification attempts.")
    print()
    print("The proposed fix logic produces correct URIs for the bug-trigger")
    print("input and for the two regression-preservation inputs. Design may")
    print("proceed to remaining claims (C4-C7).")
