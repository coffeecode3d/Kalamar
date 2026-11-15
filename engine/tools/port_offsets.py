"""Carry module offsets from one build of a game to the next by matching the two modules' code.

Usage: port_offsets.py --old <exefs of the known build> --new <exefs of the new build> [--module main]
           [--code <hex>[,<hex>...]] [--data <hex>[,...]] [--string <hex>[,...]] [--patches <patch table>]
           [--out-patches <file>] [--json <file>]

How offsets are matched (nothing is guessed; an offset that fails a step is reported as unmatched):

- The text of both builds is read as 32-bit words and normalised: the position-dependent fields are masked out (the
  target of b, bl, b.cond, cbz, cbnz, tbz, tbnz, adr, adrp and of a literal load, and the low 12 bits an add or a
  load/store takes from an adrp a few instructions before it). Every other bit, register numbers and structure offsets included,
  stays, so two normalised words are equal only when the instructions differ by where their targets moved.
- Anchors: runs of GRAM normalised words that occur exactly once in the old text and exactly once in the new text.
  An anchor fixes the shift (new offset - old offset) of its words.
- Every other old word takes the shift of the nearest anchor before it, else of the nearest after it, when the
  normalised word at the shifted place is equal. The words around an offset that carry the same shift without a gap
  are its run.
- --code: an instruction or function entry. Certain when its run holds at least MIN_RUN words and an anchor, and,
  when the old offset is the target of bl instructions, every matched caller calls the new offset in the new build
  (the call graph agrees). The report gives the run, the callers that agree and those that do not.
- --data: an address in the data or bss segment. Every adrp+add, adrp+load/store pair of the old text that forms the
  address is carried over as code and the pair at the new place is decoded: certain when at least one pair was
  carried and all of them give one address.
- --string: an address in the read-only data. As --data, and the NUL-terminated bytes at the old address must be
  those at the new one (the string is compared, never printed).
- --patches: the `spin <module> <head> <back-edge>...` lines of a patch table (recomp/patches.py) for this module are
  carried as --code offsets; --out-patches writes the carried lines. tools/spin_waits.py on the new build is the
  source of a new table; this is the cross-check that both give the same sites.

The report holds offsets and counts only, no bytes of either module.
"""
import argparse
import json
import re
import sys
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'recomp'))

from nso import Nso  # noqa: E402

GRAM = 12      # words of an anchor
MIN_RUN = 24   # words of a run that makes a match certain
ADRP_WIN = 32  # words between an adrp and the instructions that take its low 12 bits


def text_words(nso):
    off, size = nso.text
    return np.frombuffer(bytes(nso.image[off:off + size]), dtype='<u4').copy(), off


def normalise(w):
    n = w.copy()
    for mask, value, keep in ((0x7C000000, 0x14000000, 0xFC000000),    # b, bl
                              (0xFF000010, 0x54000000, 0xFF00001F),    # b.cond
                              (0x7E000000, 0x34000000, 0xFF00001F),    # cbz, cbnz
                              (0x7E000000, 0x36000000, 0xFFF8001F),    # tbz, tbnz
                              (0x1F000000, 0x10000000, 0x9F00001F),    # adr, adrp
                              (0x3B000000, 0x18000000, 0xFF00001F)):   # load literal
        sel = (w & mask) == value
        n[sel] = w[sel] & keep
    # add #lo12 and load/store [x, #lo12] on a register an adrp set at most ADRP_WIN words before, with no word in
    # between whose low five bits (the destination of most instructions) name that register
    adrp = (w & 0x9F000000) == 0x90000000
    low5 = w & 0x1F
    rn = (w >> 5) & 0x1F
    idx = np.arange(len(w), dtype=np.int64)
    uses = ((w & 0xFFC00000) == 0x91000000) | ((w & 0x3B000000) == 0x39000000)
    pair = np.zeros(len(w), dtype=bool)
    for r in range(31):
        named = low5 == r
        set_at = np.maximum.accumulate(np.where(adrp & named, idx, -1))
        other_at = np.maximum.accumulate(np.where(~adrp & named, idx, -1))
        before_set = np.concatenate(([-1], set_at[:-1]))
        before_other = np.concatenate(([-1], other_at[:-1]))
        pair |= uses & (rn == r) & (before_set > before_other) & (idx - before_set <= ADRP_WIN)
    n[pair] = w[pair] & 0xFFC003FF
    return n


def gram_hash(n):
    h = np.zeros(len(n) - GRAM + 1, dtype=np.uint64)
    for k in range(GRAM):
        h = h * np.uint64(0x9E3779B97F4A7C15) + n[k:len(n) - GRAM + 1 + k].astype(np.uint64) + np.uint64(k + 1)
    return h


def unique_positions(h):
    order = np.argsort(h, kind='stable')
    s = h[order]
    first = np.ones(len(s), dtype=bool)
    first[1:] = s[1:] != s[:-1]
    last = np.ones(len(s), dtype=bool)
    last[:-1] = s[1:] != s[:-1]
    one = first & last
    return s[one], order[one]


class Match:
    """shift[i]: new word index - old word index of old word i, or NONE; anchored[i]: i lies in an anchor."""
    NONE = np.int64(-1 << 62)

    def __init__(self, old, new):
        self.old, self.new = old, new
        self.a, self.a_off = text_words(old)
        self.b, self.b_off = text_words(new)
        self.na, self.nb = normalise(self.a), normalise(self.b)
        ha, pa = unique_positions(gram_hash(self.na))
        hb, pb = unique_positions(gram_hash(self.nb))
        common, ia, ib = np.intersect1d(ha, hb, assume_unique=True, return_indices=True)
        ai, bi = pa[ia].astype(np.int64), pb[ib].astype(np.int64)
        ok = np.ones(len(ai), dtype=bool)
        for k in range(GRAM):   # a hash is not proof: compare the words
            ok &= self.na[ai + k] == self.nb[bi + k]
        ai, bi = ai[ok], bi[ok]
        n = len(self.a)
        shift = np.full(n, self.NONE, dtype=np.int64)
        anchored = np.zeros(n, dtype=bool)
        for k in range(GRAM):
            shift[ai + k] = bi - ai
            anchored[ai + k] = True
        self.anchors = len(ai)
        idx = np.arange(n, dtype=np.int64)
        known = shift != self.NONE
        # nearest anchored word before, then after
        for direction in (1, -1):
            if direction == 1:
                src = np.maximum.accumulate(np.where(known, idx, -1))
            else:
                src = np.minimum.accumulate(np.where(known, idx, n)[::-1])[::-1]
            valid = (src >= 0) & (src < n) & (shift == self.NONE)
            cand = np.where(valid, shift[np.clip(src, 0, n - 1)], 0)
            j = idx + cand
            inside = valid & (j >= 0) & (j < len(self.b))
            same = np.zeros(n, dtype=bool)
            same[inside] = self.na[inside] == self.nb[j[inside]]
            shift[same] = cand[same]
        self.shift, self.anchored = shift, anchored
        self.matched = int((shift != self.NONE).sum())

    def run(self, i):
        """(first, last, has an anchor) of the words around i with i's shift and no gap, or None."""
        d = self.shift[i]
        if d == self.NONE:
            return None
        lo = hi = i
        while lo > 0 and self.shift[lo - 1] == d:
            lo -= 1
        while hi + 1 < len(self.shift) and self.shift[hi + 1] == d:
            hi += 1
        return lo, hi, bool(self.anchored[lo:hi + 1].any())

    def carry(self, off):
        """New module offset of old module offset off (text), or None."""
        i = (off - self.a_off) // 4
        if off % 4 or not 0 <= i < len(self.a) or self.shift[i] == self.NONE:
            return None
        return int(self.b_off + 4 * (i + self.shift[i]))

    def bl_targets(self, w, base):
        sel = np.nonzero((w & 0xFC000000) == 0x94000000)[0]
        imm = (w[sel] & 0x3FFFFFF).astype(np.int64)
        imm = np.where(imm & 0x2000000, imm - 0x4000000, imm)
        return sel, base + 4 * (sel + imm)

    def callers(self):
        if not hasattr(self, '_calls'):
            sa, ta = self.bl_targets(self.a, self.a_off)
            order = np.argsort(ta, kind='stable')
            self._calls = (sa[order], ta[order])
            sb, tb = self.bl_targets(self.b, self.b_off)
            self._new_target = dict(zip((self.b_off + 4 * sb).tolist(), tb.tolist()))
        return self._calls

    def code(self, off):
        r = {'old': off, 'kind': 'code', 'new': None, 'certain': False, 'how': ''}
        i = (off - self.a_off) // 4
        if off % 4 or not 0 <= i < len(self.a):
            r['how'] = 'not an instruction of the text'
            return r
        run = self.run(i)
        if not run:
            r['how'] = 'the instruction has no equal at the shift of its neighbours'
            return r
        lo, hi, anchored = run
        new = self.carry(off)
        sites, targets = self.callers()
        first, last = np.searchsorted(targets, off, 'left'), np.searchsorted(targets, off, 'right')
        agree = differ = unmatched = 0
        for s in sites[first:last].tolist():
            c = self.carry(self.a_off + 4 * s)
            if c is None:
                unmatched += 1
            elif self._new_target.get(c) == new:
                agree += 1
            else:
                differ += 1
        r['new'] = new
        r['run'] = [int(self.a_off + 4 * lo), int(self.a_off + 4 * hi + 4)]
        r['callers'] = {'agree': agree, 'differ': differ, 'unmatched': unmatched}
        r['certain'] = bool(anchored and hi - lo + 1 >= MIN_RUN and differ == 0)
        r['how'] = (f'run of {hi - lo + 1} equal normalised instructions at shift {4 * int(self.shift[i]):+#x}'
                    f'{" with a unique anchor" if anchored else " without an anchor"}; '
                    f'bl callers: {agree} agree, {differ} differ, {unmatched} not carried')
        return r

    def pairs(self, w, base):
        """Old or new text: {address: [offsets of the adrp of an adrp+add or adrp+load/store pair forming it]}"""
        adrp = np.nonzero((w[:-1] & 0x9F000000) == 0x90000000)[0]
        nxt = w[adrp + 1]
        rd = w[adrp] & 0x1F
        same = ((nxt >> 5) & 0x1F) == rd
        add = same & ((nxt & 0xFFC00000) == 0x91000000)
        mem = same & ((nxt & 0x3B000000) == 0x39000000)
        immhi = ((w[adrp] >> 5) & 0x7FFFF).astype(np.int64)
        immlo = ((w[adrp] >> 29) & 3).astype(np.int64)
        page = (immhi << 2) | immlo
        page = np.where(page & 0x100000, page - 0x200000, page)
        pc = (base + 4 * adrp.astype(np.int64)) & ~np.int64(0xFFF)
        imm12 = ((nxt >> 10) & 0xFFF).astype(np.int64)
        scale = np.where(mem, np.where((nxt & 0x04800000) == 0x04800000, 4, (nxt >> 30) & 3), 0).astype(np.int64)
        addr = pc + (page << 12) + (imm12 << scale)
        sel = add | mem
        return base + 4 * adrp[sel].astype(np.int64), addr[sel]

    def data(self, addr, string=False):
        r = {'old': addr, 'kind': 'string' if string else 'data', 'new': None, 'certain': False, 'how': ''}
        if not hasattr(self, '_pa'):
            self._pa = self.pairs(self.a, self.a_off)
            sb, ab = self.pairs(self.b, self.b_off)
            self._pb = dict(zip(sb.tolist(), ab.tolist()))
        sites, addrs = self._pa
        votes = {}
        lost = 0
        for s in sites[addrs == addr].tolist():
            c = self.carry(s)
            got = self._pb.get(c) if c is not None and self.carry(s + 4) == c + 4 else None
            if got is None:
                lost += 1
            else:
                votes[got] = votes.get(got, 0) + 1
        if not votes:
            r['how'] = f'no carried adrp pair forms the address ({lost} pairs not carried)'
            return r
        best = max(votes, key=votes.get)
        r['new'] = best
        r['votes'] = {hex(k): v for k, v in sorted(votes.items())}
        r['certain'] = len(votes) == 1
        r['how'] = (f'{votes[best]} carried adrp pairs form it' +
                    (f', {sum(votes.values()) - votes[best]} form other addresses' if len(votes) > 1 else '') +
                    f', {lost} pairs not carried')
        if string:
            def cstr(nso, a):
                e = nso.image.find(b'\0', a)
                return bytes(nso.image[a:e])
            same = cstr(self.old, addr) == cstr(self.new, best)
            r['certain'] = bool(r['certain'] and same)
            r['how'] += '; the bytes up to the NUL are ' + ('equal' if same else 'DIFFERENT')
        return r


def hexes(v):
    return [int(x, 16) for x in v.split(',') if x.strip()] if v else []


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--old', required=True)
    ap.add_argument('--new', required=True)
    ap.add_argument('--module', default='main')
    ap.add_argument('--code')
    ap.add_argument('--data')
    ap.add_argument('--string')
    ap.add_argument('--patches')
    ap.add_argument('--out-patches')
    ap.add_argument('--json')
    args = ap.parse_args()
    m = Match(Nso(str(Path(args.old) / args.module)), Nso(str(Path(args.new) / args.module)))
    print(f'{args.module}: old text {len(m.a) * 4:#x} bytes, new text {len(m.b) * 4:#x} bytes, {m.anchors} anchors, '
          f'{m.matched} of {len(m.a)} old instructions carried ({100.0 * m.matched / len(m.a):.3f} %)')
    shifts, counts = np.unique(m.shift[m.shift != m.NONE], return_counts=True)
    order = np.argsort(-counts)[:12]
    print('shifts (bytes: instructions): ' + ', '.join(f'{4 * int(shifts[k]):+#x}: {int(counts[k])}' for k in order) +
          (f' and {len(shifts) - 12} more' if len(shifts) > 12 else ''))
    rows = [m.code(o) for o in hexes(args.code)] + [m.data(o) for o in hexes(args.data)] + \
           [m.data(o, True) for o in hexes(args.string)]
    for r in rows:
        print(f'{r["kind"]:6} {r["old"]:#x} -> {("%#x" % r["new"]) if r["new"] is not None else "?"}  '
              f'{"certain" if r["certain"] else "NOT CERTAIN"}  ({r["how"]})')
    if args.patches:
        out, total, sure, bad = [], 0, 0, []
        pat = re.compile(r'^spin\s+(\S+)\s+([0-9a-fA-F ]+?)\s*(#.*)?$')
        for line in Path(args.patches).read_text(encoding='utf-8').splitlines():
            g = pat.match(line)
            if not g or g.group(1) != args.module:
                continue
            total += 1
            got = [m.code(int(x, 16)) for x in g.group(2).split()]
            # one loop: every site carried at one shift, and at least one of them certain by itself
            if all(x['new'] is not None and x['new'] - x['old'] == got[0]['new'] - got[0]['old'] for x in got) and \
                    any(x['certain'] for x in got):
                sure += 1
                out.append(f'spin {args.module} ' + ' '.join(f'{x["new"]:x}' for x in got))
            else:
                bad.append(g.group(2))
            rows += got
        print(f'patch table: {total} spin lines of {args.module}, {sure} carried with certainty, {len(bad)} not')
        for b in bad:
            print(f'  not carried: spin {args.module} {b}')
        if args.out_patches:
            Path(args.out_patches).write_text('\n'.join(out) + '\n', encoding='utf-8')
    if args.json:
        Path(args.json).write_text(json.dumps(rows, indent=1), encoding='utf-8')


if __name__ == '__main__':
    main()
