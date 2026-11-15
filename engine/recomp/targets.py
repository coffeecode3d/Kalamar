"""Game builds the generator knows: which ExeFS modules to lift and how to recognise the dump they come from.

A target names one build of one game. `gen.py --target <name>` lifts the target's modules, stops before lifting when
an ExeFS code module's SHA-256 or the program id in main.npdm differs from the target's, takes the target's patch table
unless --patches names one, and writes target.json next to layout.json so later steps know what they build.

TotK targets read their hashes from tools/data/expected-v<display version>.json, the files tools/extract.py and
tools/build_game.py use; other games carry theirs here. main.npdm is checked by program id only: dumps that a
repacking tool re-signed differ in its bytes (tools/extract.py exefs_problems).

tools/build_game.py writes the target into the executable (modules.c, rt_target in runtime/host.h): the game family,
the program id, whether the sdk module is SDK 20.5's, whether the game links libmovie, and the boot checks of --boot
(runtime/boot.c): the return addresses of nnMain's SetFocusHandlingMode call and of the first nvnBootstrapLoader call,
and the init_array size of every module. A boot value of None is not checked; the boot log prints what it saw.
"""
import hashlib
import json
import struct
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class TargetError(ValueError):
    pass


@dataclass(frozen=True)
class Target:
    name: str                 # gen.py --target, and the version name of the build tools
    game: str
    title_id: str             # program id in main.npdm, 16 upper-case hex digits
    display_version: str
    modules: tuple            # ExeFS modules to lift (recomp/layout.py places them)
    exefs_sha256: dict = field(default_factory=dict)   # ExeFS code file -> SHA-256 it must have
    patches: str | None = None   # code patch table relative to the repository (recomp/patches.py), or None
    family: str = 'totk'         # the runtime's game family: totk or splatoon3 (runtime/host.h RT_GAME_*)
    sdk_20_5: bool = False       # sdk is the SDK 20.5 module of TotK 1.4.3: its structure layouts (runtime/swkbd.c)
    movie: bool = True           # the game links libmovie: runtime/ov_movie.c's movie::Decoder overrides apply
    boot_checkpoint: int | None = None   # main offset of the return address of nnMain's SetFocusHandlingMode call
    boot_nvn: int | None = None          # main offset of the return address of the first nvnBootstrapLoader call
    init_arrays: dict = field(default_factory=dict)   # module -> init_array entries its DT_INIT runs
    exe: str = 'totk'            # the executable tools/build_game.py links (.exe on Windows)


TOTK_TITLE_ID = '0100F2C0115B6000'
TOTK_MODULES = ('main', 'subsdk0', 'sdk')
# name: (display version, patch table, boot checks: (SetFocusHandlingMode return, first nvnBootstrapLoader return,
# init_array entries of main, subsdk0, sdk) or None). 1.1.0 and 1.2.1: this port's analysis of main; 1.4.3: MacNativeGames'
# scripts/verify_boot.py and DT_INIT_ARRAYSZ.
TOTK_VERSIONS = {
    'v0': ('1.0.0', None, None),
    'v110': ('1.1.0', 'recomp/patches-v110.txt', (0xeb8c14, 0xebcdec, (10668, 45, 116))),
    'v112': ('1.1.2', None, None),
    'v121': ('1.2.1', 'recomp/patches-v121.txt', (0xe7a954, 0xe7eb2c, (10673, 45, 116))),
    'v143': ('1.4.3', 'recomp/patches-v143.txt', (0xbdb48c, 0xbe0360, (10717, 45, 83))),
}

# Splatoon 3 11.2.0 (SDK 20.5): no subsdk modules and no libmovie. Its sdk and rtld are byte for byte the ones of TotK
# 1.4.3. The boot values were read from this port's first boots (runtime/boot.c logs them): the first
# nvnBootstrapLoader call is the bl at main+0x452917c. nnMain calls SetFocusHandlingMode only after it (return address
# main+0x15c6ec4), so that call is no checkpoint of --boot here.
SPLATOON3 = {
    'splatoon3-v1120': Target(
        name='splatoon3-v1120', game='Splatoon 3', title_id='0100C2500FC20000', display_version='11.2.0',
        modules=('main', 'sdk'),
        exefs_sha256={
            'main': 'e20596a523990dd8b31dc452c7d9baa89b409927ec10ed9b956806774521629c',
            'sdk': '1d187f29784cd0af46e9e123fc2a62c77b3b07c5a39c8633d90d6c80de64e4ad',
            'rtld': '49308064fddb52dfe9140c11a32cc6333a0917546c871967d013dcb076402817',
        },
        patches='recomp/patches-splatoon3-v1120.txt', family='splatoon3', sdk_20_5=True, movie=False,
        boot_nvn=0x4529180, init_arrays={'main': 6775, 'sdk': 83}, exe='splat3'),
    # Splatoon 3 11.3.0: the same sdk and rtld files; main was rebuilt (build id 28c4287a..., one more init_array
    # entry, 2416 more bytes of code). The first nvnBootstrapLoader call is 11.2.0's carried by tools/port_offsets.py
    # (the bl at main+0x4529aec); the runtime's other main offsets for this build are its row of
    # runtime/splat3_addr.c.
    'splatoon3-v1130': Target(
        name='splatoon3-v1130', game='Splatoon 3', title_id='0100C2500FC20000', display_version='11.3.0',
        modules=('main', 'sdk'),
        exefs_sha256={
            'main': '942420a731d52141f97d70baacf7c61ac871de5cebfed44efc39cc70dfcbfd52',
            'sdk': '1d187f29784cd0af46e9e123fc2a62c77b3b07c5a39c8633d90d6c80de64e4ad',
            'rtld': '49308064fddb52dfe9140c11a32cc6333a0917546c871967d013dcb076402817',
        },
        patches='recomp/patches-splatoon3-v1130.txt', family='splatoon3', sdk_20_5=True, movie=False,
        boot_nvn=0x4529af0, init_arrays={'main': 6776, 'sdk': 83}, exe='splat3'),
}


def _totk(name):
    version, patches, boot = TOTK_VERSIONS[name]
    data = json.loads((ROOT / 'tools' / 'data' / f'expected-v{version}.json').read_text())
    hashes = {k: v for k, v in data.get('exefs_sha256', {}).items() if k != 'main.npdm'}
    checkpoint, nvn, inits = boot or (None, None, ())
    return Target(name=name, game='The Legend of Zelda: Tears of the Kingdom', title_id=data['title_id'].upper(),
                  display_version=version, modules=TOTK_MODULES, exefs_sha256=hashes, patches=patches,
                  sdk_20_5=name == 'v143', boot_checkpoint=checkpoint, boot_nvn=nvn,
                  init_arrays=dict(zip(TOTK_MODULES, inits)))


def names():
    return sorted(TOTK_VERSIONS) + sorted(SPLATOON3)


def get(name):
    if name in TOTK_VERSIONS:
        return _totk(name)
    if name in SPLATOON3:
        return SPLATOON3[name]
    raise TargetError(f'unknown target {name!r} (known: {", ".join(names())})')


def program_id(npdm):
    """Program id of the ACI0 section of an NPDM file, as 16 upper-case hex digits."""
    data = Path(npdm).read_bytes()
    if data[:4] != b'META':
        raise TargetError(f'{npdm}: not an NPDM file')
    aci0 = struct.unpack_from('<I', data, 0x70)[0]
    if data[aci0:aci0 + 4] != b'ACI0':
        raise TargetError(f'{npdm}: no ACI0 section')
    return f'{struct.unpack_from("<Q", data, aci0 + 0x10)[0]:016X}'


def sha256(path):
    with open(path, 'rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def check_exefs(target, exefs):
    """Problems of the ExeFS directory for target, as messages (empty when it matches): a lifted module missing, a code
    file whose hash differs, another program id in main.npdm."""
    exefs = Path(exefs)
    problems = []
    for name in target.modules:
        if not (exefs / name).is_file():
            problems.append(f'{name}: missing')
    for name, want in sorted(target.exefs_sha256.items()):
        path = exefs / name
        if not path.is_file():
            continue   # a lifted module is reported above; others (rtld) are checked when present
        have = sha256(path)
        if have != want:
            problems.append(f'{name}: SHA-256 {have}, {target.name} has {want}')
    npdm = exefs / 'main.npdm'
    if npdm.is_file():
        pid = program_id(npdm)
        if pid != target.title_id:
            problems.append(f'main.npdm: program {pid}, {target.name} is {target.title_id}')
    return problems


def describe(target):
    """target.json contents."""
    return {'target': target.name, 'game': target.game, 'title_id': target.title_id,
            'display_version': target.display_version, 'modules': list(target.modules),
            'patches': target.patches}


def c_initializer(target):
    """The C initializer of runtime/host.h RtTarget for target (tools/build_game.py writes it into modules.c)."""
    def s(v):
        return '"' + v.replace('\\', '\\\\').replace('"', '\\"') + '"'
    inits = ', '.join(f'{{{s(m)}, {n}}}' for m, n in target.init_arrays.items())
    return ('{\n'
            f'    .family = RT_GAME_{target.family.upper()},\n'
            f'    .name = {s(target.name)},\n'
            f'    .game = {s(target.game)},\n'
            f'    .display_version = {s(target.display_version)},\n'
            f'    .program_id = 0x{target.title_id}ull,\n'
            f'    .sdk_20_5 = {int(target.sdk_20_5)},\n'
            f'    .movie = {int(target.movie)},\n'
            f'    .boot_checkpoint = {target.boot_checkpoint or 0:#x}ull,\n'
            f'    .boot_nvn = {target.boot_nvn or 0:#x}ull,\n'
            f'    .inits = (const RtInitCount[]){{{inits or "{NULL, 0}"}}},\n'
            f'    .ninits = {len(target.init_arrays)},\n'
            '}')
