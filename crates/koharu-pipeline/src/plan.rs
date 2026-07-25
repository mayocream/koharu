use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use koharu_translator::Providers;
use petgraph::{
    Direction,
    algo::toposort,
    stable_graph::{NodeIndex, StableDiGraph},
    visit::{EdgeRef, IntoEdgeReferences},
};

use crate::{ConfiguredNode, Phase, PipelineConfig, ProcessorId, RunTarget};

#[derive(Clone)]
pub(crate) struct PlanNode {
    pub node: ConfiguredNode,
}

impl PlanNode {
    pub(crate) const fn id(&self) -> ProcessorId {
        self.node.spec().id
    }

    pub(crate) const fn phase(&self) -> Phase {
        self.node.spec().phase
    }
}

pub(crate) struct Plan {
    graph: StableDiGraph<PlanNode, ()>,
    waves: Vec<Vec<NodeIndex>>,
}

impl Plan {
    pub(crate) fn build(config: &PipelineConfig, translation: &Providers) -> Result<Self> {
        let mut graph = StableDiGraph::new();
        let detection = graph.add_node(PlanNode {
            node: ConfiguredNode::Detection(config.detection.clone()),
        });
        let ocr = graph.add_node(PlanNode {
            node: ConfiguredNode::Ocr(config.ocr.clone()),
        });
        let translation = graph.add_node(PlanNode {
            node: ConfiguredNode::Translation(translation.clone()),
        });
        let typography = graph.add_node(PlanNode {
            node: ConfiguredNode::Typography(config.typography.clone()),
        });
        let inpainting = graph.add_node(PlanNode {
            node: ConfiguredNode::Inpainting(config.inpainting.clone()),
        });

        graph.add_edge(detection, ocr, ());
        graph.add_edge(ocr, translation, ());
        graph.add_edge(detection, typography, ());
        graph.add_edge(detection, inpainting, ());

        let waves = waves(&graph)?;
        Ok(Self { graph, waves })
    }

    pub(crate) fn select(mut self, target: &RunTarget) -> Result<Self> {
        let targets = match target {
            RunTarget::All => self.graph.node_indices().collect::<HashSet<_>>(),
            RunTarget::Phase { phase } => self
                .graph
                .node_indices()
                .filter(|index| self.graph[*index].phase() == *phase)
                .collect(),
            RunTarget::Processors { processors } => {
                let mut targets = HashSet::new();
                for processor in processors {
                    let index = self
                        .graph
                        .node_indices()
                        .find(|index| self.graph[*index].id() == *processor)
                        .ok_or_else(|| {
                            anyhow::anyhow!("processor {processor} is not configured")
                        })?;
                    targets.insert(index);
                }
                targets
            }
        };
        if targets.is_empty() {
            bail!("pipeline target selects no processors");
        }

        let retained = ancestors(&self.graph, &targets);
        self.graph
            .retain_nodes(|_, index| retained.contains(&index));
        self.waves = waves(&self.graph)?;
        Ok(self)
    }

    pub(crate) fn nodes(&self) -> impl Iterator<Item = (NodeIndex, &PlanNode)> {
        self.graph
            .node_indices()
            .map(|index| (index, &self.graph[index]))
    }

    pub(crate) fn waves(&self) -> &[Vec<NodeIndex>] {
        &self.waves
    }

    pub(crate) fn node(&self, index: NodeIndex) -> &PlanNode {
        &self.graph[index]
    }

    pub(crate) fn dot(&self) -> String {
        let mut output = String::from("digraph pipeline {\n");
        for (index, node) in self.nodes() {
            output.push_str(&format!(
                "  n{} [label=\"{}: {}\"];\n",
                index.index(),
                node.phase(),
                node.node.name()
            ));
        }
        for edge in self.graph.edge_references() {
            output.push_str(&format!(
                "  n{} -> n{};\n",
                edge.source().index(),
                edge.target().index()
            ));
        }
        output.push_str("}\n");
        output
    }
}

fn ancestors(
    graph: &StableDiGraph<PlanNode, ()>,
    targets: &HashSet<NodeIndex>,
) -> HashSet<NodeIndex> {
    let mut retained = targets.clone();
    let mut pending = targets.iter().copied().collect::<Vec<_>>();
    while let Some(index) = pending.pop() {
        for dependency in graph.neighbors_directed(index, Direction::Incoming) {
            if retained.insert(dependency) {
                pending.push(dependency);
            }
        }
    }
    retained
}

fn waves(graph: &StableDiGraph<PlanNode, ()>) -> Result<Vec<Vec<NodeIndex>>> {
    let order = toposort(graph, None).map_err(|cycle| {
        anyhow::anyhow!(
            "pipeline dependency cycle includes node {}",
            cycle.node_id().index()
        )
    })?;
    let mut depths = HashMap::<NodeIndex, usize>::new();
    let mut waves = Vec::<Vec<NodeIndex>>::new();
    for index in order {
        let depth = graph
            .neighbors_directed(index, Direction::Incoming)
            .filter_map(|dependency| depths.get(&dependency).copied())
            .max()
            .map_or(0, |depth| depth + 1);
        depths.insert(index, depth);
        if waves.len() <= depth {
            waves.resize_with(depth + 1, Vec::new);
        }
        waves[depth].push(index);
    }
    for wave in &mut waves {
        wave.sort_by_key(|index| index.index());
    }
    Ok(waves)
}

#[cfg(test)]
mod tests {
    use super::*;
    use koharu_translator::{OpenAiConfig, Providers};

    fn plan() -> Plan {
        Plan::build(
            &PipelineConfig::default(),
            &Providers::OpenAi(OpenAiConfig::default()),
        )
        .unwrap()
    }

    #[test]
    fn fixed_graph_has_five_nodes_and_three_waves() {
        let plan = plan();

        assert_eq!(plan.nodes().count(), 5);
        assert_eq!(
            plan.waves().iter().map(Vec::len).collect::<Vec<_>>(),
            [1, 3, 1]
        );
    }

    #[test]
    fn translation_selects_detection_and_ocr_ancestors() {
        let plan = plan()
            .select(&RunTarget::Phase {
                phase: Phase::Translation,
            })
            .unwrap();

        assert_eq!(
            plan.nodes()
                .map(|(_, node)| node.phase())
                .collect::<Vec<_>>(),
            [Phase::Detection, Phase::Ocr, Phase::Translation]
        );
    }

    #[test]
    fn typography_selects_detection_ancestor() {
        let plan = plan()
            .select(&RunTarget::Phase {
                phase: Phase::Typography,
            })
            .unwrap();

        assert_eq!(
            plan.nodes()
                .map(|(_, node)| node.phase())
                .collect::<Vec<_>>(),
            [Phase::Detection, Phase::Typography]
        );
    }
}
