//! The `collect` pass: attribution and summing, split out of
//! `project_footprint.rs` purely to keep that file under the 200-line
//! guidance. This is where the double-counting bug would live, so it is
//! tested directly through `collect` with a scripted [`super::ProcessSampler`]
//! fake -- see `project_footprint_tests.rs`.

use std::collections::HashMap;

use gpui::EntityId;

use super::{FootprintRoots, Pid, ProcessSampler, ProjectFootprint};

/// One background pass: discover (optionally) which PIDs belong to which
/// project, sample them narrowly, and sum per project.
///
/// `discover` gates the expensive full-enumeration call: the ~30s cadence
/// calls this with `discover: true`, the ~3s cadence with `discover: false`.
///
/// This function is pure and keeps no state between calls, so on a
/// `discover: false` pass it can only attribute the PIDs it is *given*. The
/// PIDs it attributed are therefore returned alongside each footprint, and the
/// caller is **required** to feed them back in as roots on the narrow ticks.
/// Skip that and the narrow ticks measure root processes only -- a badge that
/// silently sheds every agent's descendant tree for 27 of every 30 seconds and
/// jumps back up for 3.
///
/// Attribution: a PID reachable from more than one root (possible after a
/// re-parent to a shell that is itself a tracked terminal) is credited once,
/// to the first root that claims it in `roots`' order -- never
/// double-counted, since that is what would make the badge exceed reality.
pub fn collect(
    sampler: &mut dyn ProcessSampler,
    roots: &[FootprintRoots],
    discover: bool,
) -> Vec<(EntityId, ProjectFootprint, Vec<Pid>)> {
    let mut owner_of: HashMap<Pid, EntityId> = HashMap::new();
    let mut project_pids: HashMap<EntityId, Vec<Pid>> = HashMap::new();

    for project in roots {
        let pids = project_pids.entry(project.key).or_default();
        for &root in &project.roots {
            claim(&mut owner_of, pids, root, project.key);
        }
    }

    if discover {
        let all_roots: Vec<Pid> = roots.iter().flat_map(|p| p.roots.iter().copied()).collect();
        // A root belongs to exactly one project (see `claim` above), so this
        // lookup lets the loop below attribute each descendant unambiguously.
        let project_of_root: HashMap<Pid, EntityId> = roots
            .iter()
            .flat_map(|p| p.roots.iter().map(move |&r| (r, p.key)))
            .collect();

        for (pid, root) in sampler.descendants(&all_roots) {
            let Some(&project) = project_of_root.get(&root) else {
                continue;
            };
            let pids = project_pids.entry(project).or_default();
            claim(&mut owner_of, pids, pid, project);
        }
    }

    let union: Vec<Pid> = owner_of.keys().copied().collect();
    let mut rss_by_pid: HashMap<Pid, u64> = HashMap::new();
    let mut cpu_by_pid: HashMap<Pid, Option<f32>> = HashMap::new();
    for (pid, rss, cpu) in sampler.sample(&union) {
        rss_by_pid.insert(pid, rss);
        cpu_by_pid.insert(pid, cpu);
    }

    let core_count = sampler.core_count().max(1);
    roots
        .iter()
        .map(|project| {
            let pids = project_pids.remove(&project.key).unwrap_or_default();
            let footprint = sum(&pids, &rss_by_pid, &cpu_by_pid, core_count);
            (project.key, footprint, pids)
        })
        .collect()
}

/// Records that `pid` belongs to `project`, unless another project already
/// claimed it -- the single place double-counting is prevented.
fn claim(owner_of: &mut HashMap<Pid, EntityId>, pids: &mut Vec<Pid>, pid: Pid, project: EntityId) {
    if owner_of.contains_key(&pid) {
        return;
    }
    owner_of.insert(pid, project);
    pids.push(pid);
}

/// Sums one project's PIDs into a footprint. CPU is normalized by core count
/// and clamped to 100%: `sysinfo::cpu_usage()` is per single core and can
/// exceed 100% on a multi-core machine, and a badge reading "340%" answers a
/// different question than the one the user asked.
fn sum(
    pids: &[Pid],
    rss_by_pid: &HashMap<Pid, u64>,
    cpu_by_pid: &HashMap<Pid, Option<f32>>,
    core_count: usize,
) -> ProjectFootprint {
    if pids.is_empty() {
        return ProjectFootprint::default();
    }

    // Already deduplicated by `claim`: a PID is pushed into a project's list
    // only the first time `owner_of` accepts it.
    let mut rss_bytes: Option<u64> = None;
    let mut cpu_percent: Option<f32> = None;
    for pid in pids {
        if let Some(&rss) = rss_by_pid.get(pid) {
            rss_bytes = Some(rss_bytes.unwrap_or(0) + rss);
        }
        if let Some(Some(cpu)) = cpu_by_pid.get(pid) {
            cpu_percent = Some(cpu_percent.unwrap_or(0.0) + cpu);
        }
    }

    ProjectFootprint {
        rss_bytes,
        cpu_percent: cpu_percent.map(|total| (total / core_count as f32).clamp(0.0, 100.0)),
    }
}
