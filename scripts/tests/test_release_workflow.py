import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest


WORKFLOW = Path(__file__).resolve().parents[2] / ".github/workflows/release.yml"


class ReleaseWorkflowTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue(WORKFLOW.is_file(), "tag release workflow must exist")
        self.text = WORKFLOW.read_text(encoding="utf-8")

    def test_only_version_tag_push_trigger(self):
        self.assertRegex(self.text, r'(?m)^on:\s*\n  push:\s*\n    tags: \[\x27v\*\x27\]')
        self.assertNotRegex(self.text, r"(?m)^\s*(workflow_dispatch|pull_request|schedule|branches):")

    def test_native_matrix_and_build_contract(self):
        for runner, target in [("windows-latest", "x86_64-pc-windows-msvc"),
                               ("macos-15", "aarch64-apple-darwin"),
                               ("macos-15-intel", "x86_64-apple-darwin")]:
            self.assertIn("runner: " + runner, self.text)
            self.assertIn("target: " + target, self.text)
        self.assertNotIn("macos-latest", self.text)
        for fragment in ["max-parallel: 3", "rustc -vV", "host:",
                         "dtolnay/rust-toolchain@stable", "Swatinem/rust-cache@v2",
                         "cargo build --release --locked --target", "MOTOR_TRAIL_DISTRIBUTION: '1'",
                         "-C target-feature=+crt-static", "package_release.py build",
                         "package_release.py smoke", "unittest discover -s scripts/tests"]:
            self.assertIn(fragment, self.text)

    def test_permissions_and_publish_gate(self):
        self.assertRegex(self.text, r"(?m)^permissions:\n  contents: read")
        self.assertEqual(self.text.count("contents: write"), 1)
        publish = self.text.split("\n  publish:", 1)[1]
        self.assertIn("needs: build", publish)
        self.assertIn("contents: write", publish)
        self.assertNotIn("always()", publish)
        self.assertIn("GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}", publish)
        self.assertIn("package_release.py validate-all", publish)
        self.assertLess(publish.index("package_release.py validate-all"), publish.index("gh release create"))
        for fragment in ["--verify-tag", "--draft", "gh release upload", "gh release edit", "--draft=false",
                         "alpha", "beta", "rc", "--prerelease"]:
            self.assertIn(fragment, publish)
        self.assertLess(publish.index("gh release upload"), publish.index("gh release edit"))
        self.assertNotRegex(self.text, r"secrets\.(?!GITHUB_TOKEN)[A-Z_]+")

    def test_both_jobs_derive_metadata_from_checked_out_commit_and_verify_tag(self):
        build, publish = self.text.split("\n  publish:", 1)
        for name, job in (("build", build), ("publish", publish)):
            with self.subTest(job=name):
                self.assertNotIn("github.sha", job)
                self.assertIn('release_commit="$(git rev-parse HEAD)"', job)
                self.assertIn('tag_commit="$(git rev-parse --verify "refs/tags/${RELEASE_TAG}^{commit}")"', job)
                self.assertIn('test "$release_commit" = "$tag_commit"', job)
                self.assertIn('echo "RELEASE_COMMIT=$release_commit" >> "$GITHUB_ENV"', job)
                self.assertLess(job.index("actions/checkout@"), job.index("git rev-parse HEAD"))
                self.assertLess(job.index("git rev-parse HEAD"), job.index("package_release.py"))
                self.assertIn("RELEASE_TAG: ${{ github.ref_name }}", job)

    def test_macos_build_steps_avoid_bash_four_expansions(self):
        build, publish = self.text.split("\n  publish:", 1)
        self.assertNotIn("${RELEASE_TAG,,}", build)
        self.assertNotIn("declare -A", build)
        self.assertIn("runs-on: ubuntu-latest", publish)
        self.assertIn("${RELEASE_TAG,,}", publish)

    def run_metadata_steps(self, matching):
        bash = shutil.which("bash")
        if os.name == "nt":
            git = shutil.which("git")
            bash = str(Path(git).parents[1] / "bin/bash.exe") if git else None
        if not bash or not Path(bash).is_file():
            self.skipTest("Bash is unavailable for isolated workflow shell checks")
        steps = re.findall(
            r"      - name: Resolve checked-out release commit\n        shell: bash\n        run: \|\n"
            r"((?:          [^\n]*\n)+)", self.text)
        self.assertEqual(len(steps), 2, "both jobs must resolve checkout metadata")
        for step in steps:
            with tempfile.TemporaryDirectory() as directory:
                environment = os.environ.copy()
                env_file = Path(directory) / "github-env"
                environment.update(RELEASE_TAG="v0.1.0-alpha.1", GITHUB_ENV=env_file.as_posix(),
                                   TEST_HEAD="a" * 40, TEST_TAG_COMMIT=("a" if matching else "b") * 40)
                # Model an annotated tag: only its peeled commit can match HEAD.
                fake_git = '''git() {
  case "$*" in
    "rev-parse HEAD") echo "$TEST_HEAD" ;;
    "rev-parse --verify refs/tags/${RELEASE_TAG}^{commit}") echo "$TEST_TAG_COMMIT" ;;
    *) return 1 ;;
  esac
}
'''
                result = subprocess.run([bash, "--noprofile", "--norc", "-e", "-o", "pipefail"],
                                        input=fake_git + "".join(line[10:] + "\n" for line in step.splitlines()),
                                        env=environment, text=True, capture_output=True, timeout=15)
                if matching:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(env_file.read_text().strip(), "RELEASE_COMMIT=" + "a" * 40)
                else:
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertFalse(env_file.exists(), "mismatched tags must not export metadata")

    def test_metadata_steps_export_peeled_commit(self):
        self.run_metadata_steps(matching=True)

    def test_metadata_steps_fail_before_export_if_tag_and_head_differ(self):
        self.run_metadata_steps(matching=False)

    def test_yaml_structure_when_parser_is_available(self):
        try:
            import yaml
        except ImportError:
            self.skipTest("PyYAML is optional; stdlib workflow contract checks still run")
        data = yaml.load(self.text, Loader=yaml.BaseLoader)
        self.assertEqual(data["on"], {"push": {"tags": ["v*"]}})
        self.assertEqual(data["permissions"], {"contents": "read"})
        self.assertEqual(set(data["jobs"]), {"build", "publish"})
        matrix = data["jobs"]["build"]["strategy"]["matrix"]["include"]
        self.assertEqual(len(matrix), 3)
        self.assertEqual({row["target"] for row in matrix},
                         {"x86_64-pc-windows-msvc", "aarch64-apple-darwin", "x86_64-apple-darwin"})


if __name__ == "__main__":
    unittest.main()
