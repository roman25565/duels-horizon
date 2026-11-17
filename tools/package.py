#!/usr/bin/env python3
"""Packs mixins/duels/ into dist/duels-<version>.hnmod (horizon-native.json + mixins/duels/*.rs|*.toml)."""
import hashlib, json, tomllib, zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
bundle = ROOT / 'mixins' / 'duels'
mixin = tomllib.loads((bundle / 'horizon-mixin.toml').read_text(encoding='utf-8'))
compat = json.loads((ROOT / 'compatibility.json').read_text(encoding='utf-8'))
mod = mixin['mod']
assert mixin['compatibility']['horizon_revision'] == compat['horizon_revision']
manifest = dict(schema=1, id=mod['id'], version=mod['version'], game='26.3',
                horizon=compat['horizon_revision'], mixins=compat['horizon_mixins_revision'])
files = {'horizon-native.json': (json.dumps(manifest, indent=2, sort_keys=True) + '\n').encode()}
for p in sorted(bundle.iterdir()):
    if p.is_file() and p.suffix in ('.rs', '.toml'):
        files[p.relative_to(ROOT).as_posix()] = p.read_bytes()
out = ROOT / 'dist' / f"{mod['id']}-{mod['version']}.hnmod"
out.parent.mkdir(exist_ok=True)
with zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED) as z:
    for name, data in sorted(files.items()):
        info = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
        info.compress_type = zipfile.ZIP_DEFLATED
        info.external_attr = 0o100644 << 16
        z.writestr(info, data)
print(out, hashlib.sha256(out.read_bytes()).hexdigest())
