import importlib.util
import json
from pathlib import Path
import plistlib
import re
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
import unittest
import warnings
import zipfile
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/package_release.py"
TAG = "v0.1.0-alpha.1"
COMMIT = "0123456789abcdef0123456789abcdef01234567"
TARGETS = {
    "x86_64-pc-windows-msvc": "Motor-Trail-Windows-x64.zip",
    "aarch64-apple-darwin": "Motor-Trail-macOS-AppleSilicon.zip",
    "x86_64-apple-darwin": "Motor-Trail-macOS-Intel.zip",
}
VEHICLES = ["Baseline.json", "Confort.json", "Dynamique.json",
            "Franchissement.json", "Origine.json", "motor_trail_4x4.mounts.json"]
TERRAINS = ["Exemple Plat.json", "Exemple Relief.json"]
NATURE_IDS = (
    "tree_oak_round", "tree_oak_wide", "tree_oak_tall", "tree_young",
    "tree_pine_small", "tree_pine_medium", "tree_pine_tall",
    "rock_boulder_small", "rock_boulder_medium", "rock_boulder_large",
    "rock_slab_flat", "rock_cluster", "bush_low", "bush_round", "bush_wild",
    "grass_short", "grass_tall", "grass_wild", "fern_small", "fern_large",
    "log_fallen_small", "log_fallen_large", "stump_old",
)
NATURE_CATALOG = "assets/3D/catalog.json"
NATURE_MODELS = ["assets/3D/assets/" + name + ".glb" for name in NATURE_IDS]
NATURE_SOURCES = NATURE_MODELS + [NATURE_CATALOG]
SOURCES = ["assets/vehicles/motor_trail_4x4.glb"] + [
    "configs/vehicles/" + name for name in VEHICLES
] + ["configs/terrains/" + name for name in TERRAINS] + NATURE_SOURCES


def fixture_binary(target):
    if target.endswith("windows-msvc"):
        data = bytearray(512)
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 0x3C, 0x80)
        data[0x80:0x84] = b"PE\0\0"
        struct.pack_into("<HHIIIHH", data, 0x84, 0x8664, 1, 0, 0, 0, 240, 0x22)
        struct.pack_into("<H", data, 0x98, 0x20B)
        return bytes(data)
    cpu = 0x100000C if target.startswith("aarch64") else 0x1000007
    return struct.pack("<IIIIIIII", 0xFEEDFACF, cpu, 0, 2, 0, 0, 0, 0)


if SCRIPT.is_file():
    spec = importlib.util.spec_from_file_location("package_release", SCRIPT)
    package = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(package)
else:
    package = None


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.assertIsNotNone(package, "packaging implementation must exist")
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "source"
        self.out = self.root / "dist"
        for name in SOURCES:
            dest = self.repo / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / name, dest)

    def build(self, target="x86_64-pc-windows-msvc"):
        binary = self.root / ("fixture.exe" if "windows" in target else "fixture")
        binary.write_bytes(fixture_binary(target))
        return package.build_archive(self.repo, binary, self.out, target, TAG, COMMIT)

    def rewrite(self, path, replacements=None, omit=(), extra=()):
        with zipfile.ZipFile(path) as archive:
            items = [(info, archive.read(info)) for info in archive.infolist()]
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", UserWarning)
            with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
                for info, data in items:
                    if info.filename not in omit:
                        archive.writestr(info, (replacements or {}).get(info.filename, data))
                for name, data in extra:
                    archive.writestr(name, data)

    def validate(self, path, target="x86_64-pc-windows-msvc"):
        return package.validate_archive(path, target, TAG, COMMIT)

    def test_three_exact_archive_layouts_and_versions(self):
        for target, filename in TARGETS.items():
            with self.subTest(target=target):
                path = self.build(target)
                self.assertEqual(Path(path).name, filename)
                self.validate(path, target)
                with zipfile.ZipFile(path) as archive:
                    prefix = "Motor-Trail" if "windows" in target else "Motor-Trail.app/Contents/Resources"
                    version = json.loads(archive.read(prefix + "/version.json"))
                    self.assertEqual({key: version[key] for key in ("tag", "target", "commit")},
                                     {"tag": TAG, "target": target, "commit": COMMIT})
                    names = set(archive.namelist())
                    self.assertIn(prefix + "/README.txt", names)
                    self.assertIn(prefix + "/assets/vehicles/motor_trail_4x4.glb", names)
                    for name in VEHICLES:
                        self.assertIn(prefix + "/defaults/vehicles/" + name, names)
                    for name in TERRAINS:
                        self.assertIn(prefix + "/defaults/terrains/" + name, names)
                    expected = {prefix + "/" + name.replace("configs/", "defaults/", 1)
                                for name in SOURCES}
                    expected.update((prefix + "/README.txt", prefix + "/version.json"))
                    if "apple" in target:
                        expected.update(("Motor-Trail.app/Contents/MacOS/motor-trail",
                                         "Motor-Trail.app/Contents/Info.plist"))
                        info = plistlib.loads(archive.read("Motor-Trail.app/Contents/Info.plist"))
                        self.assertEqual(info["CFBundleIdentifier"], "com.motortrail.game")
                        self.assertEqual(info["CFBundleDisplayName"], "Motor-Trail")
                        self.assertEqual(info["CFBundleExecutable"], "motor-trail")
                        self.assertEqual(info["CFBundleShortVersionString"], "0.1.0")
                        self.assertEqual(info["MotorTrailFullVersion"], TAG)
                        self.assertNotIn("CFBundleIconFile", info)
                        mode = archive.getinfo("Motor-Trail.app/Contents/MacOS/motor-trail").external_attr >> 16
                        self.assertEqual(stat.S_IMODE(mode), 0o755)
                    else:
                        expected.add("Motor-Trail/motor-trail.exe")
                    self.assertEqual(names, expected)

    def test_all_nature_models_and_catalog_are_shipped(self):
        self.assertEqual(len(NATURE_IDS), 23)
        for target in TARGETS:
            with self.subTest(target=target):
                with zipfile.ZipFile(self.build(target)) as archive:
                    prefix = ("Motor-Trail" if "windows" in target
                              else "Motor-Trail.app/Contents/Resources")
                    expected = {prefix + "/" + name for name in NATURE_SOURCES}
                    shipped = {name for name in archive.namelist()
                               if name.startswith(prefix + "/assets/3D/")}
                    self.assertEqual(shipped, expected)
                    for name in NATURE_SOURCES:
                        self.assertEqual(archive.read(prefix + "/" + name),
                                         (self.repo / name).read_bytes())
                    catalog = json.loads(archive.read(prefix + "/" + NATURE_CATALOG))
                    self.assertEqual({entry["name"]: entry["file"] for entry in catalog["assets"]},
                                     {name: "assets/" + name + ".glb" for name in NATURE_IDS})

    def test_only_whitelisted_resources_are_shipped(self):
        for name in ["configs/vehicles/.last-profile.json", "configs/vehicles/Custom.json",
                     "configs/vehicles/Baseline.json.tmp", "configs/terrains/Custom.json",
                     "assets/vehicles/generate_motor_trail_4x4.py", "assets/secret.json",
                     "assets/3D/generate_assets.py", "assets/3D/preview.png",
                     "assets/3D/README.md", "assets/3D/STYLE_GUIDE.md",
                     "assets/3D/MASTER_PROMPT.txt", "assets/3D/images/custom.png",
                     "assets/3D/prompts/custom.txt", "assets/3D/assets/custom.glb",
                     "assets/3D/assets/tree_oak_round.glb.tmp", "assets/3D/custom.json"]:
            path = self.repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("unshipped", encoding="utf-8")
        with zipfile.ZipFile(self.build()) as archive:
            self.assertEqual(len(archive.namelist()), len(SOURCES) + 3)
            self.assertEqual({name for name in archive.namelist()
                              if name.startswith("Motor-Trail/assets/3D/")},
                             {"Motor-Trail/" + name for name in NATURE_SOURCES})
            self.assertFalse(any("Custom" in name or ".py" in name or ".last-profile" in name
                                 or ".tmp" in name for name in archive.namelist()))

    def test_build_rejects_missing_resource_without_final_archive(self):
        (self.repo / SOURCES[0]).unlink()
        with self.assertRaises(ValueError):
            self.build()
        self.assertFalse((self.out / TARGETS["x86_64-pc-windows-msvc"]).exists())

    def test_build_rejects_corrupt_shipped_json(self):
        for bad in ("{", "[]", '{"value": NaN}', '{"value": 1, "value": 2}'):
            with self.subTest(bad=bad):
                (self.repo / SOURCES[1]).write_text(bad, encoding="utf-8")
                with self.assertRaises(ValueError):
                    self.build()

    def test_build_rejects_corrupt_glb(self):
        (self.repo / SOURCES[0]).write_bytes(b"glTF" + b"\0" * 20)
        with self.assertRaises(ValueError):
            self.build()

    def test_build_rejects_each_missing_nature_resource(self):
        for name in NATURE_SOURCES:
            with self.subTest(name=name):
                source = self.repo / name
                original = source.read_bytes()
                source.unlink()
                try:
                    with self.assertRaisesRegex(ValueError, re.escape(Path(name).name)):
                        self.build()
                    self.assertFalse((self.out / TARGETS["x86_64-pc-windows-msvc"]).exists())
                finally:
                    source.write_bytes(original)

    def test_build_rejects_each_corrupt_nature_glb(self):
        for name in NATURE_MODELS:
            with self.subTest(name=name):
                source = self.repo / name
                original = source.read_bytes()
                source.write_bytes(b"glTF" + b"\0" * 20)
                try:
                    with self.assertRaisesRegex(ValueError, re.escape(Path(name).name)):
                        self.build()
                    self.assertFalse((self.out / TARGETS["x86_64-pc-windows-msvc"]).exists())
                finally:
                    source.write_bytes(original)

    def test_validator_rejects_each_missing_nature_resource(self):
        for name in NATURE_SOURCES:
            with self.subTest(name=name):
                archive = self.build()
                self.rewrite(archive, omit=["Motor-Trail/" + name])
                with self.assertRaisesRegex(ValueError, re.escape(name)):
                    self.validate(archive)

    def test_validator_rejects_each_corrupt_nature_glb(self):
        archive = self.build()
        for name in NATURE_MODELS:
            with self.subTest(name=name):
                # Keep a valid GLB header but break the embedded chunk framing.
                original = (self.repo / name).read_bytes()
                data = bytearray(original)
                struct.pack_into("<I", data, 12, len(data) * 2)
                self.rewrite(archive, {"Motor-Trail/" + name: bytes(data)})
                try:
                    with self.assertRaisesRegex(ValueError, re.escape(name)):
                        self.validate(archive)
                finally:
                    self.rewrite(archive, {"Motor-Trail/" + name: original})

    def assert_catalog_rejected(self, data):
        archive = self.build()
        original_archive = archive.read_bytes()
        source = self.repo / NATURE_CATALOG
        original = source.read_bytes()
        source.write_bytes(data)
        try:
            with self.subTest(operation="build"):
                with self.assertRaisesRegex(ValueError, "catalog.json"):
                    self.build()
                self.assertEqual(archive.read_bytes(), original_archive)
        finally:
            source.write_bytes(original)
        self.rewrite(archive, {"Motor-Trail/" + NATURE_CATALOG: data})
        with self.subTest(operation="validate"):
            with self.assertRaisesRegex(ValueError, "catalog.json"):
                self.validate(archive)

    def test_catalog_rejects_corrupt_json_at_build_and_validation(self):
        for bad in (b"{", b"[]", b"null", b"{}", b"\xff",
                    b'{"assets": [], "value": NaN}',
                    b'{"assets": [], "assets": []}',
                    b'{"assets": [{"name": "tree_young", "name": "tree_young"}]}'):
            with self.subTest(bad=bad):
                self.assert_catalog_rejected(bad)

    def test_catalog_rejects_wrong_asset_set_at_build_and_validation(self):
        original = (self.repo / NATURE_CATALOG).read_bytes()
        invalid_assets = [None, {}, [], [None] * 23, [{}] * 23]
        assets = json.loads(original)["assets"]
        invalid_assets.extend((assets[:-1], assets + [assets[0]], assets[:-1] + [assets[0]]))
        for index, entries in enumerate(invalid_assets):
            with self.subTest(case=index):
                catalog = json.loads(original)
                catalog["assets"] = entries
                self.assert_catalog_rejected(json.dumps(catalog).encode())
        catalog = json.loads(original)
        del catalog["assets"]
        self.assert_catalog_rejected(json.dumps(catalog).encode())

    def test_catalog_rejects_unknown_ids_and_mismatching_paths(self):
        original = (self.repo / NATURE_CATALOG).read_bytes()
        changes = [("name", value) for value in
                   (None, 23, [], {}, "unknown_tree", "TREE_OAK_ROUND", "../tree_oak_round")]
        changes += [("file", value) for value in
                    (None, 23, [], {}, "assets/tree_oak_wide.glb", "assets/custom.glb",
                     "assets/tree_oak_round.GLB", "assets/../tree_oak_round.glb",
                     "../assets/tree_oak_round.glb", "/assets/tree_oak_round.glb",
                     "C:/assets/tree_oak_round.glb", "assets\\tree_oak_round.glb",
                     "assets//tree_oak_round.glb", "assets/./tree_oak_round.glb",
                     "https://example.com/tree_oak_round.glb", "assets/%2e%2e/tree_oak_round.glb",
                     "assets/tree_oak_round.glb\x00", "assets/tree_oak_round.glb\n")]
        for key, value in changes:
            with self.subTest(key=key, value=value):
                catalog = json.loads(original)
                if value is None:
                    del catalog["assets"][0][key]
                else:
                    catalog["assets"][0][key] = value
                self.assert_catalog_rejected(json.dumps(catalog).encode())
        catalog = json.loads(original)
        catalog["assets"][0]["name"] = "custom"
        catalog["assets"][0]["file"] = "assets/custom.glb"
        self.assert_catalog_rejected(json.dumps(catalog).encode())

    def test_catalog_accepts_reordered_entries(self):
        source = self.repo / NATURE_CATALOG
        catalog = json.loads(source.read_bytes())
        catalog["assets"].reverse()
        source.write_text(json.dumps(catalog), encoding="utf-8")
        self.validate(self.build())

    def test_build_rejects_source_symlinks(self):
        link = self.repo / SOURCES[1]
        other = self.root / "external.json"
        other.write_text("{}", encoding="utf-8")
        link.unlink()
        try:
            link.symlink_to(other)
        except OSError as error:
            self.skipTest("host does not permit symlinks: " + str(error))
        with self.assertRaises(ValueError):
            self.build()

    def test_build_rejects_symlink_directory(self):
        directory = self.repo / "configs/vehicles"
        moved = self.root / "vehicles"
        directory.rename(moved)
        try:
            directory.symlink_to(moved, target_is_directory=True)
        except OSError as error:
            self.skipTest("host does not permit symlinks: " + str(error))
        with self.assertRaises(ValueError):
            self.build()

    def test_system_alias_above_source_root_is_allowed(self):
        alias = self.root / "system-alias"
        try:
            alias.symlink_to(self.root, target_is_directory=True)
        except OSError as error:
            self.skipTest("host does not permit symlinks: " + str(error))
        binary = self.root / "fixture.exe"
        binary.write_bytes(fixture_binary("x86_64-pc-windows-msvc"))
        archive = package.build_archive(alias / "source", binary, self.out,
                                        "x86_64-pc-windows-msvc", TAG, COMMIT)
        self.validate(archive)

    def test_glb_invalid_json_shapes_are_reported_as_validation_errors(self):
        for document in [{"asset": [], "meshes": [1]},
                         {"asset": {"version": "2.0"}, "meshes": [1], "buffers": None},
                         {"asset": {"version": "2.0"}, "meshes": [1],
                          "buffers": [{"byteLength": 1000}]}]:
            with self.subTest(document=document):
                encoded = json.dumps(document).encode()
                encoded += b" " * (-len(encoded) % 4)
                chunks = struct.pack("<II", len(encoded), 0x4E4F534A) + encoded
                chunks += struct.pack("<II", 4, 0x004E4942) + b"\0" * 4
                data = struct.pack("<4sII", b"glTF", 2, 12 + len(chunks)) + chunks
                (self.repo / SOURCES[0]).write_bytes(data)
                with self.assertRaises(ValueError):
                    self.build()

    def test_validator_reads_crc_of_every_entry(self):
        archive = self.build()
        data = bytearray(Path(archive).read_bytes())
        with zipfile.ZipFile(archive) as opened:
            info = opened.getinfo("Motor-Trail/motor-trail.exe")
        filename_length, extra_length = struct.unpack_from("<HH", data, info.header_offset + 26)
        offset = info.header_offset + 30 + filename_length + extra_length
        data[offset + info.compress_size // 2] ^= 0xFF
        Path(archive).write_bytes(data)
        with self.assertRaises(ValueError):
            self.validate(archive)

    def test_smoke_rejects_checker_writes_and_checker_failure(self):
        archive = self.build()
        def write_data(command, **kwargs):
            (Path(kwargs["cwd"]) / "unexpected.txt").write_text("write", encoding="utf-8")
            return subprocess.CompletedProcess(command, 0)
        with patch.object(package.subprocess, "run", side_effect=write_data):
            with self.assertRaises(ValueError):
                package.smoke_archive(archive, "x86_64-pc-windows-msvc", TAG, COMMIT)
        with patch.object(package.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "check")):
            with self.assertRaises(subprocess.CalledProcessError):
                package.smoke_archive(archive, "x86_64-pc-windows-msvc", TAG, COMMIT)

    def test_wrong_architecture_is_rejected_at_build_and_validation(self):
        for target in TARGETS:
            with self.subTest(target=target):
                archive = self.build(target)
                executable = ("Motor-Trail/motor-trail.exe" if "windows" in target
                              else "Motor-Trail.app/Contents/MacOS/motor-trail")
                wrong = fixture_binary("aarch64-apple-darwin" if "aarch64" not in target
                                       else "x86_64-apple-darwin")
                self.rewrite(archive, {executable: wrong})
                with self.assertRaises(ValueError):
                    self.validate(archive, target)
                binary = self.root / "wrong"
                binary.write_bytes(wrong)
                with self.assertRaises(ValueError):
                    package.build_archive(self.repo, binary, self.out, target, TAG, COMMIT)

    def test_validator_rejects_missing_files_and_corrupt_data(self):
        entries = ["motor-trail.exe", "README.txt", "version.json",
                   "assets/vehicles/motor_trail_4x4.glb", "defaults/vehicles/Origine.json",
                   "defaults/vehicles/motor_trail_4x4.mounts.json", "defaults/terrains/Exemple Plat.json"]
        for entry in entries:
            with self.subTest(missing=entry):
                archive = self.build()
                self.rewrite(archive, omit=["Motor-Trail/" + entry])
                with self.assertRaises(ValueError):
                    self.validate(archive)
        for entry, bad in [("assets/vehicles/motor_trail_4x4.glb", b"bad"),
                           ("defaults/vehicles/Origine.json", b"{"),
                           ("defaults/terrains/Exemple Plat.json", b"null"),
                           ("README.txt", b"")]:
            with self.subTest(corrupt=entry):
                archive = self.build()
                self.rewrite(archive, {"Motor-Trail/" + entry: bad})
                with self.assertRaises(ValueError):
                    self.validate(archive)

    def test_validator_rejects_glb_with_valid_header_and_broken_chunks(self):
        archive = self.build()
        data = bytearray((self.repo / SOURCES[0]).read_bytes())
        struct.pack_into("<I", data, 12, len(data) * 2)
        self.rewrite(archive, {"Motor-Trail/" + SOURCES[0]: bytes(data)})
        with self.assertRaises(ValueError):
            self.validate(archive)

    def test_validator_rejects_version_mismatch_and_missing_contract_fields(self):
        for key, value in [("tag", "v9.9.9"), ("target", "aarch64-apple-darwin"),
                           ("commit", "f" * 40), ("commit", "bad"), ("tag", None),
                           ("target", None), ("commit", None)]:
            with self.subTest(key=key, value=value):
                archive = self.build()
                metadata = {"tag": TAG, "target": "x86_64-pc-windows-msvc", "commit": COMMIT}
                if value is None:
                    del metadata[key]
                else:
                    metadata[key] = value
                self.rewrite(archive, {"Motor-Trail/version.json": json.dumps(metadata).encode()})
                with self.assertRaises(ValueError):
                    self.validate(archive)

    def test_validator_rejects_bad_plist_and_nonexecutable_mac_binary(self):
        target = "aarch64-apple-darwin"
        for invalid in (b"broken", b'<?xml version="1.0"?><plist><dict>'):
            with self.subTest(plist=invalid):
                archive = self.build(target)
                self.rewrite(archive, {"Motor-Trail.app/Contents/Info.plist": invalid})
                with self.assertRaises(ValueError):
                    self.validate(archive, target)
        archive = self.build(target)
        info = zipfile.ZipInfo("Motor-Trail.app/Contents/MacOS/motor-trail")
        info.external_attr = (stat.S_IFREG | 0o644) << 16
        self.rewrite(archive, omit=[info.filename], extra=[(info, fixture_binary(target))])
        with self.assertRaises(ValueError):
            self.validate(archive, target)

    def test_validator_rejects_unsafe_extra_duplicate_and_symlink_entries(self):
        for name in ["../escape", "/absolute", "C:/escape", "Motor-Trail\\escape",
                     "Motor-Trail/../escape", "Motor-Trail//escape", "Motor-Trail/./escape",
                     "Motor-Trail/custom.json", "motor-trail/README.txt", "Motor-Trail/README.txt"]:
            with self.subTest(name=name):
                archive = self.build()
                self.rewrite(archive, extra=[(name, b"bad")])
                with self.assertRaises(ValueError):
                    self.validate(archive)
        archive = self.build()
        info = zipfile.ZipInfo("Motor-Trail/README.txt")
        info.create_system = 3
        info.external_attr = (stat.S_IFLNK | 0o777) << 16
        self.rewrite(archive, omit=[info.filename], extra=[(info, b"/etc/passwd")])
        with self.assertRaises(ValueError):
            self.validate(archive)

    def test_validator_rejects_non_zip_and_wrong_archive_filename(self):
        archive = self.build()
        wrong = self.out / "wrong.zip"
        shutil.copyfile(archive, wrong)
        with self.assertRaises(ValueError):
            self.validate(wrong)
        archive.write_bytes(b"not a zip")
        with self.assertRaises(ValueError):
            self.validate(archive)

    def test_publish_validation_requires_all_three_and_matching_metadata(self):
        self.build()
        with self.assertRaises(ValueError):
            package.validate_all(self.out, TAG, COMMIT)
        for target in TARGETS:
            self.build(target)
        paths = package.validate_all(self.out, TAG, COMMIT)
        self.assertEqual({Path(path).name for path in paths}, set(TARGETS.values()))
        (self.out / "unexpected.zip").write_bytes(b"bad")
        with self.assertRaises(ValueError):
            package.validate_all(self.out, TAG, COMMIT)

    def test_cli_build_and_validate_from_unrelated_cwd(self):
        binary = self.root / "fixture.exe"
        binary.write_bytes(fixture_binary("x86_64-pc-windows-msvc"))
        common = ["--target", "x86_64-pc-windows-msvc", "--tag", TAG, "--commit", COMMIT]
        result = subprocess.run([sys.executable, "-B", str(SCRIPT), "build", "--repo-root",
                                 str(self.repo), "--binary", str(binary), "--output-dir", str(self.out),
                                 *common], cwd=self.root, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        result = subprocess.run([sys.executable, "-B", str(SCRIPT), "validate",
                                 str(self.out / TARGETS[common[1]]), *common],
                                cwd=self.root, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_smoke_extracts_and_runs_checker_from_unrelated_temp_directory(self):
        archive = self.build()
        def check(command, **kwargs):
            executable = Path(command[0])
            cwd = Path(kwargs["cwd"])
            self.assertTrue(executable.is_file())
            self.assertTrue(cwd.is_dir())
            self.assertNotIn(cwd, executable.parents)
            self.assertEqual(command[1:2], ["--check-installation"])
            self.assertTrue(kwargs.get("check"))
            self.assertGreater(kwargs.get("timeout", 0), 0)
            return subprocess.CompletedProcess(command, 0)
        with patch.object(package.subprocess, "run", side_effect=check) as runner:
            package.smoke_archive(archive, "x86_64-pc-windows-msvc", TAG, COMMIT)
        self.assertEqual(runner.call_count, 1)

    def test_invalid_tag_commit_target_are_rejected(self):
        archive = self.build()
        for tag, commit, target in [("main", COMMIT, next(iter(TARGETS))),
                                    ("v0.1", COMMIT, next(iter(TARGETS))),
                                    (TAG, "abc", next(iter(TARGETS))),
                                    (TAG, COMMIT, "x86_64-unknown-linux-gnu")]:
            with self.subTest(tag=tag, commit=commit, target=target):
                with self.assertRaises(ValueError):
                    package.validate_archive(archive, target, tag, commit)


if __name__ == "__main__":
    unittest.main()
