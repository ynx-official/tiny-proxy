"""Check the shared packaging/updater contract without invoking any build."""
import re
import unittest
from pathlib import Path

from scripts.update_manifest import expected_asset_names

ROOT = Path(__file__).resolve().parents[1]


class ReleaseContractTest(unittest.TestCase):
    def test_release_targets_only_windows_x64_and_macos_arm64(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        self.assertEqual(re.findall(r"^\s+target: ([a-z0-9_-]+)$", workflow, re.MULTILINE), ["x86_64-pc-windows-msvc", "aarch64-apple-darwin"])
        self.assertEqual(expected_asset_names("v0.1.0"), {
            "tiny-proxy-v0.1.0-windows-x86_64-portable.zip",
            "tiny-proxy-v0.1.0-windows-x86_64-setup.exe",
            "tiny-proxy-v0.1.0-macos-aarch64-portable.zip",
            "tiny-proxy-v0.1.0-macos-aarch64-setup.pkg",
        })
        self.assertIn("if: startsWith(github.ref, 'refs/tags/')", workflow)
        self.assertIn("prerelease:", workflow)

    def test_installer_identity_is_shared_with_runtime_detection(self):
        runtime = (ROOT / "src/updater/install.rs").read_text(encoding="utf-8")
        installer = (ROOT / "scripts/windows/tiny-proxy.iss").read_text(encoding="utf-8")
        app_id = re.search(r"^AppId=\{\{([^}]+)\}", installer, re.MULTILINE).group(1)
        self.assertIn(app_id, runtime)
        self.assertIn("dev.tiny-proxy.app", runtime)
        self.assertIn("dev.tiny-proxy.app", (ROOT / "assets/macos/Info.plist").read_text())

    def test_sources_use_this_repository_only(self):
        for source in (ROOT / "src/updater").glob("*.rs"):
            self.assertNotIn("ynx-official/tiny-shell", source.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
