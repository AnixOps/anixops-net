#!/usr/bin/env python3
"""Uploads a signed AAB to a Google Play testing track as a draft release (play-bundle.yml).

The release is left as a draft, so nothing reaches Google review or testers until the maintainer rolls it out in
Play Console. The edit is committed with changesNotSentForReview=true; apps without managed publishing reject that
flag, and then the edit is committed without it (a draft release is still not sent anywhere).

Usage: play-upload.py PACKAGE TRACK AAB RELEASE_NAME [NOTES_FILE]
The service account key (JSON) is read from the PLAY_SERVICE_ACCOUNT_JSON environment variable.
"""

import json
import os
import sys

import requests
from google.auth.transport.requests import Request
from google.oauth2 import service_account

API = "https://androidpublisher.googleapis.com/androidpublisher/v3/applications"
UPLOAD = "https://androidpublisher.googleapis.com/upload/androidpublisher/v3/applications"
SCOPE = "https://www.googleapis.com/auth/androidpublisher"


def fail(message, response=None):
    detail = ""
    if response is not None:
        try:
            detail = json.dumps(response.json().get("error", {}).get("message", response.text))
        except ValueError:
            detail = response.text[:500]
    print(f"::error::{message} {detail}".rstrip())
    sys.exit(1)


def main():
    if len(sys.argv) not in (5, 6):
        fail("usage: play-upload.py PACKAGE TRACK AAB RELEASE_NAME [NOTES_FILE]")
    package, track, aab, release_name = sys.argv[1:5]
    notes = open(sys.argv[5], encoding="utf-8").read().strip() if len(sys.argv) == 6 else ""

    key = os.environ.get("PLAY_SERVICE_ACCOUNT_JSON", "")
    if not key:
        fail("PLAY_SERVICE_ACCOUNT_JSON is not set in the google-play environment")
    credentials = service_account.Credentials.from_service_account_info(json.loads(key), scopes=[SCOPE])
    credentials.refresh(Request())
    session = requests.Session()
    session.headers["Authorization"] = f"Bearer {credentials.token}"

    r = session.post(f"{API}/{package}/edits", json={})
    if r.status_code != 200:
        fail("Could not open an edit; is the service account invited in Play Console with release rights?", r)
    edit = r.json()["id"]
    base = f"{API}/{package}/edits/{edit}"

    with open(aab, "rb") as f:
        r = session.post(
            f"{UPLOAD}/{package}/edits/{edit}/bundles",
            params={"uploadType": "media"},
            headers={"Content-Type": "application/octet-stream"},
            data=f,
            timeout=600,
        )
    if r.status_code != 200:
        fail("Uploading the AAB failed", r)
    version_code = r.json()["versionCode"]
    print(f"Uploaded versionCode {version_code}")

    release = {"name": release_name, "versionCodes": [str(version_code)], "status": "draft"}
    if notes:
        release["releaseNotes"] = [{"language": "en-US", "text": notes[:500]}]
    r = session.put(f"{base}/tracks/{track}", json={"track": track, "releases": [release]})
    if r.status_code != 200:
        fail(f"Could not put the release on the {track} track", r)

    r = session.post(f"{base}:commit", params={"changesNotSentForReview": "true"})
    if r.status_code == 400 and "changesNotSentForReview must not be set" in r.text:
        # Apps without managed publishing reject the flag. The release is a draft, so committing without the flag
        # still puts nothing in front of testers or review until it is rolled out in Play Console.
        r = session.post(f"{base}:commit")
    if r.status_code != 200:
        fail("Committing the edit failed", r)
    print(f"Draft release {release_name} (versionCode {version_code}) is on the {track} track, not sent for review")


if __name__ == "__main__":
    main()
