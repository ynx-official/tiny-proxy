from __future__ import annotations

import tempfile
import unittest
import os
import subprocess
import sys
from pathlib import Path

from scripts.release_notes import ReleaseNotesError, generate_release_notes


class ReleaseNotesTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.root = Path(self.temp_dir.name)
        (self.root / "docs" / "upgrade" / "v1.2.3").mkdir(parents=True)
        (self.root / "Cargo.toml").write_text(
            '[package]\nname = "tiny-proxy"\nversion = "1.2.3"\n',
            encoding="utf-8",
        )
        (self.root / "Cargo.lock").write_text(
            'version = 4\n\n[[package]]\nname = "tiny-proxy"\nversion = "1.2.3"\n', encoding="utf-8"
        )
        (self.root / "CHANGELOG.md").write_text(
            "# Changelog\n\n"
            "## [1.2.3] - 2026-07-30\n\n"
            "- 发布摘要。\n\n"
            "[1.2.3]: docs/upgrade/v1.2.3/README.md\n",
            encoding="utf-8",
        )
        (self.root / "docs" / "upgrade" / "README.md").write_text(
            "当前代码版本为 [`v1.2.3`](v1.2.3/README.md)。\n\n"
            "| [v1.2.3](v1.2.3/README.md) | 2026-07-30 | 发布摘要。 |\n",
            encoding="utf-8",
        )
        self.detail_path = (
            self.root / "docs" / "upgrade" / "v1.2.3" / "README.md"
        )
        self.detail_path.write_text(
            "# Tiny Proxy v1.2.3\n\n"
            "> 发布日期：2026-07-30\n\n"
            "> 状态：Approved\n> 最后更新：2026-07-30\n\n"
            "## 版本概述\n\n发布概述。\n\n"
            "## 新功能\n\n- 新功能。\n\n"
            "## 行为与界面变化\n\n用户可见变化。\n\n"
            "## 配置与数据兼容性\n\n无迁移。\n\n"
            "## 升级说明\n\n覆盖安装。\n\n"
            "## 破坏性变更与已知问题\n\n无。\n\n"
            "## 验证结果\n\n- 测试通过。\n\n"
            "## 变更依据\n\n- 比较链接。\n\n"
            "[返回版本总览](../README.md)\n",
            encoding="utf-8",
        )

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def test_generate_release_notes_keeps_user_facing_sections(self) -> None:
        notes = generate_release_notes(self.root, "v1.2.3")

        self.assertTrue(notes.startswith("# Tiny Proxy v1.2.3"))
        self.assertIn("> 发布日期：2026-07-30", notes)
        self.assertIn("## 版本概述", notes)
        self.assertIn("## 新功能", notes)
        self.assertNotIn("验证结果", notes)
        self.assertNotIn("变更依据", notes)
        self.assertNotIn("返回版本总览", notes)

    def test_cli_succeeds_when_the_host_console_defaults_to_cp1252(self) -> None:
        environment = dict(os.environ, PYTHONIOENCODING="cp1252", PYTHONUTF8="0")
        script = Path(__file__).with_name("release_notes.py").resolve()
        result = subprocess.run([sys.executable, str(script), "--root", str(self.root), "--tag", "v1.2.3"], env=environment, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", errors="replace"))
        self.assertIn("发布资料校验通过：v1.2.3", result.stdout.decode("utf-8"))

    def test_rejects_tag_that_does_not_match_cargo_version(self) -> None:
        with self.assertRaisesRegex(ReleaseNotesError, "与 Cargo.toml 版本 1.2.3 不一致"):
            generate_release_notes(self.root, "v1.2.4")

    def test_rejects_missing_changelog_entry(self) -> None:
        (self.root / "Cargo.lock").write_text(
            'version = 4\n\n[[package]]\nname = "tiny-proxy"\nversion = "1.2.3"\n', encoding="utf-8"
        )
        (self.root / "CHANGELOG.md").write_text("# Changelog\n", encoding="utf-8")

        with self.assertRaisesRegex(ReleaseNotesError, "缺少必要内容"):
            generate_release_notes(self.root, "v1.2.3")

    def test_rejects_mismatched_lockfile_version(self) -> None:
        lock = self.root / "Cargo.lock"
        lock.write_text(lock.read_text().replace('version = "1.2.3"', 'version = "1.2.2"'), encoding="utf-8")
        with self.assertRaisesRegex(ReleaseNotesError, "Cargo.lock"):
            generate_release_notes(self.root, "v1.2.3")

    def test_draft_can_be_checked_but_cannot_be_published(self) -> None:
        self.detail_path.write_text(self.detail_path.read_text(encoding="utf-8").replace("2026-07-30", "未发布", 1).replace("状态：Approved", "状态：Draft"), encoding="utf-8")
        for path in [self.root / "CHANGELOG.md", self.root / "docs/upgrade/README.md"]:
            path.write_text(path.read_text(encoding="utf-8").replace("2026-07-30", "未发布"), encoding="utf-8")
        self.assertIn("版本概述", generate_release_notes(self.root, "v1.2.3", allow_draft=True))
        with self.assertRaisesRegex(ReleaseNotesError, "Approved"):
            generate_release_notes(self.root, "v1.2.3")


if __name__ == "__main__":
    unittest.main()
