#!/usr/bin/env python3
"""Splatoon 3 save.dat tool (11.2.0 and 11.3.0): decrypt, dump, edit and re-encrypt the game's save, and apply a launcher profile.

The format was measured with the runtime's save trace (TOTK_SAVE_TRACE, runtime/savetrace.c): the game encrypts the
save with the nn::crypto AES-256 functions it imports, and the trace logged their keys, counters and buffers.

  save.dat = seed:u32le | check:u32le | body
  body     = AES-256-CTR(key, counter0) over the plaintext
  key, counter0 = the top byte of each of the first 48 outputs of sead::Random(T[sead::Random(seed).getU32() >> 27])
             (32 key bytes, then 16 counter bytes, big-endian counter); T is a 32-entry u32 table in the game's main
  check    = MurmurHash3_x86_32(body, seed) + the first output of that second generator (mod 2^32)
  plaintext = "MS" 00 04 | game version u32be (0x000B0200 = 11.2.0) | MessagePack map (game) | MessagePack map (player)
             (the version word is only shown, as major.minor.patch, by `check` and `dump`: no command reads it to decide
             anything or writes it, so a save made by an earlier game version is edited and written back with its own)
  map keys = MurmurHash3_x86_32(field name, seed 0), written as uint32

The table T is never stored in this tool: `s3save.py keys --exefs <dir>` reads it from the user's own main (and the
field names from its strings) into a local key file (default s3save.keys.json next to this script, or $S3SAVE_KEYS).

Commands (run with -h for each):
  keys      extract the key table and field names from main into the key file
  check     verify a save's checksum and parse it
  decrypt   save.dat -> plaintext;  encrypt  plaintext -> save.dat (new random seed)
  dump      readable tree (or JSON) of a save
  get/set   read or change fields by path, e.g. 0/Common/Coordinates/HairId or 1/UserName
  profile   apply a launcher profile.json (appearance, first-launch flags) to a save; backs the save up first
  selftest  decrypt -> encode -> re-encrypt round trip of a save
"""
import argparse
import datetime
import json
import os
import re
import shutil
import struct
import sys

MASK = 0xFFFFFFFF
HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_KEYS = os.environ.get('S3SAVE_KEYS') or os.path.join(HERE, 's3save.keys.json')
PLAIN_MAGIC = b'MS\x00\x04'


# ---------------------------------------------------------------- hashes and generators
def murmur3(data, seed=0):
    """MurmurHash3_x86_32."""
    c1, c2 = 0xCC9E2D51, 0x1B873593
    h = seed & MASK
    n = len(data) // 4
    for (k,) in struct.iter_unpack('<I', data[:4 * n]):
        k = (k * c1) & MASK
        k = ((k << 15) | (k >> 17)) & MASK
        h ^= (k * c2) & MASK
        h = ((h << 13) | (h >> 19)) & MASK
        h = (h * 5 + 0xE6546B64) & MASK
    tail = data[4 * n:]
    if tail:
        k = int.from_bytes(tail, 'little')
        k = (k * c1) & MASK
        k = ((k << 15) | (k >> 17)) & MASK
        h ^= (k * c2) & MASK
    h ^= len(data)
    h ^= h >> 16
    h = (h * 0x85EBCA6B) & MASK
    h ^= h >> 13
    h = (h * 0xC2B2AE35) & MASK
    return h ^ (h >> 16)


def key_hash(name):
    return murmur3(name.encode(), 0)


class SeadRandom:
    """sead::Random: xorshift128 seeded through the MT19937 initialiser."""

    def __init__(self, seed):
        x, v = [], seed & MASK
        for i in range(4):
            v = (0x6C078965 * (v ^ (v >> 30)) + i + 1) & MASK
            x.append(v)
        self.x = x

    def u32(self):
        x, y, z, w = self.x
        t = (x ^ (x << 11)) & MASK
        n = (w ^ (w >> 19) ^ t ^ (t >> 8)) & MASK
        self.x = [y, z, w, n]
        return n


def derive(table, seed):
    """-> (key 32 bytes, counter 16 bytes, first output of the second generator)."""
    r = SeadRandom(table[SeadRandom(seed).u32() >> 27])
    outs = [r.u32() for _ in range(48)]
    s = bytes(v >> 24 for v in outs)
    return s[:32], s[32:], outs[0]


# ---------------------------------------------------------------- AES-256-CTR
def _gmul(a, b):
    r = 0
    while b:
        if b & 1:
            r ^= a
        a = ((a << 1) ^ 0x11B) if a & 0x80 else a << 1
        b >>= 1
    return r


def _aes_tables():
    """The AES S-box: the inverse in GF(2^8), then the affine map."""
    inv = [0] * 256
    for a in range(1, 256):
        for b in range(1, 256):
            if _gmul(a, b) == 1:
                inv[a] = b
                break
    rot = lambda x, n: ((x << n) | (x >> (8 - n))) & 0xFF
    return [inv[a] ^ rot(inv[a], 1) ^ rot(inv[a], 2) ^ rot(inv[a], 3) ^ rot(inv[a], 4) ^ 0x63 for a in range(256)]


_SBOX = None


def _xt(a):
    return ((a << 1) ^ 0x1B) & 0xFF if a & 0x80 else a << 1


class _Aes256:
    """Pure-Python AES-256 encryption (CTR needs only the forward cipher); used when `cryptography` is missing."""

    def __init__(self, key):
        global _SBOX
        if _SBOX is None:
            _SBOX = _aes_tables()
        s = _SBOX
        w = [list(key[i:i + 4]) for i in range(0, 32, 4)]
        rcon = 1
        for i in range(8, 60):
            t = list(w[i - 1])
            if i % 8 == 0:
                t = [s[t[1]] ^ rcon, s[t[2]], s[t[3]], s[t[0]]]
                rcon = _xt(rcon)
            elif i % 8 == 4:
                t = [s[b] for b in t]
            w.append([a ^ b for a, b in zip(w[i - 8], t)])
        self.rk = [sum(w[4 * r:4 * r + 4], []) for r in range(15)]

    def block(self, b):
        s = _SBOX
        st = [x ^ k for x, k in zip(b, self.rk[0])]
        for r in range(1, 15):
            st = [s[x] for x in st]
            st = [st[(i + 4 * (i % 4)) % 16] for i in range(16)]   # ShiftRows (column-major state)
            if r != 14:
                m = []
                for c in range(4):
                    a = st[4 * c:4 * c + 4]
                    t = a[0] ^ a[1] ^ a[2] ^ a[3]
                    m += [a[i] ^ t ^ _xt(a[i] ^ a[(i + 1) % 4]) for i in range(4)]
                st = m
            st = [x ^ k for x, k in zip(st, self.rk[r])]
        return bytes(st)


def aes256_ctr(key, counter, data):
    try:
        from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
        e = Cipher(algorithms.AES(key), modes.CTR(counter)).encryptor()
        return e.update(data) + e.finalize()
    except ImportError:
        pass
    a = _Aes256(key)
    c = int.from_bytes(counter, 'big')
    out = bytearray()
    for i in range(0, len(data), 16):
        ks = a.block(c.to_bytes(16, 'big'))
        out += bytes(x ^ y for x, y in zip(data[i:i + 16], ks))
        c = (c + 1) & ((1 << 128) - 1)
    return bytes(out)


# ---------------------------------------------------------------- the save file
class SaveError(Exception):
    pass


def decrypt(table, data):
    """save.dat bytes -> plaintext; raises SaveError when the checksum fails."""
    if len(data) < 16:
        raise SaveError('file too short (%d bytes)' % len(data))
    seed, check = struct.unpack_from('<II', data, 0)
    key, ctr, first = derive(table, seed)
    body = data[8:]
    if (murmur3(body, seed) + first) & MASK != check:
        raise SaveError('checksum mismatch: not a Splatoon 3 save, or the key table is not this game\'s')
    return aes256_ctr(key, ctr, body)


def encrypt(table, plain, seed=None):
    if seed is None:
        seed = int.from_bytes(os.urandom(4), 'little')
    key, ctr, first = derive(table, seed)
    body = aes256_ctr(key, ctr, plain)
    return struct.pack('<II', seed, (murmur3(body, seed) + first) & MASK) + body


# ---------------------------------------------------------------- MessagePack, keeping each value's encoding
class Node:
    """A MessagePack value with the type byte it was read with (fmt), so an unchanged tree encodes to the same bytes.
    kind: int, nil, bool, f32, f64, str, bin, arr, map. map: v = list of [key Node, value Node]."""
    __slots__ = ('kind', 'fmt', 'v')

    def __init__(self, kind, v, fmt=None):
        self.kind, self.v, self.fmt = kind, v, fmt

    def __repr__(self):
        return 'Node(%s, %r)' % (self.kind, self.v if self.kind not in ('arr', 'map') else len(self.v))


def mp_read(b, i):
    t = b[i]
    if t <= 0x7F:
        return Node('int', t, t), i + 1
    if t >= 0xE0:
        return Node('int', t - 256, t), i + 1
    if 0x80 <= t <= 0x8F or t in (0xDE, 0xDF):
        if t <= 0x8F:
            n, i = t & 15, i + 1
        elif t == 0xDE:
            n, i = struct.unpack_from('>H', b, i + 1)[0], i + 3
        else:
            n, i = struct.unpack_from('>I', b, i + 1)[0], i + 5
        items = []
        for _ in range(n):
            k, i = mp_read(b, i)
            v, i = mp_read(b, i)
            items.append([k, v])
        return Node('map', items, t), i
    if 0x90 <= t <= 0x9F or t in (0xDC, 0xDD):
        if t <= 0x9F:
            n, i = t & 15, i + 1
        elif t == 0xDC:
            n, i = struct.unpack_from('>H', b, i + 1)[0], i + 3
        else:
            n, i = struct.unpack_from('>I', b, i + 1)[0], i + 5
        items = []
        for _ in range(n):
            v, i = mp_read(b, i)
            items.append(v)
        return Node('arr', items, t), i
    if 0xA0 <= t <= 0xBF or t in (0xD9, 0xDA, 0xDB, 0xC4, 0xC5, 0xC6):
        if t <= 0xBF:
            n, i = t & 31, i + 1
        elif t in (0xD9, 0xC4):
            n, i = b[i + 1], i + 2
        elif t in (0xDA, 0xC5):
            n, i = struct.unpack_from('>H', b, i + 1)[0], i + 3
        else:
            n, i = struct.unpack_from('>I', b, i + 1)[0], i + 5
        return Node('bin' if t in (0xC4, 0xC5, 0xC6) else 'str', bytes(b[i:i + n]), t), i + n
    if t == 0xC0:
        return Node('nil', None, t), i + 1
    if t in (0xC2, 0xC3):
        return Node('bool', t == 0xC3, t), i + 1
    fixed = {0xCC: '>B', 0xCD: '>H', 0xCE: '>I', 0xCF: '>Q', 0xD0: '>b', 0xD1: '>h', 0xD2: '>i', 0xD3: '>q',
             0xCA: '>f', 0xCB: '>d'}
    if t in fixed:
        f = fixed[t]
        v = struct.unpack_from(f, b, i + 1)[0]
        kind = 'f32' if t == 0xCA else 'f64' if t == 0xCB else 'int'
        return Node(kind, v, t), i + 1 + struct.calcsize(f)
    raise SaveError('MessagePack type %#x at %d is not handled' % (t, i))


_INT_FMT = {0xCC: (0, 0xFF, '>B'), 0xCD: (0, 0xFFFF, '>H'), 0xCE: (0, MASK, '>I'), 0xCF: (0, (1 << 64) - 1, '>Q'),
            0xD0: (-128, 127, '>b'), 0xD1: (-32768, 32767, '>h'), 0xD2: (-(1 << 31), (1 << 31) - 1, '>i'),
            0xD3: (-(1 << 63), (1 << 63) - 1, '>q')}


def _int_bytes(v, fmt):
    if fmt is not None:
        if fmt <= 0x7F and 0 <= v <= 0x7F:
            return bytes([v])
        if fmt >= 0xE0 and -32 <= v < 0:
            return bytes([v & 0xFF])
        if fmt in _INT_FMT and _INT_FMT[fmt][0] <= v <= _INT_FMT[fmt][1]:
            return bytes([fmt]) + struct.pack(_INT_FMT[fmt][2], v)
    if 0 <= v <= 0x7F or -32 <= v < 0:
        return bytes([v & 0xFF])
    for f in ((0xCC, 0xCD, 0xCE, 0xCF) if v >= 0 else (0xD0, 0xD1, 0xD2, 0xD3)):
        if _INT_FMT[f][0] <= v <= _INT_FMT[f][1]:
            return bytes([f]) + struct.pack(_INT_FMT[f][2], v)
    raise SaveError('integer %d does not fit MessagePack' % v)


def _len_prefix(n, small_base, small_max, f8, f16, f32, fmt):
    """Length prefix of a str/arr/map: the format it was read with when the new length fits it, else the smallest."""
    if fmt == f8 and f8 is not None and n <= 0xFF:
        return bytes([f8, n])
    if fmt == f16 and n <= 0xFFFF:
        return bytes([f16]) + struct.pack('>H', n)
    if fmt == f32:
        return bytes([f32]) + struct.pack('>I', n)
    if n <= small_max:
        return bytes([small_base | n])
    if f8 is not None and n <= 0xFF:
        return bytes([f8, n])
    if n <= 0xFFFF:
        return bytes([f16]) + struct.pack('>H', n)
    return bytes([f32]) + struct.pack('>I', n)


def mp_write(node, out):
    k, v, f = node.kind, node.v, node.fmt
    if k == 'int':
        out += _int_bytes(v, f)
    elif k == 'nil':
        out.append(0xC0)
    elif k == 'bool':
        out.append(0xC3 if v else 0xC2)
    elif k == 'f32':
        out += b'\xca' + struct.pack('>f', v)
    elif k == 'f64':
        out += b'\xcb' + struct.pack('>d', v)
    elif k == 'str':
        out += _len_prefix(len(v), 0xA0, 31, 0xD9, 0xDA, 0xDB, f) + v
    elif k == 'bin':
        n = len(v)
        out += (bytes([0xC4, n]) if n <= 0xFF and f in (None, 0xC4) else
                b'\xc5' + struct.pack('>H', n) if n <= 0xFFFF and f != 0xC6 else b'\xc6' + struct.pack('>I', n)) + v
    elif k == 'arr':
        out += _len_prefix(len(v), 0x90, 15, None, 0xDC, 0xDD, f)
        for x in v:
            mp_write(x, out)
    elif k == 'map':
        out += _len_prefix(len(v), 0x80, 15, None, 0xDE, 0xDF, f)
        for kk, vv in v:
            mp_write(kk, out)
            mp_write(vv, out)
    else:
        raise SaveError('node kind %s' % k)


class Plain:
    """A decrypted save: the 8-byte inner header and the MessagePack roots after it (two in 11.2.0 and 11.3.0)."""

    def __init__(self, plain):
        if plain[:4] != PLAIN_MAGIC:
            raise SaveError('plaintext does not start with MS 00 04 (another save format version?)')
        self.header = bytes(plain[:8])
        self.roots, i = [], 8
        while i < len(plain):
            n, i = mp_read(plain, i)
            self.roots.append(n)
        if i != len(plain):
            raise SaveError('MessagePack runs past the end')

    @property
    def game_version(self):
        v = struct.unpack_from('>I', self.header, 4)[0]
        return '%d.%d.%d' % (v >> 16, (v >> 8) & 0xFF, v & 0xFF)

    def encode(self):
        out = bytearray(self.header)
        for r in self.roots:
            mp_write(r, out)
        return bytes(out)


# ---------------------------------------------------------------- key file: the table and the field names
def lz4_block(src, size):
    o = bytearray()
    i = 0
    while i < len(src):
        t = src[i]
        i += 1
        n = t >> 4
        if n == 15:
            while True:
                b = src[i]
                i += 1
                n += b
                if b != 255:
                    break
        o += src[i:i + n]
        i += n
        if i >= len(src):
            break
        off = src[i] | src[i + 1] << 8
        i += 2
        m = t & 15
        if m == 15:
            while True:
                b = src[i]
                i += 1
                m += b
                if b != 255:
                    break
        m += 4
        s = len(o) - off
        if off >= m:
            o += o[s:s + m]
        else:
            for j in range(m):
                o.append(o[s + j])
    if len(o) != size:
        raise SaveError('NSO segment: LZ4 gave %d bytes, header says %d' % (len(o), size))
    return o


def load_nso(path):
    """-> (image bytes, [(mem_off, size)] for text, ro, data)."""
    raw = open(path, 'rb').read()
    if raw[:4] != b'NSO0':
        raise SaveError('%s is not an NSO' % path)
    flags = struct.unpack_from('<I', raw, 0xC)[0]
    segs = [struct.unpack_from('<III', raw, 0x10 + 0x10 * k) for k in range(3)]
    csz = struct.unpack_from('<3I', raw, 0x60)
    mem = bytearray(max(s[1] + s[2] for s in segs))
    for k in range(3):
        fo, mo, ds = segs[k]
        blob = raw[fo:fo + (csz[k] if flags >> k & 1 else ds)]
        mem[mo:mo + ds] = lz4_block(blob, ds) if flags >> k & 1 else blob
    return bytes(mem), [(s[1], s[2]) for s in segs]


def find_table(mem, text):
    """The key table's address: the code that indexes it with getU32() >> 27 (lsr wN, wN, #27, then
    ldr wT, [xB, wN, uxtw #2] with xB from adrp + add), after a sead::Random initialiser (movk #0x6c07, lsl #16)."""
    t0, tsize = text
    found = {}
    for m in re.finditer(rb'[\x00-\xff][\x7c-\x7f]\x1b\x53', mem[t0:t0 + tsize]):
        a = t0 + m.start()
        if a & 3:
            continue
        lsr, ldr = struct.unpack_from('<II', mem, a)
        if (lsr & 0xFFFFFC00) != 0x531B7C00 or (lsr & 31) != ((lsr >> 5) & 31):
            continue
        n = lsr & 31
        if (ldr & 0xFFE0FC00) != 0xB8605800 or ((ldr >> 16) & 31) != n:
            continue
        base = (ldr >> 5) & 31
        window = struct.unpack_from('<24I', mem, a - 96)
        if not any((w & 0xFFFFFFE0) == 0x72AD80E0 for w in window):   # movk wX, #0x6c07, lsl #16
            continue
        page = lo = None
        for k in range(len(window) - 1, -1, -1):
            w, pc = window[k], a - 96 + 4 * k
            if lo is None and (w & 0xFFC00000) == 0x91000000 and (w & 31) == base and ((w >> 5) & 31) == base:
                lo = (w >> 10) & 0xFFF
            elif lo is not None and (w & 0x9F00001F) == (0x90000000 | base):
                imm = ((w >> 29) & 3) | (((w >> 5) & 0x7FFFF) << 2)
                if imm & (1 << 20):
                    imm -= 1 << 21
                page = (pc & ~0xFFF) + (imm << 12)
                break
        if page is not None:
            found[page + lo] = found.get(page + lo, 0) + 1
    if not found:
        raise SaveError('the key table code was not found in main (another game version?)')
    if len(found) > 1:
        raise SaveError('several key table candidates in main: %s' % ', '.join('%#x' % k for k in found))
    return next(iter(found))


def find_enum(mem, segs, first):
    """An enum's reflection string in main ("None = -1 , MainInk_Save = 0 , ...") -> {name: value}."""
    for lo, size in segs[1:]:
        i = mem.find(first.encode(), lo, lo + size)
        if i >= 0:
            s = mem[i:mem.index(b'\0', i)].decode()
            return {k.strip(): int(v) for k, v in (e.split('=') for e in s.split(','))}
    return {}


# ---- RomFS tables (RSDB *.rstbl.byml.zs): the ids the profile is checked against
def byml_parse(b):
    """BYML v7 reader for the RSDB tables (arrays of dicts of scalars)."""
    if b[:2] != b'YB':
        raise SaveError('not a little-endian BYML')
    ko, so, ro = struct.unpack_from('<III', b, 4)

    def strtab(off):
        if not off:
            return []
        n = struct.unpack_from('<I', b, off)[0] >> 8
        offs = struct.unpack_from('<%dI' % (n + 1), b, off + 4)
        return [b[off + offs[i]:off + offs[i + 1]].split(b'\0')[0].decode('utf8', 'replace') for i in range(n)]

    keys, strs = strtab(ko), strtab(so)

    def val(t, v):
        if t == 0xA0:
            return strs[v]
        if t == 0xD0:
            return bool(v)
        if t == 0xD1:
            return struct.unpack('<i', struct.pack('<I', v))[0]
        if t == 0xD2:
            return struct.unpack('<f', struct.pack('<I', v))[0]
        if t == 0xD3:
            return v
        if t == 0xD4:
            return struct.unpack_from('<q', b, v)[0]
        if t == 0xD5:
            return struct.unpack_from('<Q', b, v)[0]
        if t == 0xD6:
            return struct.unpack_from('<d', b, v)[0]
        if t == 0xFF:
            return None
        if t in (0xC0, 0xC1):
            return node(v)
        return None

    def node(off):
        t, n = b[off], int.from_bytes(b[off + 1:off + 4], 'little')
        if t == 0xC0:
            ts = b[off + 4:off + 4 + n]
            base = off + 4 + ((n + 3) & ~3)
            return [val(ts[i], struct.unpack_from('<I', b, base + 4 * i)[0]) for i in range(n)]
        d = {}
        for i in range(n):
            e = off + 4 + 8 * i
            d[keys[int.from_bytes(b[e:e + 3], 'little')]] = val(b[e + 3], struct.unpack_from('<I', b, e + 4)[0])
        return d

    return node(ro)


def zstd_decompress(b):
    # The Python the install wizard sets up is 3.14, whose standard library decompresses zstd. A zstd program is
    # only the fallback of an older interpreter: a player's machine has none on PATH.
    try:
        from compression import zstd
    except ImportError:
        zstd = None
    if zstd is not None:
        try:
            return zstd.decompress(b)
        except zstd.ZstdError as e:
            raise SaveError('zstd: %s' % e)
    import subprocess
    exe = shutil.which('zstd') or r'C:\msys64\ucrt64\bin\zstd.exe'
    if not os.path.exists(exe):
        raise SaveError('zstd is needed to read the RomFS tables: use Python 3.14 or later, or put zstd.exe on PATH')
    r = subprocess.run([exe, '-d', '-c', '-q'], input=b, capture_output=True)
    if r.returncode:
        raise SaveError('zstd: %s' % r.stderr.decode(errors='replace'))
    return r.stdout


def romfs_reader(root):
    """A function path -> bytes over this project's romfs.bin + index.json, or an extracted RomFS directory."""
    if os.path.exists(os.path.join(root, 'index.json')):
        idx = {f['path']: f for f in json.load(open(os.path.join(root, 'index.json')))['files']}

        def get(p):
            f = idx[p]
            with open(os.path.join(root, 'romfs.bin'), 'rb') as fh:
                fh.seek(f['offset'])
                return fh.read(f['size'])
        get.paths = list(idx)
        return get

    def get(p):
        return open(os.path.join(root, p), 'rb').read()
    rsdb_dir = os.path.join(root, 'RSDB')
    get.paths = ['RSDB/' + n for n in os.listdir(rsdb_dir)] if os.path.isdir(rsdb_dir) else []
    return get


def rsdb(get, name):
    """One RSDB table by name. The build's name tag in the file name (`b20` in 11.2.0, `b30` in 11.3.0) is not assumed:
    it is whatever the RomFS holds, and a RomFS that holds two of the same table is refused."""
    prefix, suffix = 'RSDB/%s.Product.' % name, '.rstbl.byml.zs'
    hits = sorted(p for p in get.paths if p.startswith(prefix) and p.endswith(suffix) and '.' not in p[len(prefix):-len(suffix)])
    if not hits:
        raise SaveError('the RomFS has no table %s%s%s (another game version?)' % (prefix, '<tag>', suffix))
    if len(hits) > 1:
        raise SaveError('the RomFS has several tables for %s: %s' % (name, ', '.join(hits)))
    return byml_parse(zstd_decompress(get(hits[0])))


def read_tables(romfs, abilities):
    """The ids the profile accepts, from the game's own RSDB tables (kept in the local key file)."""
    get = romfs_reader(romfs)
    t = {}
    sel = lambda rows: [r for r in rows if r.get('Order', 0) >= 0 and 'Sdodr' not in r.get('__RowId', '')]
    hair = sel(rsdb(get, 'HairInfo'))
    t['hair'] = {'inkling': sorted(r['Id'] for r in hair if r['IsSquid']),
                 'octoling': sorted(r['Id'] for r in hair if not r['IsSquid'])}
    brows = sel(rsdb(get, 'EyebrowInfo'))
    t['eyebrows'] = {'inkling': sorted(r['Id'] for r in brows if r['IsSquid']),
                     'octoling': sorted(r['Id'] for r in brows if not r['IsSquid'])}
    t['legwear'] = {str(r['Id']): r.get('VariationNum', 0) for r in sel(rsdb(get, 'BottomInfo'))}
    for part, name in (('head', 'GearInfoHead'), ('clothes', 'GearInfoClothes'), ('shoes', 'GearInfoShoes')):
        t['gear_' + part] = {str(r['Id']): {'rarity': r['Rarity'], 'skill': abilities.get(r['Skill'], -1),
                                            'how': r['HowToGet']} for r in rsdb(get, name)}
    t['weapon'] = sorted(r['Id'] for r in rsdb(get, 'WeaponInfoMain') if r['Type'] == 'Versus')
    t['banner'] = sorted(r['Id'] for r in rsdb(get, 'NamePlateBgInfo'))
    t['adjective'] = sorted(r['Id'] for r in rsdb(get, 'BynameAdjectiveInfo'))
    t['subject'] = sorted(r['Id'] for r in rsdb(get, 'BynameSubjectInfo'))
    t['badge'] = sorted(r['Id'] for r in rsdb(get, 'BadgeInfo'))
    t['emote'] = sorted(r['Id'] for r in rsdb(get, 'EmoteInfo'))
    return t


def cmd_keys(a):
    path = a.main or os.path.join(a.exefs, 'main')
    mem, segs = load_nso(path)
    addr = find_table(mem, segs[0])
    table = struct.unpack_from('<32I', mem, addr)
    names = {}
    for lo, size in segs[1:]:
        for m in re.finditer(rb'[A-Za-z_][A-Za-z0-9_]{1,63}', mem[lo:lo + size]):
            s = m.group().decode()
            names.setdefault('%08x' % key_hash(s), s)
    abilities = find_enum(mem, segs, 'None = -1 , MainInk_Save = 0')
    doc = {'what': 's3save key table, field names and option ids, read from this machine\'s own Splatoon 3; keep local',
           'main': os.path.abspath(path), 'table_offset': '%#x' % addr,
           'table': ''.join('%08x' % v for v in table), 'abilities': abilities, 'names': names}
    if a.romfs:
        doc['tables'] = read_tables(a.romfs, abilities)
    if a.check:
        decrypt(table, open(a.check, 'rb').read())
    with open(a.out, 'w', encoding='utf8') as f:
        json.dump(doc, f)
    print('key table at main+%#x, %d names, %d abilities%s -> %s' %
          (addr, len(names), len(abilities), ', RomFS option tables' if a.romfs else '', a.out))


class Keys:
    def __init__(self, path):
        if not os.path.exists(path):
            raise SaveError('no key file %s: run `s3save.py keys --exefs <exefs dir> --romfs <romfs dir>` first' % path)
        d = json.load(open(path, encoding='utf8'))
        self.table = [int(d['table'][i:i + 8], 16) for i in range(0, 256, 8)]
        self.names = {int(k, 16): v for k, v in d.get('names', {}).items()}
        self.tables = d.get('tables')

    def name(self, k):
        return self.names.get(k, '#%08x' % k)

    def need_tables(self):
        if not self.tables:
            raise SaveError('the key file has no option tables: run `s3save.py keys` again with --romfs')
        return self.tables


# ---------------------------------------------------------------- paths
def _key_of(tok):
    if tok.startswith('#'):
        return int(tok[1:], 16)
    if re.fullmatch(r'-?\d+', tok):
        return int(tok)   # numeric map keys (weapon ids, gear ids)
    return key_hash(tok)


def resolve(plain, path, create=False):
    """path: <root>/<key or [i]>/...; returns (parent node, index or key node, node)."""
    toks = [t for t in re.split(r'[/.]', path) if t]
    if not toks or not toks[0].isdigit() or int(toks[0]) >= len(plain.roots):
        raise SaveError('path %s must start with a root index (0 or 1)' % path)
    node, parent, slot = plain.roots[int(toks[0])], None, None
    for tok in toks[1:]:
        m = re.fullmatch(r'\[(\d+)\]', tok)
        if m:
            if node.kind != 'arr':
                raise SaveError('%s: [%s] on a %s' % (path, m.group(1), node.kind))
            parent, slot = node, int(m.group(1))
            node = node.v[slot]
            continue
        if node.kind != 'map':
            raise SaveError('%s: %s on a %s' % (path, tok, node.kind))
        key = _key_of(tok)
        for pair in node.v:
            if pair[0].kind == 'int' and pair[0].v == key:
                parent, slot, node = node, pair, pair[1]
                break
        else:
            raise SaveError('%s: no field %s' % (path, tok))
    return parent, slot, node


def set_value(plain, path, value):
    parent, slot, node = resolve(plain, path)
    if node.kind == 'bool':
        if isinstance(value, str):
            value = {'true': True, '1': True, 'false': False, '0': False}[value.lower()]
        node.v = bool(value)
    elif node.kind == 'int':
        node.v = int(value)
    elif node.kind in ('f32', 'f64'):
        node.v = float(value)
    elif node.kind == 'str':
        b = value.encode('utf8') if isinstance(value, str) else bytes(value)
        node.v = b
    else:
        raise SaveError('%s is a %s; only scalars can be set' % (path, node.kind))
    return node


def fmt_scalar(n):
    if n.kind == 'str':
        return json.dumps(n.v.decode('utf8', 'replace'), ensure_ascii=False)
    if n.kind == 'bin':
        return 'bin:' + n.v.hex()
    return repr(n.v)


def dump_tree(keys, node, ind, out):
    if node.kind == 'map':
        for k, v in node.v:
            kn = keys.name(k.v) if k.kind == 'int' and k.fmt == 0xCE else fmt_scalar(k)
            if v.kind in ('map', 'arr'):
                out.append('%s%s: %s[%d]' % ('  ' * ind, kn, v.kind, len(v.v)))
                dump_tree(keys, v, ind + 1, out)
            else:
                out.append('%s%s = %s' % ('  ' * ind, kn, fmt_scalar(v)))
    else:
        for i, v in enumerate(node.v):
            if v.kind in ('map', 'arr'):
                out.append('%s[%d]: %s[%d]' % ('  ' * ind, i, v.kind, len(v.v)))
                dump_tree(keys, v, ind + 1, out)
            else:
                out.append('%s[%d] = %s' % ('  ' * ind, i, fmt_scalar(v)))


def to_json(keys, node):
    if node.kind == 'map':
        return {(keys.name(k.v) if k.kind == 'int' and k.fmt == 0xCE else str(k.v)): to_json(keys, v)
                for k, v in node.v}
    if node.kind == 'arr':
        return [to_json(keys, v) for v in node.v]
    if node.kind == 'str':
        return node.v.decode('utf8', 'replace')
    if node.kind == 'bin':
        return node.v.hex()
    return node.v


# ---------------------------------------------------------------- profile
# Model types: main's spl::PlayerModelType reflection string "SquidF , SquidM , OctopusF , OctopusM , Rival , ...".
STYLES = {'inkling_f': 0, 'inkling_m': 1, 'octoling_f': 2, 'octoling_m': 3}
# Skin tone and eye color have no RomFS table; the NPC rows of RSDB NpcInfo use SkinColor 0..8 and EyeColor 0..20,
# the only ranges the game's data shows (inferred, not read from the creation screen).
SKIN = list(range(0, 9))
EYES = list(range(0, 21))

# The one field that sends the boot to the plaza instead of the first-launch flow (character creation, tutorial):
# measured by experiment.
FIRST_LAUNCH_DONE = [('0/Plaza/IsDoneFirstBootSeq', True)]
# Gear the game drops from the ownership maps when it loads a save that lists them (measured with unlock_all;
# the reason is not known). Left out of unlock_all and refused as equipped gear.
GEAR_DROPPED = {'head': {3007}, 'clothes': {1030, 6003}, 'shoes': {2011, 3009, 3012}}

COORDS = '0/Common/Coordinates'              # what the game shows and plays with
LAST_COORDS = '1/LastPlayCoordinates'        # the player data's copy of the appearance
NAMEPLATES = ['0/NamePlateEditor/NamePlate', '1/LastPlayNamePlate']
GEAR_PARTS = {'head': ('GearHeadId', 'HaveGearHeadMap'), 'clothes': ('GearClothesId', 'HaveGearClothesMap'),
              'shoes': ('GearShoesId', 'HaveGearShoesMap')}


def validate_profile(p, tables):
    """-> list of errors. tables: the key file's option tables (None: appearance checked against built-in lists)."""
    errs = []
    style = p.get('style')
    if style not in STYLES:
        return ['style must be one of %s' % ', '.join(STYLES)]
    species = 'octoling' if style.startswith('octoling') else 'inkling'
    hair = tables['hair'][species] if tables else (list(range(16)) if species == 'inkling' else list(range(30, 38)))
    brows = tables['eyebrows'][species] if tables else ([0, 1, 2, 3] if species == 'inkling' else [20, 21, 22, 23])
    legwear = {int(k): v for k, v in tables['legwear'].items()} if tables else {i: 3 if i == 1 else 0 for i in range(9)}
    if p.get('hair') not in hair:
        errs.append('hair %r is not a %s hairstyle %s' % (p.get('hair'), species, hair))
    if p.get('eyebrows') not in brows:
        errs.append('eyebrows %r are not %s eyebrows %s' % (p.get('eyebrows'), species, brows))
    if p.get('legwear') not in legwear:
        errs.append('legwear must be one of %s' % sorted(legwear))
    else:
        var = p.get('legwear_variation', 0)
        if not isinstance(var, int) or not 0 <= var < max(1, legwear[p['legwear']]):
            errs.append('legwear_variation %r out of range for legwear %r' % (var, p['legwear']))
    if p.get('skin') not in SKIN:
        errs.append('skin must be one of %s' % SKIN)
    if p.get('eyes') not in EYES:
        errs.append('eyes must be one of %s' % EYES)
    name = p.get('name')
    if name is not None and (not isinstance(name, str) or not 1 <= len(name) <= 10 or len(name.encode()) > 32):
        errs.append('name must be 1 to 10 characters')
    extra = [k for k in ('splashtag', 'gear', 'weapon', 'emote', 'unlock_all') if p.get(k)]
    if extra and not tables:
        errs.append('%s need the option tables: run `s3save.py keys` with --romfs' % ', '.join(extra))
        return errs
    tag = p.get('splashtag') or {}
    for k, t in (('banner', 'banner'), ('adjective', 'adjective'), ('subject', 'subject')):
        if k in tag and tag[k] not in tables[t]:
            errs.append('splashtag.%s %r is not in the game\'s table' % (k, tag[k]))
    badges = tag.get('badges')
    if badges is not None and (not isinstance(badges, list) or len(badges) != 3 or
                               any(b != 0 and b not in tables['badge'] for b in badges)):
        errs.append('splashtag.badges must be 3 badge ids (0 = empty)')
    for part, gid in (p.get('gear') or {}).items():
        if part not in GEAR_PARTS:
            errs.append('gear.%s: the parts are head, clothes, shoes' % part)
        elif (str(gid) not in tables['gear_' + part] or tables['gear_' + part][str(gid)]['how'] == 'Impossible' or
              gid in GEAR_DROPPED[part]):
            errs.append('gear.%s %r is not a player gear id' % (part, gid))
    if p.get('weapon') is not None and p['weapon'] not in tables['weapon']:
        errs.append('weapon %r is not a Versus main weapon id' % p['weapon'])
    if p.get('emote') is not None and p['emote'] not in tables['emote']:
        errs.append('emote %r is not in the game\'s table' % p['emote'])
    return errs


def _int(v, fmt=None):
    return Node('int', v, fmt)


def _field(name):
    return Node('int', key_hash(name), 0xCE)   # the game writes field-name keys as uint32


def map_get(node, key):
    """The value of key (an int id or a field name) in a map node, or None."""
    k = key_hash(key) if isinstance(key, str) else key
    for kk, vv in node.v:
        if kk.kind == 'int' and kk.v == k:
            return vv
    return None


def clone(node):
    if node.kind == 'map':
        return Node('map', [[clone(k), clone(v)] for k, v in node.v], node.fmt)
    if node.kind == 'arr':
        return Node('arr', [clone(v) for v in node.v], node.fmt)
    return Node(node.kind, node.v, node.fmt)


def own(plain, path, item, make):
    """Adds item (an int id key, written with the smallest encoding as the game writes ids) to the ownership map at
    path with the value make(template), template being an existing entry of that map (or None). Returns 1 if added."""
    _, _, m = resolve(plain, path)
    if map_get(m, item) is not None:
        return 0
    tmpl = m.v[0][1] if m.v else None
    m.v.append([_int(item), make(tmpl)])
    return 1


def _map(fields):
    return Node('map', [[_field(k), v] for k, v in fields])


def weapon_record(tmpl):
    """The player data's record of an owned weapon: the starter weapon's record with its counters at zero."""
    if tmpl is not None:
        r = clone(tmpl)
        for _, v in r.v:
            v.v = 0.0 if v.kind in ('f32', 'f64') else 0 if v.kind == 'int' else v.v
        return r
    return _map([(k, _int(0)) for k in ('TotalPaintTubo', 'LastPlayDateTimeUtc', 'RegularWinPoint',
                                        'RegularWinHighGrade', 'Exp', 'Level', 'RewardAcceptLevel', 'WinCount')] +
                [('MaxWeaponPower', Node('f32', 0.0))])


def gear_record(info):
    """An owned gear as the game records the starter gear: Rarity = the table's rarity, MainSkill = the table's main
    ability as the ability enum's value, one empty (-1) sub-ability slot per rarity step plus one."""
    def make(tmpl):
        fields = [('Exp', _int(0)), ('TotalExp', _int(0)), ('Rarity', _int(info['rarity'])),
                  ('MainSkill', _int(info['skill'])),
                  ('ExSkillArray', Node('arr', [_int(-1) for _ in range(info['rarity'] + 1)])),
                  ('LastPlayDateTimeUtc', _int(0)), ('RandomContext', _int(0))]
        if tmpl is None:
            return _map(fields)
        r = clone(tmpl)   # the game's field order and encodings
        vals = {key_hash(n): v for n, v in fields}
        for pair in r.v:
            if pair[0].v in vals:
                pair[1] = vals[pair[0].v]
        return r
    return make


def seen_record(now):
    """Root 0's Customize record of an owned item: when it was got, and that its "new" mark was seen."""
    def make(tmpl):
        if tmpl is not None:
            r = clone(tmpl)
            map_get(r, 'DateTimeUtc').v = now
            map_get(r, 'IsLookedCustomize').v = True
            return r
        return _map([('DateTimeUtc', _int(now)), ('IsLookedCustomize', Node('bool', True))])
    return make


def banner_record(tmpl):
    if tmpl is not None:
        r = clone(tmpl)
        for _, v in r.v:
            v.v = 0
        return r
    return _map([('AcquiredDateTimeUtc', _int(0))])


def apply_profile(plain, p, tables, skip=True, now=None):
    """Writes the profile into the save. Returns the list of changes."""
    import time
    now = int(time.time()) if now is None else now
    changes = []

    def put(path, value):
        _, _, node = resolve(plain, path)
        if node.v != value:
            changes.append('%s: %r -> %r' % (path, node.v, value))
            set_value(plain, path, value)

    def add(path, item, make, what):
        if own(plain, path, item, make):
            changes.append('%s: + %s %d' % (path, what, item))

    for base in (COORDS, LAST_COORDS):
        put(base + '/ModelType', STYLES[p['style']])
        put(base + '/HairId', p['hair'])
        put(base + '/Eyebrow', p['eyebrows'])
        put(base + '/BottomId', p['legwear'])
        put(base + '/BottomVariation', p.get('legwear_variation', 0))
        put(base + '/SkinColor', p['skin'])
        put(base + '/EyeColor', p['eyes'])
    if p.get('name'):
        put('1/UserName', p['name'])
    if skip:
        for path, value in FIRST_LAUNCH_DONE:
            put(path, value)
    if not tables:
        return changes

    bool_true = lambda tmpl: Node('bool', True)
    unlock = p.get('unlock_all')
    weapons = set(tables['weapon']) if unlock else set()
    gear = {part: set(int(k) for k, v in tables['gear_' + part].items()
                      if v['how'] != 'Impossible' and int(k) not in GEAR_DROPPED[part]) if unlock
            else set() for part in GEAR_PARTS}
    banners = set(tables['banner']) if unlock else set()
    adjectives = set(tables['adjective']) if unlock else set()
    subjects = set(tables['subject']) if unlock else set()
    emotes = set(tables['emote']) if unlock else set()

    if p.get('weapon') is not None:
        weapons.add(p['weapon'])
        put(COORDS + '/WeaponId', p['weapon'])
    for part, gid in (p.get('gear') or {}).items():
        gear[part].add(gid)
        put(COORDS + '/' + GEAR_PARTS[part][0], gid)
        put(COORDS + '/' + GEAR_PARTS[part][0].replace('Id', 'VariationId'), 0)
    if p.get('emote') is not None:
        emotes.add(p['emote'])
        put(COORDS + '/WinEmote', p['emote'])
    tag = p.get('splashtag') or {}
    if 'banner' in tag:
        banners.add(tag['banner'])
    if 'adjective' in tag:
        adjectives.add(tag['adjective'])
    if 'subject' in tag:
        subjects.add(tag['subject'])
    for base in NAMEPLATES:
        if 'banner' in tag:
            put(base + '/Background', tag['banner'])
        if 'adjective' in tag:
            put(base + '/Byname/Adjective', tag['adjective'])
        if 'subject' in tag:
            put(base + '/Byname/Subject', tag['subject'])
        if 'title_gender' in tag:
            put(base + '/Byname/Gender', tag['title_gender'])
    badges = tag.get('badges')
    if badges:
        owned = set()
        for path in ('0/Common/HaveNamePlateBadgeMap', '1/LastPlayHaveBadgeMap'):
            owned |= {k.v for k, _ in resolve(plain, path)[2].v}
        missing = [b for b in badges if b and b not in owned]
        if missing:
            raise SaveError('badges %s are not owned in this save, and how the game records a badge is not known '
                            '(no save with a badge has been seen yet)' % missing)
        for base in NAMEPLATES:
            for i, b in enumerate(badges):
                put('%s/Badge%d' % (base, i + 1), b)

    for w in sorted(weapons):
        add('1/HaveWeaponMap', w, weapon_record, 'weapon')
        add('0/Customize/HaveWeaponMap', w, seen_record(now), 'weapon')
    for part, ids in gear.items():
        mapname = GEAR_PARTS[part][1]
        for g in sorted(ids):
            add('1/' + mapname, g, gear_record(tables['gear_' + part][str(g)]), part)
            add('0/Customize/' + mapname, g, seen_record(now), part)
    for b in sorted(banners):
        add('1/HaveNamePlateBgMap', b, banner_record, 'banner')
    for a in sorted(adjectives):
        add('1/HaveBynameAdjectiveMap', a, bool_true, 'title adjective')
    for s in sorted(subjects):
        add('1/HaveBynameSubjectMap', s, bool_true, 'title subject')
    for e in sorted(emotes):
        add('1/HaveEmoteMap', e, bool_true, 'emote')
        add('0/Customize/HaveEmoteMap', e, seen_record(now), 'emote')
    return changes


# ---------------------------------------------------------------- commands
def load_plain(keys, path):
    return Plain(decrypt(keys.table, open(path, 'rb').read()))


def backup(path):
    stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S')
    dst = '%s.%s.bak' % (path, stamp)
    shutil.copy2(path, dst)
    return dst


def write_save(keys, plain, path, make_backup=True):
    data = encrypt(keys.table, plain.encode())
    decrypt(keys.table, data)   # the check the game makes
    if make_backup and os.path.exists(path):
        print('backup:', backup(path))
    tmp = path + '.tmp'
    with open(tmp, 'wb') as f:
        f.write(data)
    os.replace(tmp, path)


def cmd_check(a):
    k = Keys(a.keys)
    p = load_plain(k, a.save)
    print('%s: checksum ok, game version %s, %d MessagePack roots, %d bytes of plaintext' %
          (a.save, p.game_version, len(p.roots), len(p.encode())))


def cmd_decrypt(a):
    k = Keys(a.keys)
    open(a.out, 'wb').write(decrypt(k.table, open(a.save, 'rb').read()))


def cmd_encrypt(a):
    k = Keys(a.keys)
    plain = open(a.plain, 'rb').read()
    Plain(plain)   # parses
    data = encrypt(k.table, plain, a.seed)
    if os.path.exists(a.out):
        print('backup:', backup(a.out))
    open(a.out, 'wb').write(data)


def cmd_dump(a):
    k = Keys(a.keys)
    p = load_plain(k, a.save)
    if a.json:
        text = json.dumps({'header': p.header.hex(), 'roots': [to_json(k, r) for r in p.roots]}, ensure_ascii=False,
                          indent=1)
    else:
        lines = ['# game version %s' % p.game_version]
        for i, r in enumerate(p.roots):
            lines.append('=== root %d' % i)
            dump_tree(k, r, 1, lines)
        text = '\n'.join(lines) + '\n'
    if a.out:
        open(a.out, 'w', encoding='utf8').write(text)
    else:
        sys.stdout.write(text)


def cmd_get(a):
    k = Keys(a.keys)
    p = load_plain(k, a.save)
    for path in a.paths:
        _, _, n = resolve(p, path)
        if n.kind in ('map', 'arr'):
            out = []
            dump_tree(k, n, 1, out)
            print('%s: %s[%d]\n%s' % (path, n.kind, len(n.v), '\n'.join(out)))
        else:
            print('%s = %s' % (path, fmt_scalar(n)))


def cmd_set(a):
    k = Keys(a.keys)
    p = load_plain(k, a.save)
    for item in a.assignments:
        path, value = item.split('=', 1)
        _, _, before = resolve(p, path)
        old = fmt_scalar(before)
        n = set_value(p, path, value)
        print('%s: %s -> %s' % (path, old, fmt_scalar(n)))
    write_save(k, p, a.out or a.save, make_backup=not a.no_backup)


def cmd_profile(a):
    k = Keys(a.keys)
    prof = json.load(open(a.profile, encoding='utf-8-sig'))
    errs = validate_profile(prof, k.tables)
    if errs:
        raise SaveError('profile %s: %s' % (a.profile, '; '.join(errs)))
    src = a.save if os.path.exists(a.save) else a.template
    if not src or not os.path.exists(src):
        raise SaveError('%s does not exist and no --template save was given to create it from' % a.save)
    p = load_plain(k, src)
    skip = prof.get('skip_first_launch', True) and not a.keep_first_launch
    changes = apply_profile(p, prof, k.tables, skip=skip)
    for c in changes:
        print(' ', c)
    if not changes and src == a.save:
        print('nothing to change')
        return
    write_save(k, p, a.save)
    print('%s %s' % ('updated' if src == a.save else 'created from %s:' % src, a.save))


def cmd_catalogue(a):
    """The character-creation options for a launcher: ids, internal labels, the style each belongs to."""
    get = romfs_reader(a.romfs)
    sel = lambda rows: [r for r in rows if r.get('Order', 0) >= 0 and 'Sdodr' not in r.get('__RowId', '')]
    order = lambda rows: sorted(rows, key=lambda r: (r.get('Order', 0), r['Id']))
    doc = {
        'what': 'Splatoon 3 character-creation options (RomFS RSDB tables, read from the player\'s own game); '
                'ids as profile.json takes them',
        'styles': [{'id': v, 'key': k, 'species': k.split('_')[0], 'body': k.split('_')[1].upper(),
                    'model_type_name': ['SquidF', 'SquidM', 'OctopusF', 'OctopusM'][v]} for k, v in STYLES.items()],
        'hair': [{'id': r['Id'], 'row': r['__RowId'], 'label_ja': r.get('Label', ''),
                  'species': 'inkling' if r['IsSquid'] else 'octoling', 'order': r['Order']}
                 for r in order(sel(rsdb(get, 'HairInfo')))],
        'eyebrows': [{'id': r['Id'], 'row': r['__RowId'], 'label_ja': r.get('Label', ''),
                      'species': 'inkling' if r['IsSquid'] else 'octoling', 'order': r['Order']}
                     for r in order(sel(rsdb(get, 'EyebrowInfo')))],
        'legwear': [{'id': r['Id'], 'row': r['__RowId'], 'label_ja': r.get('Label', ''),
                     'variations': max(1, r.get('VariationNum', 0)), 'order': r['Order'], 'species': 'any'}
                    for r in order(sel(rsdb(get, 'BottomInfo')))],
        'skin': {'ids': SKIN, 'species': 'any', 'source': 'inferred: the SkinColor values of RSDB NpcInfo rows; '
                 'no table lists the player palette'},
        'eyes': {'ids': EYES, 'species': 'any', 'source': 'inferred: the EyeColor values of RSDB NpcInfo rows; '
                 'no table lists the player palette'},
        'rules': ['hair and eyebrows must match the style\'s species (inkling: IsSquid rows, octoling: the others)',
                  'legwear_variation < variations'],
    }
    with open(a.out, 'w', encoding='utf8') as f:
        json.dump(doc, f, ensure_ascii=False, indent=1)
    print('%s: %d hairstyles, %d eyebrows, %d legwear' % (a.out, len(doc['hair']), len(doc['eyebrows']),
                                                           len(doc['legwear'])))


def cmd_selftest(a):
    k = Keys(a.keys)
    data = open(a.save, 'rb').read()
    plain = decrypt(k.table, data)
    p = Plain(plain)
    if p.encode() != plain:
        raise SaveError('MessagePack re-encoding differs from the plaintext')
    seed = struct.unpack_from('<I', data, 0)[0]
    if encrypt(k.table, plain, seed) != data:
        raise SaveError('re-encryption with the same seed differs from the file')
    print('%s: plaintext re-encodes identically and re-encrypts to the same bytes (%d bytes)' % (a.save, len(data)))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--keys', default=DEFAULT_KEYS, help='key file (default %(default)s)')
    sub = ap.add_subparsers(dest='cmd', required=True)
    s = sub.add_parser('keys', help='extract the key table and field names from the game\'s main')
    g = s.add_mutually_exclusive_group(required=True)
    g.add_argument('--exefs', help='ExeFS directory holding main')
    g.add_argument('--main', help='the main NSO')
    s.add_argument('--romfs', help='RomFS (romfs.bin + index.json, or an extracted directory): option ids to check '
                                   'profiles against')
    s.add_argument('--check', metavar='SAVE', help='also decrypt this save as a test')
    s.add_argument('--out', default=DEFAULT_KEYS)
    s.set_defaults(fn=cmd_keys)
    s = sub.add_parser('check', help='verify and parse a save')
    s.add_argument('save')
    s.set_defaults(fn=cmd_check)
    s = sub.add_parser('decrypt', help='save.dat -> plaintext')
    s.add_argument('save')
    s.add_argument('out')
    s.set_defaults(fn=cmd_decrypt)
    s = sub.add_parser('encrypt', help='plaintext -> save.dat')
    s.add_argument('plain')
    s.add_argument('out')
    s.add_argument('--seed', type=lambda x: int(x, 0))
    s.set_defaults(fn=cmd_encrypt)
    s = sub.add_parser('dump', help='readable tree of a save')
    s.add_argument('save')
    s.add_argument('--json', action='store_true')
    s.add_argument('--out')
    s.set_defaults(fn=cmd_dump)
    s = sub.add_parser('get', help='print fields')
    s.add_argument('save')
    s.add_argument('paths', nargs='+')
    s.set_defaults(fn=cmd_get)
    s = sub.add_parser('set', help='change fields: PATH=VALUE ...')
    s.add_argument('save')
    s.add_argument('assignments', nargs='+')
    s.add_argument('--out', help='write here instead of in place')
    s.add_argument('--no-backup', action='store_true')
    s.set_defaults(fn=cmd_set)
    s = sub.add_parser('profile', help='apply a launcher profile.json to a save (backup first)')
    s.add_argument('save')
    s.add_argument('--profile', required=True)
    s.add_argument('--template', help='a save to start from when SAVE does not exist yet')
    s.add_argument('--keep-first-launch', action='store_true', help='appearance only; leave the first-launch fields')
    s.set_defaults(fn=cmd_profile)
    s = sub.add_parser('catalogue', help='the character-creation options as JSON for a launcher')
    s.add_argument('--romfs', required=True)
    s.add_argument('--out', required=True)
    s.set_defaults(fn=cmd_catalogue)
    s = sub.add_parser('selftest', help='round trip of a save')
    s.add_argument('save')
    s.set_defaults(fn=cmd_selftest)
    a = ap.parse_args(argv)
    try:
        a.fn(a)
    except SaveError as e:
        sys.exit('s3save: %s' % e)


if __name__ == '__main__':
    main()
