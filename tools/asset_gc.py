"""Conservative project GLB GC. Dry-run by default; all other asset types retained."""
import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
from urllib.parse import unquote


def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for key, child in value.items():
            yield key
            yield from strings(child)
    elif isinstance(value, list):
        for child in value:
            yield from strings(child)


def glb_json(data):
    if len(data) < 20 or struct.unpack_from('<4sII', data) != (b'glTF', 2, len(data)):
        raise ValueError('invalid GLB container')
    length, kind = struct.unpack_from('<II', data, 12)
    if kind != 0x4E4F534A or length > len(data) - 20:
        raise ValueError('invalid GLB JSON chunk')
    return json.loads(data[20:20 + length])


@contextlib.contextmanager
def writer_lock(root):
    meta = root / '.ai-engine'
    if meta.is_symlink():
        raise ValueError('metadata symlink refused')
    meta.mkdir(exist_ok=True)
    lock = meta / 'writer.lock'
    if lock.is_symlink():
        raise ValueError('writer lock symlink refused')
    with lock.open('a+b') as stream:
        if os.name == 'nt':
            import msvcrt
            stream.seek(0)
            msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield


def inventory(root, blender=None):
    files = []
    for directory, dirs, names in os.walk(root, followlinks=False):
        dirs[:] = [d for d in dirs if d != '.git']
        for name in dirs + names:
            if (Path(directory) / name).is_symlink():
                raise ValueError('project symlinks make the reference closure ambiguous')
        files.extend(Path(directory) / name for name in names)
    hashes = {p: hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
    refs, blends, candidates = [], [], []
    for path in files:
        suffix = path.suffix.lower()
        if re.search(r'\.blend\d*$', path.name, re.I):
            blends.append(path)
        elif suffix in ('.json', '.gltf'):
            refs.extend(strings(json.loads(path.read_text(encoding='utf8'))))
        elif suffix == '.glb':
            refs.extend(strings(glb_json(path.read_bytes())))
            if path.is_relative_to(root / 'assets'):
                candidates.append(path)
        elif suffix in ('.py', '.js'):
            refs.append('assets/')
        elif suffix not in ('.png', '.jpg', '.jpeg', '.webp', '.gif', '.hdr', '.exr', '.ktx2', '.dds', '.wav', '.ogg', '.mp3', '.flac', '.m4a', '.bin', '.woff', '.woff2', '.ttf', '.otf'):
            try:
                text = path.read_text(encoding='utf8')
            except UnicodeDecodeError:
                refs.append('assets/')
            else:
                refs.append(text)
                try:
                    refs.extend(strings(json.loads(text)))
                except ValueError:
                    pass
    if blends:
        executable = blender or os.environ.get('BLENDER_BIN') or shutil.which('blender')
        if not executable and Path('/Applications/Blender.app/Contents/MacOS/Blender').is_file():
            executable = '/Applications/Blender.app/Contents/MacOS/Blender'
        if not executable:
            raise ValueError('Blender required to prove source dependencies; no removal allowed')
        result = subprocess.run([executable, '--background', '--factory-startup', '--disable-autoexec', '--python', str(Path(__file__).with_name('asset_gc_blender.py')), '--', *map(str, blends)], capture_output=True, text=True, timeout=180, check=True)
        reports = [line.removeprefix('ASSET_GC_REFERENCES=') for line in result.stdout.splitlines() if line.startswith('ASSET_GC_REFERENCES=')]
        if len(reports) != 1:
            raise ValueError('Blender dependency inventory missing')
        dependencies = json.loads(reports[0])
        refs.extend(dependencies['paths'])
        if dependencies.get('scripts'):
            refs.append('assets/')
        for ref in dependencies['libraries']:
            if not Path(ref).resolve(strict=True).is_relative_to(root):
                raise ValueError('external linked Blender source requires a self-contained project')
    if any(re.search(r'\b(?:spawn\w*|Fn|eval|import)\b', s) for s in refs):
        refs.append('assets/')
    normalized = [unquote(s).replace('\\', '/').casefold() for s in refs]
    garbage = []
    for path in candidates:
        relative = path.relative_to(root).as_posix().casefold()
        if not any(path.name.casefold() in s or relative in s or s == 'assets/' for s in normalized):
            garbage.append(path)
    return sorted(garbage), hashes, len(blends), len(candidates), 'assets/' in normalized


def collect(project, apply=False, blender=None):
    root = Path(project).resolve(strict=True)
    if not (root / 'scene.json').is_file():
        raise ValueError('GC requires a project root containing scene.json')
    with writer_lock(root):
        garbage, hashes, blends, count, retained_all = inventory(root, blender)
        if any(hashlib.sha256(p.read_bytes()).hexdigest() != h for p, h in hashes.items()):
            raise ValueError('project changed during inventory; no removal allowed')
        current = {p for d, dirs, names in os.walk(root) for p in (Path(d) / n for n in names) if '.git' not in p.relative_to(root).parts}
        if current != set(hashes):
            raise ValueError('project files changed during inventory; no removal allowed')
        report = {'dry_run': not apply, 'candidate_glbs': count, 'blender_sources_protected': blends, 'retained_all_glbs': retained_all, 'unreferenced': [str(p.relative_to(root)) for p in garbage], 'bytes': sum(p.stat().st_size for p in garbage), 'removed': []}
        if apply:
            for path in garbage:
                if path.is_symlink() or not path.resolve().is_relative_to(root / 'assets'):
                    raise ValueError('candidate changed; removal refused')
                path.unlink()
                report['removed'].append(str(path.relative_to(root)))
        return report


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('project')
    parser.add_argument('--apply', action='store_true', help='remove proven-unreferenced project GLBs')
    parser.add_argument('--blender')
    args = parser.parse_args()
    try:
        print(json.dumps(collect(args.project, args.apply, args.blender), indent=2))
    except Exception as error:
        print(f'asset GC refused: {error}', file=sys.stderr)
        sys.exit(1)
