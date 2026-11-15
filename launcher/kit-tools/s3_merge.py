"""Merge base NSP + update NSP RomFS with hactool. Usage: s3_merge.py BASE UPDATE KEYS HACTOOL OUTPUT VENDOR [--in-place]

--in-place: hactool reads the two Program NCAs where they lie inside the NSP files (HACTOOL_INPUT_RANGE and
HACTOOL_BASENCA_RANGE, the engine's tools/hactool/range.patch) instead of copies written next to OUTPUT first, so the
merge needs the free space of the merged image only. Without it the NCAs are copied out, as before."""
import sys, os, re, json, hashlib, shutil, tempfile, subprocess
from pathlib import Path
# The wizard runs this script with -I, which ignores PYTHONUTF8: the log it is redirected to would otherwise take the
# system code page and fail on an NSP whose name has characters outside it.
sys.stdout.reconfigure(encoding='utf-8', errors='replace'); sys.stderr.reconfigure(encoding='utf-8', errors='replace')
base, update, keyp, hactool, output, vendor = map(Path, sys.argv[1:7])
in_place = '--in-place' in sys.argv[7:]
sys.path.insert(0, str(vendor))
from extract_exefs import container_entries, common_tickets, nca_inventory, record_offset, require, load_keys
from extract_romfs import index_romfs
from extract_update_romfs import copy_range, hactool_prod_keys
keys = load_keys(keyp)
tickets = {}; inputs = []
for src, is_up in ((base, False), (update, True)):
    with src.open('rb') as f:
        kind, entries = container_entries(f, src.stat().st_size)
        tickets.update(common_tickets(f, entries))
        progs = [(r, h) for r, h in nca_inventory(f, entries, keys) if r['content_type'] == 0]
        require(len(progs) == 1, 'one Program NCA expected')
        inputs.append((src, *progs[0]))
        print(src.name, 'program', progs[0][0]['name'], progs[0][0]['size'], flush=True)
sections = [i for i in range(4) if inputs[1][2][0x400 + i * 512 + 4] == 4]
require(len(sections) == 1, 'one BKTR section expected')
# The merged image is at most the two Program NCAs together; the copies (without --in-place) take as much again.
nca_bytes = inputs[0][1]['size'] + inputs[1][1]['size']
need = nca_bytes * (1 if in_place else 2) + (1 << 30)
free = shutil.disk_usage(output.parent.parent).free
print('free GiB', free >> 30, 'need approx GiB', need >> 30, 'in place' if in_place else 'with copies', flush=True)
require(free > need, f'under {need >> 30} GiB free')
output.parent.mkdir(parents=True, exist_ok=True)
require(not output.exists(), 'output exists')
UNSIGNED = re.compile(r'\s*(?:Fixed-Key Signature|NPDM Signature) \(FAIL\)')
with tempfile.TemporaryDirectory(prefix='update-merge-', dir=output.parent) as tmp:
    tmp = Path(tmp)
    tk = tmp / 'title.keys'
    fd = os.open(tk, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as f:
        for r, k in tickets.items(): f.write(f'{r.hex()} = {k.hex()}\n')
    hk = tmp / 'hactool.keys'
    hactool_prod_keys(keyp, hk)
    env = dict(os.environ, MNG_TITLE_KEYS=str(tk))
    if in_place:
        (src_base, rec_base, _), (src_up, rec_up, _) = inputs
        bn, un = src_base, src_up
        env['HACTOOL_BASENCA_RANGE'] = f'{record_offset(rec_base)}:{rec_base["size"]}'
        env['HACTOOL_INPUT_RANGE'] = f'{record_offset(rec_up)}:{rec_up["size"]}'
    else:
        bn, un = tmp / 'base.nca', tmp / 'update.nca'
        for (src, rec, _), t in zip(inputs, (bn, un)):
            copy_range(src, record_offset(rec), rec['size'], t)
    result = tmp / 'result'; result.mkdir()
    cmd = [str(hactool), '-x', '-y', '--disablekeywarns', '--suppresskeys', '-k', str(hk), f'--basenca={bn}',
           f'--section{sections[0]}={result / "romfs.bin"}', str(un)]
    cp = subprocess.run(cmd, env=env, capture_output=True, encoding='utf-8', errors='replace')
    log = re.sub(r'[0-9A-Fa-f]{32,}', '<hex removed>', cp.stdout + cp.stderr)
    (output.parent / 'merge_romfs.log').write_text(log, encoding='utf-8')
    checked = '\n'.join(l for l in log.splitlines() if not UNSIGNED.match(l))
    print('hactool exit', cp.returncode)
    if in_place:
        # A hactool without range.patch would have read the NSP itself as an NCA: it must say it read both in place.
        require(all(f'{v}: in place at offset' in log for v in ('HACTOOL_BASENCA_RANGE', 'HACTOOL_INPUT_RANGE')),
                'this hactool cannot read an NCA inside its NSP (range.patch missing)')
    bad = re.findall(r'.*(?:\(FAIL\)|\(INVALID\)|Failed to|Error:).*', checked)
    print('problem lines:', bad[:10])
    if any(re.search(r'Level \d \(FAIL\)', l) for l in bad):
        # The game data does not match its own hashes. An NCA is named after the SHA-256 of its content (the first 16
        # bytes, in hex): hashing the two Program NCAs tells a damaged dump from another cause, and which file it is.
        # Only done here, after a failure: it reads both files once more.
        damaged = []
        for which, (src, rec, _) in zip(('base', 'update'), inputs):
            h = hashlib.sha256()
            with src.open('rb') as f:
                f.seek(record_offset(rec)); left = rec['size']
                while left:
                    chunk = f.read(min(left, 8 << 20))
                    if not chunk: break
                    h.update(chunk); left -= len(chunk)
            ok = left == 0 and rec['name'].lower().startswith(h.hexdigest()[:32])
            print('content check', which, 'matches its name' if ok else 'DAMAGED', flush=True)
            if not ok: damaged.append(which)
        # Exit codes the wizard reads: 41 base damaged, 42 update damaged, 43 both, 40 neither (keys, disk or memory).
        sys.exit({('base',): 41, ('update',): 42, ('base', 'update'): 43}.get(tuple(damaged), 40))
    require(cp.returncode == 0 and not bad, 'hactool failed; see merge_romfs.log')
    require(all(re.search(rf'Level {i} \(GOOD\)', log) for i in range(6)), 'IVFC levels not all verified')
    image = result / 'romfs.bin'
    files = index_romfs(image)
    with image.open('rb') as f: dig = hashlib.file_digest(f, 'sha256').hexdigest()
    (result / 'index.json').write_text(json.dumps(dict(status='pass', romfs_size=image.stat().st_size, romfs_sha256=dig, files=files), indent=1))
    result.rename(output)
print('done files', len(files), 'size', (output / 'romfs.bin').stat().st_size, 'sha256', dig)
