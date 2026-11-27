#!/usr/bin/env python3
"""Tests for download_page.py: which release the download page, latest.json and the fixed links point to.

Run: python3 -I .github/scripts/download_page_test.py (CI runs it in the first job and in download-page.yml).
"""

import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import download_page  # noqa: E402


def release(tag, prerelease=False, draft=False):
    base = f"https://github.com/AnixOps/anixops-net/releases/download/{tag}/anixops-net-{tag[1:]}"
    return {
        "tag_name": tag,
        "html_url": f"https://github.com/AnixOps/anixops-net/releases/tag/{tag}",
        "published_at": None if draft else "2026-11-20T00:00:00Z",
        "prerelease": prerelease,
        "draft": draft,
        "assets": [
            {"name": f"anixops-net-{tag[1:]}-windows-x64.msi", "size": 1048576,
             "browser_download_url": f"{base}-windows-x64.msi"},
            {"name": f"anixops-net-{tag[1:]}-android-arm64.apk", "size": 2097152,
             "browser_download_url": f"{base}-android-arm64.apk"},
            {"name": "SHA256SUMS", "size": 100,
             "browser_download_url": f"https://github.com/AnixOps/anixops-net/releases/download/{tag}/SHA256SUMS"},
        ],
    }


class LatestReleaseTest(unittest.TestCase):
    def test_only_prereleases_uses_the_newest_prerelease(self):
        releases = [release("v0.3.0-beta.11", True), release("v0.3.0-beta.10", True)]
        self.assertEqual(download_page.latest_release(releases)["tag_name"], "v0.3.0-beta.11")
        self.assertIsNone(download_page.beta_release(releases))

    def test_a_newer_prerelease_does_not_replace_the_stable_release(self):
        releases = [release("v1.1.0-beta.1", True), release("v1.0.0"), release("v0.3.0-beta.11", True)]
        self.assertEqual(download_page.latest_release(releases)["tag_name"], "v1.0.0")
        self.assertEqual(download_page.beta_release(releases)["tag_name"], "v1.1.0-beta.1")

    def test_a_prerelease_older_than_the_stable_release_is_no_beta_channel(self):
        releases = [release("v1.0.0"), release("v1.0.0-rc.1", True)]
        self.assertEqual(download_page.latest_release(releases)["tag_name"], "v1.0.0")
        self.assertIsNone(download_page.beta_release(releases))

    def test_drafts_are_skipped(self):
        releases = [release("v1.0.1", draft=True), release("v1.1.0-beta.1", True, draft=True), release("v1.0.0")]
        self.assertEqual(download_page.latest_release(releases)["tag_name"], "v1.0.0")
        self.assertIsNone(download_page.beta_release(releases))

    def test_api_order_wins_over_tag_string_order(self):
        # "beta.10" < "beta.9" as strings; the API lists the newest first.
        releases = [release("v1.1.0-beta.10", True), release("v1.1.0-beta.9", True), release("v1.0.0")]
        self.assertEqual(download_page.beta_release(releases)["tag_name"], "v1.1.0-beta.10")

    def test_no_published_release(self):
        self.assertIsNone(download_page.latest_release([release("v1.0.0", draft=True)]))
        self.assertIsNone(download_page.latest_release([]))


class BuildTest(unittest.TestCase):
    def build(self, releases):
        out = tempfile.mkdtemp()
        download_page.build(releases, out, "AnixOps/anixops-net")
        return out

    def read_json(self, path):
        with open(path, encoding="utf-8") as f:
            return json.load(f)

    def read_text(self, path):
        with open(path, encoding="utf-8") as f:
            return f.read()

    def test_stable_with_newer_beta(self):
        out = self.build([release("v1.1.0-beta.1", True), release("v1.0.0")])
        latest = self.read_json(os.path.join(out, "latest.json"))
        self.assertEqual(latest["version"], "v1.0.0")
        self.assertFalse(latest["prerelease"])
        self.assertEqual(latest["channel"], "stable")
        self.assertIn("/v1.0.0/", latest["assets"]["windows"])
        beta = self.read_json(os.path.join(out, "latest-beta.json"))
        self.assertEqual(beta["version"], "v1.1.0-beta.1")
        self.assertEqual(beta["channel"], "beta")
        # The fixed links without beta/ stay on the stable release.
        self.assertIn("/v1.0.0/", self.read_text(os.path.join(out, "windows", "index.html")))
        self.assertIn("/v1.1.0-beta.1/", self.read_text(os.path.join(out, "beta", "windows", "index.html")))
        page = self.read_text(os.path.join(out, "index.html"))
        self.assertIn("最新正式版", page)
        self.assertIn("<h2>测试版</h2>", page)

    def test_page_states_the_client_is_not_offered_in_mainland_china(self):
        page = self.read_text(os.path.join(self.build([release("v1.0.0")]), "index.html"))
        self.assertIn("本软件不向中国大陆提供。", page)
        self.assertIn("https://github.com/AnixOps/anixops-net/blob/main/TERMS.md", page)

    def test_only_prereleases_labels_the_test_version(self):
        out = self.build([release("v0.3.0-beta.11", True)])
        latest = self.read_json(os.path.join(out, "latest.json"))
        self.assertEqual(latest["version"], "v0.3.0-beta.11")
        self.assertTrue(latest["prerelease"])
        self.assertEqual(latest["channel"], "beta")
        self.assertFalse(os.path.exists(os.path.join(out, "latest-beta.json")))
        self.assertFalse(os.path.exists(os.path.join(out, "beta")))
        page = self.read_text(os.path.join(out, "index.html"))
        self.assertIn("最新测试版", page)
        self.assertNotIn("<h2>测试版</h2>", page)

    def test_stable_only(self):
        out = self.build([release("v1.0.0"), release("v0.3.0-beta.11", True)])
        self.assertEqual(self.read_json(os.path.join(out, "latest.json"))["version"], "v1.0.0")
        self.assertFalse(os.path.exists(os.path.join(out, "latest-beta.json")))


if __name__ == "__main__":
    unittest.main()
