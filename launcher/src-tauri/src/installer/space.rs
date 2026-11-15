//! Disk space, step by step: what each step leaves on the disk and what it needs on top of that while it runs
//! (downloads, a merge's copies), measured on reference installs.
//! The wizard checks before every step that the free space covers that step and everything after it,
//! instead of asking for one large figure at the start.

use super::StepId;

pub const GB: u64 = 1_000_000_000;
/// The merged RomFS of the current game version (romfs.bin, index.json).
pub const ROMFS: u64 = super::gamever::CURRENT.romfs_size + 3_000_000;
/// Generated C of the current game version (1.53 GB measured).
pub const LIFT: u64 = 1_550_000_000;
/// The build folder: objects (0.43 GB), splat3.exe (0.36 GB), its map, the RomFS index and the DLLs (0.84 GB measured).
pub const BUILD: u64 = 850_000_000;
/// The extracted ExeFS (58 MB).
pub const EXEFS: u64 = 60_000_000;
/// What is kept free beyond the estimate (Windows, the page file, the estimate's error).
pub const MARGIN: u64 = GB;

/// How a step uses the disk: `adds` stays once it is done, `extra` only while it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Use {
    pub adds: u64,
    pub extra: u64,
}

/// What a step does to the disk when it runs. `portable`: the tools are downloaded into the install folder;
/// `in_place`: the RomFS merge reads the NCAs inside the NSPs (no copies).
pub fn step_use(id: StepId, portable: bool, in_place: bool, nca_bytes: u64) -> Use {
    let (adds, extra) = match id {
        StepId::Prereqs if portable => (super::portable::TOOLCHAIN_DISK, super::portable::bytes(&super::portable::TOOLCHAIN)),
        StepId::Sources => (20_000_000, 15_000_000),
        StepId::Python if portable => (super::portable::PYTHON_DISK, super::portable::bytes(&super::portable::python_pins())),
        StepId::Python => (150_000_000, 40_000_000),
        StepId::Exefs => (EXEFS, 0),
        StepId::Hactool => (1_000_000, 0),
        StepId::Romfs => (ROMFS, if in_place { 0 } else { nca_bytes }),
        StepId::Lift => (LIFT, 0),
        StepId::Compile => (BUILD, 0),
        StepId::Xlate if portable => (4_000_000, 0),
        StepId::Xlate => (4_000_000, 600_000_000),
        StepId::Keys => (10_000_000, 0),
        _ => (0, 0),
    };
    Use { adds, extra }
}

/// Free space needed before each step of `steps` (the ones that will do work, in order) so that it and every later one
/// fit: going backwards, a step needs its own extra or what it adds plus what the next ones need, whichever is more.
pub fn needs(steps: &[(StepId, Use)]) -> Vec<(StepId, u64)> {
    let mut out = Vec::with_capacity(steps.len());
    let mut after = MARGIN;
    for (id, u) in steps.iter().rev() {
        let need = (u.adds + after).max(u.adds + u.extra + MARGIN);
        out.push((*id, need));
        after = need;
    }
    out.reverse();
    out
}

/// The step during which the install takes the most space (what the earlier ones left, plus its own peak).
pub fn peak_step(steps: &[(StepId, Use)]) -> Option<StepId> {
    let mut before = 0u64;
    let mut best: Option<(u64, StepId)> = None;
    for (id, u) in steps {
        let at = before + u.adds + u.extra;
        if best.is_none_or(|(b, _)| at >= b) {
            best = Some((at, *id));
        }
        before += u.adds;
    }
    best.map(|b| b.1)
}

/// Bytes the steps leave on the disk.
pub fn added(steps: &[(StepId, Use)]) -> u64 {
    steps.iter().map(|(_, u)| u.adds).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_install_needs_far_less_than_the_old_65_gb() {
        let steps: Vec<(StepId, Use)> = StepId::ALL.iter().map(|&id| (id, step_use(id, true, true, 12_000_000_000))).collect();
        let n = needs(&steps);
        let first = n[0].1;
        // 10.1 GB of RomFS, 1.5 GB of C, 1.2 GB of build, the tools, plus the margin
        assert!((13 * GB..17 * GB).contains(&first), "{first}");
        // what is left to do needs less and less
        assert!(n.windows(2).all(|w| w[0].1 >= w[1].1));
        let romfs = n.iter().find(|(id, _)| *id == StepId::Romfs).unwrap().1;
        assert!(romfs > ROMFS + LIFT + BUILD);
        // with the copies of the old merge, the merge itself needs 12 GB more
        let old: Vec<(StepId, Use)> = StepId::ALL.iter().map(|&id| (id, step_use(id, false, false, 12_000_000_000))).collect();
        let old_romfs = needs(&old).iter().find(|(id, _)| *id == StepId::Romfs).unwrap().1;
        assert!(old_romfs >= ROMFS + 12_000_000_000 + MARGIN);
        assert!((12 * GB..16 * GB).contains(&added(&steps)));
    }

    #[test]
    fn an_update_needs_only_the_c_and_the_build() {
        let steps = vec![(StepId::Lift, step_use(StepId::Lift, true, true, 0)), (StepId::Compile, step_use(StepId::Compile, true, true, 0))];
        assert_eq!(needs(&steps)[0].1, LIFT + BUILD + MARGIN);
        assert_eq!(needs(&[]), vec![]);
    }
}
