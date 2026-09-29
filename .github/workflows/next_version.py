"""Resolve the next WikiBuilder release version.

Scheme: Major.Minor with rollover (0.99 -> 1.00), starting at 0.01.
The last version is read from this repo's releases, so no version file
or version commit is needed.

- Empty override: take the latest release tag (`vX.Y`), bump until free.
- Given override: validate the `X.Y` format, fail if the tag already exists.

Writes `version=X.Y` to $GITHUB_OUTPUT.

Test hooks (never set in the workflow): FAKE_LAST_TAG simulates the latest
release tag (empty string = no releases), FAKE_EXISTING_TAGS is a
comma-separated tag list simulating taken tags.
"""

import os
import re
import subprocess
import sys

TAG_RE = re.compile(r"^v(\d+)\.(\d+)$")
VER_RE = re.compile(r"^(\d+)\.(\d+)$")


def bump(major, minor):
    """Next version with rollover: 0.09 -> 0.10, 0.99 -> 1.00."""
    minor += 1
    if minor > 99:
        major += 1
        minor = 0
    return major, minor


def fmt(major, minor):
    return f"{major}.{minor:02d}"


def tag_exists(repo, tag):
    fake = os.environ.get("FAKE_EXISTING_TAGS")
    if fake is not None:
        return tag in [t for t in fake.split(",") if t]
    cmd = ["gh", "api", f"repos/{repo}/releases/tags/{tag}", "--silent"]
    return subprocess.run(cmd, capture_output=True).returncode == 0


def last_tag(repo):
    fake = os.environ.get("FAKE_LAST_TAG")
    if fake is not None:
        return fake or None
    cmd = [
        "gh", "release", "list", "--repo", repo, "--limit", "1",
        "--json", "tagName", "--jq", ".[0].tagName // empty",
    ]
    out = subprocess.run(cmd, capture_output=True, text=True)
    if out.returncode != 0:
        print(f"::warning::gh release list failed: {out.stderr.strip()}")
        return None
    return out.stdout.strip() or None


def main():
    repo = os.environ.get("GITHUB_REPOSITORY", "TontooOS/WikiBuilder")
    override = os.environ.get("VERSION_OVERRIDE", "").strip()
    if override:
        m = VER_RE.fullmatch(override)
        if not m:
            sys.exit(f"error: version override '{override}' must look like 0.07")
        version = fmt(int(m.group(1)), int(m.group(2)))
        if tag_exists(repo, f"v{version}"):
            sys.exit(f"error: release v{version} already exists")
    else:
        m = TAG_RE.fullmatch(last_tag(repo) or "")
        major, minor = (int(m.group(1)), int(m.group(2))) if m else (0, 0)
        major, minor = bump(major, minor)
        while tag_exists(repo, f"v{fmt(major, minor)}"):
            major, minor = bump(major, minor)
        version = fmt(major, minor)
    line = f"version={version}"
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a") as f:
            f.write(line + "\n")
    print(line)


main()
