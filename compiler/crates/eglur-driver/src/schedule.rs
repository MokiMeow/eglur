use std::collections::BTreeSet;
use std::fmt;

use eglur_source::FileId;

use crate::observation::{ObservationCollector, Phase};

/// One resolved compilation-unit dependency edge.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ImportDependency {
    importer: FileId,
    imported: FileId,
}

impl ImportDependency {
    /// Records that `importer` depends on `imported`.
    #[must_use]
    pub const fn new(importer: FileId, imported: FileId) -> Self {
        Self { importer, imported }
    }

    /// Returns the source containing the resolved import.
    #[must_use]
    pub const fn importer(self) -> FileId {
        self.importer
    }

    /// Returns the source unit selected by resolution.
    #[must_use]
    pub const fn imported(self) -> FileId {
        self.imported
    }
}

/// One strongly connected import analysis unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportScc {
    members: Box<[FileId]>,
}

impl ImportScc {
    /// Returns member source units in canonical `FileId` order.
    #[must_use]
    pub fn members(&self) -> &[FileId] {
        &self.members
    }
}

/// Dependency-first deterministic schedule of all import SCCs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportSchedule {
    units: Box<[ImportScc]>,
}

impl ImportSchedule {
    /// Returns SCC analysis units in dependency-first order.
    #[must_use]
    pub fn units(&self) -> &[ImportScc] {
        &self.units
    }
}

/// Invalid resolved-edge input to the import scheduler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportScheduleError {
    FrontendBlocked,
    UnknownFile(FileId),
}

impl fmt::Display for ImportScheduleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrontendBlocked => {
                formatter.write_str("import scheduling is unavailable after build-input failure")
            }
            Self::UnknownFile(file) => {
                write!(
                    formatter,
                    "resolved import edge references unknown source {file}"
                )
            }
        }
    }
}

impl std::error::Error for ImportScheduleError {}

pub(crate) fn build_import_schedule(
    files: &[FileId],
    dependencies: impl IntoIterator<Item = ImportDependency>,
) -> Result<ImportSchedule, ImportScheduleError> {
    let mut files = files.to_vec();
    files.sort_unstable();
    files.dedup();
    let file_set = files.iter().copied().collect::<BTreeSet<_>>();

    let mut dependencies = dependencies.into_iter().collect::<Vec<_>>();
    dependencies.sort_unstable();
    dependencies.dedup();
    for dependency in &dependencies {
        for file in [dependency.importer, dependency.imported] {
            if !file_set.contains(&file) {
                return Err(ImportScheduleError::UnknownFile(file));
            }
        }
    }

    let index_by_file = files
        .iter()
        .enumerate()
        .map(|(index, &file)| (file, index))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut forward = vec![Vec::new(); files.len()];
    let mut reverse = vec![Vec::new(); files.len()];
    for dependency in dependencies {
        let importer = index_by_file[&dependency.importer];
        let imported = index_by_file[&dependency.imported];
        forward[importer].push(imported);
        reverse[imported].push(importer);
    }
    for edges in forward.iter_mut().chain(reverse.iter_mut()) {
        edges.sort_unstable();
        edges.dedup();
    }

    let finishing_order = finishing_order(&forward);
    let mut component_by_node = vec![usize::MAX; files.len()];
    let mut components = Vec::new();
    for &start in finishing_order.iter().rev() {
        if component_by_node[start] != usize::MAX {
            continue;
        }
        let component = components.len();
        let mut members = Vec::new();
        let mut stack = vec![start];
        component_by_node[start] = component;
        while let Some(node) = stack.pop() {
            members.push(files[node]);
            for &next in reverse[node].iter().rev() {
                if component_by_node[next] == usize::MAX {
                    component_by_node[next] = component;
                    stack.push(next);
                }
            }
        }
        members.sort_unstable();
        components.push(members);
    }

    let mut dependents = vec![BTreeSet::new(); components.len()];
    let mut dependency_count = vec![0_usize; components.len()];
    for (importer, imported_files) in forward.iter().enumerate() {
        let importer_component = component_by_node[importer];
        for &imported in imported_files {
            let imported_component = component_by_node[imported];
            if importer_component != imported_component
                && dependents[imported_component].insert(importer_component)
            {
                dependency_count[importer_component] += 1;
            }
        }
    }

    let mut ready = dependency_count
        .iter()
        .enumerate()
        .filter(|&(_, &count)| count == 0)
        .map(|(component, _)| (components[component][0], component))
        .collect::<BTreeSet<_>>();
    let mut units = Vec::with_capacity(components.len());
    while let Some(&(key, component)) = ready.first() {
        ready.remove(&(key, component));
        units.push(ImportScc {
            members: components[component].clone().into_boxed_slice(),
        });
        for &dependent in &dependents[component] {
            dependency_count[dependent] -= 1;
            if dependency_count[dependent] == 0 {
                ready.insert((components[dependent][0], dependent));
            }
        }
    }
    debug_assert_eq!(units.len(), components.len());
    Ok(ImportSchedule {
        units: units.into_boxed_slice(),
    })
}

pub(crate) fn build_observed_import_schedule(
    files: &[FileId],
    dependencies: impl IntoIterator<Item = ImportDependency>,
    observations: &mut ObservationCollector,
) -> Result<ImportSchedule, ImportScheduleError> {
    let dependencies = dependencies.into_iter().collect::<Vec<_>>();
    observations.measure(
        Phase::ImportScc,
        files.len(),
        || build_import_schedule(files, dependencies),
        |schedule| schedule.units().len(),
    )
}

fn finishing_order(adjacency: &[Vec<usize>]) -> Vec<usize> {
    let mut visited = vec![false; adjacency.len()];
    let mut order = Vec::with_capacity(adjacency.len());
    for start in 0..adjacency.len() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut stack = vec![(start, 0_usize)];
        while let Some((node, edge_index)) = stack.last_mut() {
            if let Some(&next) = adjacency[*node].get(*edge_index) {
                *edge_index += 1;
                if !visited[next] {
                    visited[next] = true;
                    stack.push((next, 0));
                }
            } else {
                let (finished, _) = stack.pop().expect("active DFS stack is non-empty");
                order.push(finished);
            }
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use eglur_source::FileId;

    use super::{ImportDependency, ImportScheduleError, build_import_schedule};

    fn id(raw: u32) -> FileId {
        FileId::new(raw)
    }

    fn projection(
        files: &[FileId],
        edges: impl IntoIterator<Item = ImportDependency>,
    ) -> Vec<Vec<u32>> {
        build_import_schedule(files, edges)
            .unwrap()
            .units()
            .iter()
            .map(|unit| unit.members().iter().map(|file| file.get()).collect())
            .collect()
    }

    #[test]
    fn cycles_self_edges_and_isolated_files_form_bounded_units() {
        let files = [id(0), id(1), id(2), id(3)];
        let edges = [
            ImportDependency::new(id(0), id(1)),
            ImportDependency::new(id(1), id(0)),
            ImportDependency::new(id(2), id(2)),
        ];
        assert_eq!(
            projection(&files, edges),
            vec![vec![0, 1], vec![2], vec![3]]
        );
    }

    #[test]
    fn dependency_first_order_is_stable_for_diamonds_and_shuffled_duplicates() {
        let files = [id(0), id(1), id(2), id(3)];
        let edges = [
            ImportDependency::new(id(0), id(2)),
            ImportDependency::new(id(2), id(3)),
            ImportDependency::new(id(0), id(1)),
            ImportDependency::new(id(1), id(3)),
            ImportDependency::new(id(0), id(1)),
        ];
        let mut shuffled = edges;
        shuffled.reverse();
        assert_eq!(
            projection(&files, edges),
            vec![vec![3], vec![1], vec![2], vec![0]]
        );
        assert_eq!(projection(&files, edges), projection(&files, shuffled));
    }

    #[test]
    fn unknown_edge_endpoint_is_rejected() {
        assert_eq!(
            build_import_schedule(&[id(0)], [ImportDependency::new(id(0), id(7))]),
            Err(ImportScheduleError::UnknownFile(id(7)))
        );
    }

    #[test]
    fn deep_dependency_chains_do_not_use_the_call_stack() {
        const SOURCE_COUNT: u32 = 20_000;
        let files = (0..SOURCE_COUNT).map(id).collect::<Vec<_>>();
        let edges = (1..SOURCE_COUNT)
            .map(|file| ImportDependency::new(id(file), id(file - 1)))
            .collect::<Vec<_>>();
        let schedule = build_import_schedule(&files, edges).unwrap();

        assert_eq!(schedule.units().len(), SOURCE_COUNT as usize);
        assert_eq!(schedule.units().first().unwrap().members(), &[id(0)]);
        assert_eq!(
            schedule.units().last().unwrap().members(),
            &[id(SOURCE_COUNT - 1)]
        );
    }
}
