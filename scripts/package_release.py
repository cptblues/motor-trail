"""Build and check native distributions using Python 3.10+ and no dependencies."""

import argparse
import json
import os
from pathlib import Path
import plistlib
import re
import stat
import struct
import subprocess
import sys
import tempfile
from xml.parsers.expat import ExpatError
import zipfile
import zlib


ARCHIVES = {
    "x86_64-pc-windows-msvc": "Motor-Trail-Windows-x64.zip",
    "aarch64-apple-darwin": "Motor-Trail-macOS-AppleSilicon.zip",
    "x86_64-apple-darwin": "Motor-Trail-macOS-Intel.zip",
}
VEHICLES = (
    "Baseline.json", "Confort.json", "Dynamique.json", "Franchissement.json",
    "Origine.json", "motor_trail_4x4.mounts.json",
)
TERRAINS = ("Exemple Plat.json", "Exemple Relief.json")
MODEL = "assets/vehicles/motor_trail_4x4.glb"
# Explicit whitelist: adding a new shipped resource requires a deliberate change.
RESOURCES = {MODEL: MODEL}
RESOURCES.update({"defaults/vehicles/" + name: "configs/vehicles/" + name for name in VEHICLES})
RESOURCES.update({"defaults/terrains/" + name: "configs/terrains/" + name for name in TERRAINS})
TAG_PATTERN = re.compile(
    r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
)
README = """Motor-Trail

Extract the entire archive before starting the application.
Windows: open Motor-Trail/motor-trail.exe.
macOS: open Motor-Trail.app (Apple Silicon or Intel, matching your Mac).
This application is unsigned. macOS Gatekeeper may prevent it from opening.
No signing or notarization is claimed by this package.

Keep the assets and defaults alongside the application. These files are read-only
reference resources. Custom profiles, terrains and settings are stored separately:
Windows: %APPDATA%/MotorTrail/
macOS: ~/Library/Application Support/MotorTrail/
Updating this installation does not require deleting those user-data directories.

For a headless resource check, run the executable with --check-installation.
"""


def metadata(target, tag, commit):
    if target not in ARCHIVES:
        raise ValueError("unsupported distribution target: " + str(target))
    if not isinstance(tag, str) or not TAG_PATTERN.fullmatch(tag):
        raise ValueError("tag must be vMAJOR.MINOR.PATCH with an optional prerelease/build suffix")
    if not isinstance(commit, str) or not re.fullmatch(r"[0-9a-fA-F]{40}", commit):
        raise ValueError("commit must be a full 40-character hexadecimal SHA")
    return {"tag": tag, "target": target, "commit": commit}


def layout(target):
    if target == "x86_64-pc-windows-msvc":
        return "Motor-Trail", "Motor-Trail/motor-trail.exe"
    return "Motor-Trail.app/Contents/Resources", "Motor-Trail.app/Contents/MacOS/motor-trail"


def bundle_info(tag):
    version = ".".join(TAG_PATTERN.fullmatch(tag).groups())
    return {
        "CFBundleIdentifier": "com.motortrail.game",
        "CFBundleDisplayName": "Motor-Trail",
        "CFBundleName": "Motor-Trail",
        "CFBundleExecutable": "motor-trail",
        "CFBundlePackageType": "APPL",
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "MotorTrailFullVersion": tag,
        "NSHighResolutionCapable": True,
    }


def strict_object(data, name):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError("duplicate JSON key: " + key)
            result[key] = value
        return result

    def invalid_constant(value):
        raise ValueError("non-finite JSON value: " + value)

    try:
        value = json.loads(data.decode("utf-8"), object_pairs_hook=pairs,
                           parse_constant=invalid_constant)
    except (UnicodeError, ValueError) as error:
        raise ValueError("invalid JSON in " + name + ": " + str(error)) from error
    if not isinstance(value, dict) or not value:
        raise ValueError("expected a nonempty JSON object in " + name)
    return value


def check_binary(data, target):
    if target == "x86_64-pc-windows-msvc":
        if len(data) < 64 or data[:2] != b"MZ":
            raise ValueError("expected a Windows PE executable")
        offset = struct.unpack_from("<I", data, 0x3C)[0]
        if offset < 64 or offset + 26 > len(data) or data[offset:offset + 4] != b"PE\0\0":
            raise ValueError("invalid PE header")
        machine = struct.unpack_from("<H", data, offset + 4)[0]
        characteristics = struct.unpack_from("<H", data, offset + 22)[0]
        optional_magic = struct.unpack_from("<H", data, offset + 24)[0]
        if machine != 0x8664 or optional_magic != 0x20B:
            raise ValueError("expected a Windows x64 PE32+ executable")
        if not characteristics & 0x2 or characteristics & 0x2000:
            raise ValueError("PE image must be executable, not a DLL")
    else:
        if len(data) < 32 or data[:4] != b"\xcf\xfa\xed\xfe":
            raise ValueError("expected a thin 64-bit Mach-O executable")
        cpu, _, filetype = struct.unpack_from("<III", data, 4)
        expected_cpu = 0x100000C if target == "aarch64-apple-darwin" else 0x1000007
        if cpu != expected_cpu or filetype != 2:
            raise ValueError("Mach-O architecture or executable type does not match " + target)


def check_glb(data, name):
    if len(data) < 20 or data[:4] != b"glTF":
        raise ValueError("invalid GLB header: " + name)
    version, length = struct.unpack_from("<II", data, 4)
    if version != 2 or length != len(data):
        raise ValueError("invalid GLB version or length: " + name)
    chunks = []
    offset = 12
    while offset < len(data):
        if offset + 8 > len(data):
            raise ValueError("truncated GLB chunk: " + name)
        length, kind = struct.unpack_from("<II", data, offset)
        offset += 8
        if length == 0 or length % 4 or offset + length > len(data):
            raise ValueError("invalid GLB chunk length: " + name)
        chunks.append((kind, data[offset:offset + length]))
        offset += length
    if not chunks or chunks[0][0] != 0x4E4F534A:
        raise ValueError("GLB must start with a JSON chunk: " + name)
    if len(chunks) != 2 or chunks[1][0] != 0x004E4942:
        raise ValueError("vehicle GLB must contain JSON and embedded binary chunks: " + name)
    document = strict_object(chunks[0][1], name)
    asset = document.get("asset")
    meshes = document.get("meshes")
    if (not isinstance(asset, dict) or asset.get("version") != "2.0"
            or not isinstance(meshes, list) or not meshes
            or any(not isinstance(mesh, dict) for mesh in meshes)):
        raise ValueError("GLB must contain a glTF 2.0 model: " + name)
    buffers = document.get("buffers", [])
    if not isinstance(buffers, list) or len(buffers) != 1 or not isinstance(buffers[0], dict):
        raise ValueError("GLB must contain one embedded buffer: " + name)
    size = buffers[0].get("byteLength")
    if ("uri" in buffers[0] or not isinstance(size, int) or size <= 0
            or not size <= len(chunks[1][1]) <= size + 3):
        raise ValueError("GLB binary buffer length does not match its JSON: " + name)


def read_regular(path, boundary=None):
    path = Path(os.path.abspath(path))
    boundary = Path(os.path.abspath(boundary if boundary is not None else path.parent))
    path.relative_to(boundary)
    # Reject links inside the input tree; system aliases above it (macOS /var) are valid.
    component = path
    while True:
        if component.is_symlink() or getattr(component, "is_junction", lambda: False)():
            raise ValueError("symlinks and junctions are not shipped: " + str(component))
        if component == boundary:
            break
        component = component.parent
    if not path.is_file():
        raise ValueError("missing regular file: " + str(path))
    return path.read_bytes()


def check_resource(name, data):
    if name.endswith(".json"):
        strict_object(data, name)
    elif name.endswith(".glb"):
        check_glb(data, name)


def build_archive(repo_root, binary, output_dir, target, tag, commit):
    version = metadata(target, tag, commit)
    prefix, executable = layout(target)
    files = {}
    files[executable] = read_regular(binary)
    check_binary(files[executable], target)
    for dest, source in RESOURCES.items():
        data = read_regular(Path(repo_root) / source, repo_root)
        check_resource(dest, data)
        files[prefix + "/" + dest] = data
    files[prefix + "/README.txt"] = README.encode("utf-8")
    files[prefix + "/version.json"] = (json.dumps(version, indent=2) + "\n").encode("utf-8")
    if "apple" in target:
        files["Motor-Trail.app/Contents/Info.plist"] = plistlib.dumps(bundle_info(tag))
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    destination = output_dir / ARCHIVES[target]
    # Validate in a private directory before atomically replacing a previous output.
    with tempfile.TemporaryDirectory(prefix="motor-trail-build-", dir=output_dir) as temporary:
        pending = Path(temporary) / destination.name
        with zipfile.ZipFile(pending, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for name, data in sorted(files.items()):
                info = zipfile.ZipInfo(name, date_time=(2020, 1, 1, 0, 0, 0))
                info.create_system = 3
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = (stat.S_IFREG | (0o755 if name == executable else 0o644)) << 16
                archive.writestr(info, data)
        validate_archive(pending, target, tag, commit)
        os.replace(pending, destination)
    return destination


def validate_archive(path, target, tag, commit):
    expected_version = metadata(target, tag, commit)
    path = Path(path)
    if path.name != ARCHIVES[target] or path.is_symlink():
        raise ValueError("expected regular archive named " + ARCHIVES[target])
    prefix, executable = layout(target)
    expected = {prefix + "/" + name for name in RESOURCES}
    expected.update((executable, prefix + "/README.txt", prefix + "/version.json"))
    if "apple" in target:
        expected.add("Motor-Trail.app/Contents/Info.plist")
    try:
        with zipfile.ZipFile(path) as archive:
            names = set()
            folded = set()
            for info in archive.infolist():
                name = info.filename
                parts = name.split("/")
                mode = info.external_attr >> 16
                if (info.orig_filename != name or "\\" in name or ":" in name
                        or any(part in ("", ".", "..") for part in parts)
                        or any(ord(char) < 32 for char in name)
                        or stat.S_IFMT(mode) not in (0, stat.S_IFREG)
                        or info.flag_bits & 1):
                    raise ValueError("unsafe ZIP entry: " + repr(name))
                if name in names or name.casefold() in folded:
                    raise ValueError("duplicate ZIP entry: " + name)
                names.add(name)
                folded.add(name.casefold())
            if names != expected:
                raise ValueError("archive file set mismatch; missing=" + str(sorted(expected - names))
                                 + "; unexpected=" + str(sorted(names - expected)))
            # Reading every member also checks its CRC, including binary and text files.
            for name in names:
                data = archive.read(name)
                check_resource(name, data)
            check_binary(archive.read(executable), target)
            version = strict_object(archive.read(prefix + "/version.json"), "version.json")
            if any(version.get(key) != value for key, value in expected_version.items()):
                raise ValueError("version.json tag/target/commit do not match the release")
            readme = archive.read(prefix + "/README.txt").decode("utf-8")
            if not readme.strip():
                raise ValueError("README.txt must not be empty")
            if "apple" in target:
                info = plistlib.loads(archive.read("Motor-Trail.app/Contents/Info.plist"))
                if any(info.get(key) != value for key, value in bundle_info(tag).items()):
                    raise ValueError("Info.plist does not match the application or release version")
                if "CFBundleIconFile" in info or "CFBundleIconName" in info:
                    raise ValueError("no icon asset is shipped; use the OS generic icon")
                if (archive.getinfo(executable).external_attr >> 16) & 0o111 != 0o111:
                    raise ValueError("macOS executable must retain executable permissions")
    except (OSError, zipfile.BadZipFile, UnicodeError, plistlib.InvalidFileException,
            struct.error, KeyError, TypeError, AttributeError, RuntimeError, zlib.error,
            EOFError, ExpatError) as error:
        raise ValueError("invalid archive " + str(path) + ": " + str(error)) from error
    return path


def validate_all(directory, tag, commit):
    directory = Path(directory)
    names = {path.name for path in directory.iterdir()} if directory.is_dir() else set()
    if names != set(ARCHIVES.values()):
        raise ValueError("release directory must contain exactly the three distribution archives")
    return [validate_archive(directory / name, target, tag, commit)
            for target, name in ARCHIVES.items()]


def smoke_archive(path, target, tag, commit):
    path = validate_archive(path, target, tag, commit)
    _, executable = layout(target)
    with tempfile.TemporaryDirectory(prefix="motor-trail-install-") as install:
        with tempfile.TemporaryDirectory(prefix="motor-trail-cwd-") as unrelated:
            with zipfile.ZipFile(path) as archive:
                archive.extractall(install)
                if "apple" in target:
                    (Path(install) / executable).chmod(0o755)
            environment = os.environ.copy()
            # Isolate accidental user-data writes; the checker must not write anything.
            environment["APPDATA"] = str(Path(unrelated) / "appdata")
            environment["XDG_DATA_HOME"] = str(Path(unrelated) / "xdg-data")
            user_data = Path(unrelated) / "user-data"
            subprocess.run([str(Path(install) / executable), "--check-installation",
                            "--user-data-dir", str(user_data)], cwd=unrelated,
                           env=environment, check=True, timeout=120)
            if any(Path(unrelated).iterdir()):
                raise ValueError("installation checker wrote data into the temporary working directory")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for command in ("build", "validate", "validate-all", "smoke"):
        subparser = commands.add_parser(command)
        subparser.add_argument("--tag", required=True)
        subparser.add_argument("--commit", required=True)
        if command != "validate-all":
            subparser.add_argument("--target", required=True, choices=ARCHIVES)
        if command == "build":
            subparser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[1])
            subparser.add_argument("--binary", type=Path, required=True)
            subparser.add_argument("--output-dir", type=Path, default=Path("dist"))
        elif command == "validate-all":
            subparser.add_argument("--directory", type=Path, required=True)
        else:
            subparser.add_argument("archive", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.command == "build":
            print(build_archive(args.repo_root, args.binary, args.output_dir,
                                args.target, args.tag, args.commit))
        elif args.command == "validate-all":
            for path in validate_all(args.directory, args.tag, args.commit):
                print("Validated " + str(path))
        elif args.command == "validate":
            print("Validated " + str(validate_archive(args.archive, args.target, args.tag, args.commit)))
        else:
            smoke_archive(args.archive, args.target, args.tag, args.commit)
            print("Installation check passed for " + str(args.archive))
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        parser.exit(1, "error: " + str(error) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
