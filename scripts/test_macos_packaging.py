"""Exercise packaging stdout on any CI host, isolating only native macOS tools."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]


class MacOSPackagingTest(unittest.TestCase):
    def test_success_stdout_contains_only_the_app_path(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
            tag = f"v{version}"
            for relative in ["Cargo.toml", "Cargo.lock", "CHANGELOG.md", "docs/upgrade/README.md", f"docs/upgrade/{tag}/README.md", "scripts/release_notes.py", "scripts/package-macos-app.sh", "assets/macos/Info.plist"]:
                destination = root / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / relative, destination)
            tools = root / "native-tools"
            tools.mkdir()
            scripts = {
                "python3": '#!/bin/bash\nexec "$PACKAGING_TEST_PYTHON" "$@"\n',
                "lipo": '#!/bin/bash\nprintf "%s\\n" arm64\n',
                "sips": '#!/bin/bash\nexit 0\n',
                "iconutil": '#!/bin/bash\nwhile [ "$#" -gt 0 ]; do if [ "$1" = "-o" ]; then touch "$2"; exit 0; fi; shift; done\nexit 1\n',
                "otool": '#!/bin/bash\nprintf "%s\\n" binary: "  /usr/lib/libSystem.B.dylib (compatibility version 1.0.0)"\n',
                "plutil": '#!/bin/bash\nprintf "%s\\n" "plist checked"\n',
                "codesign": '#!/bin/bash\nprintf "%s\\n" "signature checked"\n',
            }
            for name, content in scripts.items():
                path = tools / name
                path.write_text(content, encoding="utf-8", newline="\n")
                path.chmod(0o755)
            binary = root / "target/tiny-proxy"
            binary.parent.mkdir()
            binary.write_text(f'#!/bin/bash\nprintf "%s\\n" "Tiny Proxy {version}"\n', encoding="utf-8", newline="\n")
            binary.chmod(0o755)
            bash = str(Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "Git/bin/bash.exe") if os.name == "nt" else shutil.which("bash")
            environment = dict(os.environ)
            # Use POSIX separators inside Bash even when Python runs on Windows.
            environment["PATH"] = tools.as_posix() + os.pathsep + environment["PATH"]
            environment["PACKAGING_TEST_PYTHON"] = Path(sys.executable).as_posix()
            result = subprocess.run([bash, str(root / "scripts/package-macos-app.sh"), "--binary", "target/tiny-proxy", "--output-dir", "out", "--skip-build"], env=environment, capture_output=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", errors="replace"))
            lines = result.stdout.decode("utf-8").splitlines()
            self.assertEqual(len(lines), 1, lines)
            self.assertTrue(lines[0].endswith("/out/TinyProxy.app"), lines)
            self.assertIn("发布资料校验通过", result.stderr.decode("utf-8"))
            self.assertTrue((root / "out/TinyProxy.app/Contents/MacOS/tiny-proxy").is_file())


if __name__ == "__main__":
    unittest.main()
