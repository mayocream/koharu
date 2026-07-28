use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use petgraph::{Direction, algo::toposort, graphmap::DiGraphMap};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Type,
    strum::Display,
    strum::EnumIter,
    strum::EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum Stage {
    Detection,
    Ocr,
    Translation,
    Inpainting,
}

impl Stage {
    pub const ALL: [Self; 4] = [
        Self::Detection,
        Self::Ocr,
        Self::Translation,
        Self::Inpainting,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dependency {
    DetectedTextRegions,
    RecognizedSourceText,
    DetectedTextMask,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "target", content = "stages", rename_all = "snake_case")]
pub enum Target {
    #[default]
    All,
    Stage(Stage),
    Stages(BTreeSet<Stage>),
    Exact(BTreeSet<Stage>),
}

pub(crate) struct Selection {
    pub stages: BTreeSet<Stage>,
    pub exact: bool,
}

pub(crate) struct PipelineGraph {
    dependencies: DiGraphMap<Stage, Dependency>,
    canonical_order: Vec<Stage>,
}

impl PipelineGraph {
    pub(crate) fn new() -> Result<Self> {
        let mut dependencies = DiGraphMap::new();
        for stage in Stage::ALL {
            dependencies.add_node(stage);
        }
        depends_on(
            &mut dependencies,
            Stage::Ocr,
            Stage::Detection,
            Dependency::DetectedTextRegions,
        );
        depends_on(
            &mut dependencies,
            Stage::Translation,
            Stage::Ocr,
            Dependency::RecognizedSourceText,
        );
        depends_on(
            &mut dependencies,
            Stage::Inpainting,
            Stage::Detection,
            Dependency::DetectedTextMask,
        );
        toposort(&dependencies, None).map_err(|cycle| {
            anyhow::anyhow!("pipeline dependency cycle includes {}", cycle.node_id())
        })?;
        let canonical_order = canonical_order(&dependencies)?;
        Ok(Self {
            dependencies,
            canonical_order,
        })
    }

    pub(crate) fn select(&self, target: &Target) -> Result<Selection> {
        let (mut stages, exact) = match target {
            Target::All => (Stage::ALL.into_iter().collect(), false),
            Target::Stage(stage) => (BTreeSet::from([*stage]), false),
            Target::Stages(stages) => (stages.clone(), false),
            Target::Exact(stages) => (stages.clone(), true),
        };
        if stages.is_empty() {
            bail!("pipeline target selects no stages");
        }
        if !exact {
            let mut pending = stages.iter().copied().collect::<Vec<_>>();
            while let Some(stage) = pending.pop() {
                for prerequisite in self
                    .dependencies
                    .neighbors_directed(stage, Direction::Incoming)
                {
                    if stages.insert(prerequisite) {
                        pending.push(prerequisite);
                    }
                }
            }
        }
        Ok(Selection { stages, exact })
    }

    pub(crate) fn canonical(&self) -> &[Stage] {
        &self.canonical_order
    }

    pub(crate) fn incoming_selected<'a>(
        &'a self,
        stage: Stage,
        selected: &'a BTreeSet<Stage>,
    ) -> impl Iterator<Item = Stage> + 'a {
        self.dependencies
            .neighbors_directed(stage, Direction::Incoming)
            .filter(move |candidate| selected.contains(candidate))
    }

    pub(crate) fn outgoing_selected<'a>(
        &'a self,
        stage: Stage,
        selected: &'a BTreeSet<Stage>,
    ) -> impl Iterator<Item = Stage> + 'a {
        self.dependencies
            .neighbors_directed(stage, Direction::Outgoing)
            .filter(move |candidate| selected.contains(candidate))
    }

    pub(crate) fn ancestors(&self, stage: Stage, selected: &BTreeSet<Stage>) -> BTreeSet<Stage> {
        let mut ancestors = BTreeSet::new();
        let mut pending = self.incoming_selected(stage, selected).collect::<Vec<_>>();
        while let Some(candidate) = pending.pop() {
            if ancestors.insert(candidate) {
                pending.extend(self.incoming_selected(candidate, selected));
            }
        }
        ancestors
    }

    pub(crate) fn dot(&self) -> String {
        let mut output = String::from("digraph pipeline {\n");
        for stage in &self.canonical_order {
            output.push_str(&format!("  {stage} [label=\"{stage}\"];\n"));
        }
        for prerequisite in &self.canonical_order {
            let mut dependents = self
                .dependencies
                .neighbors_directed(*prerequisite, Direction::Outgoing)
                .collect::<Vec<_>>();
            dependents.sort_unstable();
            for dependent in dependents {
                let reason = self.dependencies[(*prerequisite, dependent)];
                output.push_str(&format!(
                    "  {prerequisite} -> {dependent} [label=\"{reason:?}\"];\n"
                ));
            }
        }
        output.push_str("}\n");
        output
    }
}

fn depends_on(
    graph: &mut DiGraphMap<Stage, Dependency>,
    dependent: Stage,
    prerequisite: Stage,
    reason: Dependency,
) {
    graph.add_edge(prerequisite, dependent, reason);
}

fn canonical_order(graph: &DiGraphMap<Stage, Dependency>) -> Result<Vec<Stage>> {
    let mut remaining = BTreeMap::new();
    let mut ready = BTreeSet::new();
    for stage in Stage::ALL {
        let count = graph.neighbors_directed(stage, Direction::Incoming).count();
        remaining.insert(stage, count);
        if count == 0 {
            ready.insert(stage);
        }
    }
    let mut order = Vec::with_capacity(remaining.len());
    while let Some(stage) = ready.pop_first() {
        order.push(stage);
        for dependent in graph.neighbors_directed(stage, Direction::Outgoing) {
            let count = remaining
                .get_mut(&dependent)
                .expect("graph neighbors are registered nodes");
            *count -= 1;
            if *count == 0 {
                ready.insert(dependent);
            }
        }
    }
    if order.len() != remaining.len() {
        bail!("pipeline dependency graph contains a cycle");
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_closure_contains_only_its_ancestors() {
        let graph = PipelineGraph::new().unwrap();
        let selected = graph.select(&Target::Stage(Stage::Translation)).unwrap();
        assert_eq!(
            selected.stages,
            BTreeSet::from([Stage::Detection, Stage::Ocr, Stage::Translation])
        );
    }

    #[test]
    fn canonical_order_is_stable() {
        let graph = PipelineGraph::new().unwrap();
        assert_eq!(
            graph.canonical(),
            &[
                Stage::Detection,
                Stage::Ocr,
                Stage::Translation,
                Stage::Inpainting,
            ]
        );
    }
}
