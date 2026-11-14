"""Extract ExeFS of an update NSP (no TotK identity checks). Usage: s3_extract.py NSP KEYS_PATH OUTDIR VENDOR_DIR"""
import sys, struct, hashlib, json, io
from pathlib import Path
# Run with -I, which ignores PYTHONUTF8: keep the redirected log in UTF-8 whatever the system code page is.
sys.stdout.reconfigure(encoding='utf-8', errors='replace'); sys.stderr.reconfigure(encoding='utf-8', errors='replace')
nsp, keyp, out, vendor = map(Path, sys.argv[1:5])
sys.path.insert(0, str(vendor))
from extract_exefs import (container_entries, common_tickets, nca_inventory, nca_ctr_key, record_offset,
                           require, read_at, decrypt, load_keys, partition_entries)
from cryptography.hazmat.primitives.ciphers import modes
keys = load_keys(keyp)
with nsp.open('rb') as f:
    kind, entries = container_entries(f, nsp.stat().st_size)
    tickets = common_tickets(f, entries)
    print('tickets:', len(tickets))
    records = nca_inventory(f, entries, keys)
    for r, h in records:
        print('NCA', r['name'], 'type', r['content_type'], 'title', r['title_id'], 'size', r['size'], 'keygen', max(h[0x206], h[0x220]), 'rights', h[0x230:0x240] != bytes(16))
    progs = [(r, h) for r, h in records if r['content_type'] == 0]
    require(len(progs) == 1, 'expected one Program NCA')
    record, hdr = progs[0]
    try:
        ctr_key = nca_ctr_key(hdr, keys, tickets)
    except ValueError as e:
        print('KEYERR', e); sys.exit(2)
    for i in range(4):
        start, end = struct.unpack_from('<II', hdr, 0x240 + 16 * i)
        if not start: continue
        fs = hdr[0x400 + 512 * i:0x600 + 512 * i]
        require(hashlib.sha256(fs).digest() == hdr[0x280 + 32 * i:0x2A0 + 32 * i], 'FS header hash mismatch')
        print('section', i, 'fstype', fs[2], 'hash', fs[3], 'enc', fs[4])
        if fs[2:4] != b'\x01\x02': continue
        require(fs[4] == 3, 'Only AES-CTR ExeFS supported')
        blocksize, levels = struct.unpack_from('<II', fs, 0x28)
        hashoff, hashsize, pfsoff, pfssize = struct.unpack_from('<QQQQ', fs, 0x30)
        secstart, secsize = start * 512, (end - start) * 512
        def rd(rel, size):
            absolute = secstart + rel; aligned = absolute & ~15
            iv = fs[0x140:0x148][::-1] + (aligned // 16).to_bytes(8, 'big')
            data = read_at(f, record_offset(record) + aligned, size + absolute - aligned)
            return decrypt(ctr_key, modes.CTR(iv), data)[absolute - aligned:]
        hashes = rd(hashoff, hashsize)
        require(hashlib.sha256(hashes).digest() == fs[8:40], 'hash table mismatch')
        pfs = rd(pfsoff, pfssize)
        for idx, pos in enumerate(range(0, pfssize, blocksize)):
            require(hashlib.sha256(pfs[pos:pos + blocksize]).digest() == hashes[idx * 32:(idx + 1) * 32], f'block {idx}')
        files = [(n, pfs[o:o + s]) for n, o, s in partition_entries(io.BytesIO(pfs), 0, len(pfs))]
        out.mkdir(parents=True, exist_ok=True)
        man = []
        for n, d in files:
            (out / n).write_bytes(d)
            man.append(dict(name=n, size=len(d), sha256=hashlib.sha256(d).hexdigest()))
            print('wrote', n, len(d))
        (out.parent / 'extraction-manifest.json').write_text(json.dumps(dict(source=str(nsp), nca_inventory=[r for r, _ in records], program=record, files=man), indent=2))
        break
    else:
        print('no ExeFS section'); sys.exit(3)
