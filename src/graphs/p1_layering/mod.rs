pub(crate) mod rank;
mod tree;
#[cfg(test)]
mod tests;

use std::collections::{HashSet, HashMap, VecDeque};

use petgraph::{Directed, Undirected};
use petgraph::Direction::{*, self};
use petgraph::graph::Node;
use petgraph::stable_graph::{StableDiGraph, NodeIndex, StableUnGraph, StableGraph, EdgeIndex};
use petgraph::visit::{EdgeRef, IntoNeighborsDirected, IntoNodeIdentifiers, IntoEdgeReferences};

use crate::util::layers::Layers;

use self::rank::Ranks;
use self::tree::{TightTreeDFSs, TightTreeDFS};

use super::p2_reduce_crossings::ProperLayeredGraph;

#[derive(Clone, Copy)]
struct Vertex {
    id: usize,
    rank: i32,
    low: u32,
    lim: u32,
    parent: Option<NodeIndex>,
}

impl Default for Vertex {
    fn default() -> Self {
        Self {
            id: 0,
            rank: 0,
            low: 0,
            lim: 0,
            parent: None,
        }
    }
}

#[derive(Clone, Copy)]
struct Edge {
    weight: i32,
    cut_value: Option<i32>,
    is_tree_edge: bool,
}

impl Default for Edge {
    fn default() -> Self {
        Self {
            weight: 1,
            cut_value: None,   
            is_tree_edge: false,
        }
    }
}

struct NeighborhoodInfo {
    cut_value_sum: i32,
    tree_edge_weight_sum: i32,
    non_tree_edge_weight_sum: i32,
    missing: Option<NodeIndex>,
}

pub(crate) fn start_layering<T: Default>(graph: StableDiGraph<Option<T>, usize>) -> UnlayeredGraph<T> {
    UnlayeredGraph { graph }
}

fn start(edges: &[(u32, u32)], minimum_length: u32) -> Unlayered {
    let graph = StableDiGraph::<Vertex, Edge>::from_edges(edges);
    Unlayered { graph, minimum_length: minimum_length as i32 }
}

trait Slack {
    fn slack(&self, graph: &StableDiGraph<Vertex, Edge>, minimum_length: i32) -> i32; 
}

impl Slack for EdgeIndex {
    fn slack(&self, graph: &StableDiGraph<Vertex, Edge>, minimum_length: i32) -> i32 {
        let (tail, head) = graph.edge_endpoints(*self).unwrap();
        graph[head].rank - graph[tail].rank - minimum_length
    }
}

trait SlackGraph {
    fn slack(&self, edge: EdgeIndex) -> i32 {
        let graph = self.graph();
        let (tail, head) = graph.edge_endpoints(edge).unwrap();
        graph[head].rank - graph[tail].rank - self.minimum_length()
    }

    fn graph(&self) -> &StableDiGraph<Vertex, Edge>;
    fn minimum_length(&self) -> i32;
}

macro_rules! impl_slack {
    ($t:ty) => {
        impl SlackGraph for $t {
            fn graph(&self) -> &StableDiGraph<Vertex, Edge> {
                &self.graph
            }

            fn minimum_length(&self) -> i32 {
                self.minimum_length
            }
        } 
    };
}

struct Unlayered {
    graph: StableDiGraph<Vertex, Edge>,
    minimum_length: i32
}

impl Unlayered {
    fn initial_ranking(self) -> Ranked {
        let Self { mut graph, minimum_length } = self;

        // Sort nodes topologically so we don't need to verify that we've assigned
        // a rank to all incoming neighbors
        // assume graphs contain no circles for now
        for v in petgraph::algo::toposort(&graph, None).unwrap() {
            let rank = graph.neighbors_directed(v, Incoming)
                                 .map(|n| graph[n].rank + 1)
                                 .max();

            if let Some(rank) = rank {
                graph[v].rank = rank;
            }
        }

        Ranked { graph, minimum_length }
    }
}

struct Ranked {
    graph: StableDiGraph<Vertex, Edge>,
    minimum_length: i32
}

impl_slack!(Ranked);

impl Ranked {
    fn make_tight(mut self) -> Tight {
        // let Self { mut graph, minimum_length } = self;

        // take a random edge to start the tree from
        // split edges into sets consisting of tree and non tree edges.
        // in the beginning, all edges are non tree edges, and they are added
        // with each call to dfs.

        // build a new graph which is a tree. 
        // Remember only edges which where part of the original graph
        // each time we add an edge to the tree, we remove it from the graph
        let num_nodes = self.graph.node_count();
        let mut nodes = self.graph.node_indices().collect::<Vec<_>>().into_iter();
        let mut dfs = TightTreeDFS::new();
        
        while dfs.tight_tree(&self, nodes.next().unwrap(), &mut HashSet::new()) < num_nodes {
            let edge = self.find_non_tight_edge(&dfs);
            let (_, head) = self.graph.edge_endpoints(edge).unwrap();
            let mut delta = self.slack(edge);

            if dfs.contains_vertex(&head) {
                delta = -delta;
            }

            self.tighten_edge(&dfs, delta)
        }

        self.mark_tree_edges(dfs);

        Tight { graph: self.graph, minimum_length: self.minimum_length }
    }

    fn find_non_tight_edge(&self, dfs: &TightTreeDFS) -> EdgeIndex {
        self.graph.edge_indices()
            .filter(|e| !dfs.contains_edge(*e) && dfs.is_incident_edge(e, &self.graph))
            .min_by(|e1, e2| self.slack(*e1).cmp(&self.slack(*e2))).unwrap()
    }

    fn tighten_edge(&mut self, dfs: &TightTreeDFS, delta: i32) {
        for e in &dfs.edges {
            let (tail, head) = self.graph.edge_endpoints(*e).unwrap();
            self.graph[tail].rank += delta;
            self.graph[head].rank += delta;
        }
    }

    fn mark_tree_edges(&mut self, dfs: TightTreeDFS) {
        for e in dfs.edges {
            self.graph[e].is_tree_edge = true;
        }
    }

}


struct Upd {
    graph: StableDiGraph<Vertex, Edge>,
    minimum_length: i32,
    connecting_path: Vec<EdgeIndex>,
    removed_edge: EdgeIndex,
    least_common_ancestor: NodeIndex,
    updated_low_lim: bool,
    updated_cut_values: bool,
    updated_ranks: bool,
}

impl CalculateCutValues for Upd {
    fn graph_mut(&mut self) -> &mut StableDiGraph<Vertex, Edge> {
        &mut self.graph
    }

    fn graph(&self) -> &StableDiGraph<Vertex, Edge> {
        &self.graph
    }
}

impl LowLimDFS for Upd {
    fn graph(&mut self) -> &mut StableDiGraph<Vertex, Edge> {
        &mut self.graph
    }
}

impl Upd {
    fn new(graph: StableDiGraph<Vertex, Edge>, minimum_length: i32, connecting_path: Vec<EdgeIndex>, removed_edge: EdgeIndex, least_common_ancestor: NodeIndex) -> Self {
        Self {
            graph, 
            minimum_length,
            connecting_path,
            removed_edge,
            least_common_ancestor,
            updated_cut_values: false,
            updated_low_lim: false,
            updated_ranks: false
        }
        
    }
    fn update_cutvalues(mut self) -> Self {
        self.remove_outdated_cutvalues();
        let queue = VecDeque::from([self.graph.edge_endpoints(self.removed_edge).unwrap().0]);
        self.calculate_cut_values(queue);
        self.updated_cut_values = true;
        self
    }

    fn remove_outdated_cutvalues(&mut self) {
        // starting from the first node, we know all adjacent cutvalues except for one.
        // thus we should be able to update every cut value efficiently by going through the path.
        // the last thing we need to do is calculate the cut value for the edge that was added.
        // remove all the cutvalues on the path:
        self.graph[self.removed_edge].cut_value = None;
        for edge in &self.connecting_path {
            self.graph[*edge].cut_value = None;
        }
    }

    fn update_low_lim(mut self) -> Self {
        let parent = self.graph[self.least_common_ancestor].parent;
        let mut visited = match &parent {
            Some(parent) => HashSet::from([*parent]),
            None => HashSet::new()
        };
        let mut max_lim = self.graph[self.least_common_ancestor].lim;
        self.dfs_low_lim(self.least_common_ancestor, parent, &mut max_lim, &mut visited);
        self.updated_low_lim = true;
        self
    }

    fn update_ranks(mut self) -> Self {
        let node = self.graph.node_identifiers().next().unwrap();
        let mut visited = HashSet::from([node]);
        self.graph[node].rank = 0;
        let mut queue = VecDeque::from([node]);

        while let Some(parent) = queue.pop_front() {
            self.update_neighbor_ranks(parent, Outgoing, 1, &mut queue, &mut visited);
            self.update_neighbor_ranks(parent, Incoming, -1, &mut queue, &mut visited);
        }
        self.updated_ranks = true;
        self
    }

    fn update_neighbor_ranks(&mut self, parent: NodeIndex, direction: Direction, coefficient: i32, queue: &mut VecDeque<NodeIndex>,  visited: &mut HashSet<NodeIndex>) {
        while let Some((edge, other)) = self.graph.neighbors_directed(parent, direction).detach().next(&self.graph) {
            if !self.graph[edge].is_tree_edge || visited.contains(&other) {
                continue;
            }
            self.graph[other].rank = self.graph[parent].rank + self.minimum_length * coefficient;
            queue.push_back(other);
            visited.insert(other);
        }
    }

    fn execute(self) -> Feasible {
        assert!(self.updated_cut_values && self.updated_low_lim && self.updated_ranks);
        Feasible { graph: self.graph, minimum_length: self.minimum_length }
    }
}
// struct UpdCutVals {
//     graph: StableDiGraph<Vertex, Edge>,
//     minimum_length: i32,
//     connecting_path: Vec<EdgeIndex>,
//     removed_edge: EdgeIndex,
// }

// impl UpdCutVals {
//     fn update_cutvalues(mut self) -> UpdLowLim {
//         self.remove_outdated_cutvalues(self.connecting_path, self.removed_edge);
//         let queue = VecDeque::from([self.graph.edge_endpoints(self.removed_edge).unwrap().0]);
//         let Self { graph, minimum_length, .. } = self;
//         let t = Tight { graph: self.graph, minimum_length: self.minimum_length };
//         t.calculate_cut_values(queue);
//         UpdLowLim { graph: t.graph, minimum_length: t.minimum_length }
//     }

//     fn remove_outdated_cutvalues(&mut self, connecting_path: Vec<EdgeIndex>, removed_edge: EdgeIndex) {
//         // starting from the first node, we know all adjacent cutvalues except for one.
//         // thus we should be able to update every cut value efficiently by going through the path.
//         // the last thing we need to do is calculate the cut value for the edge that was added.
//         // remove all the cutvalues on the path:
//         self.graph[removed_edge].cut_value = None;
//         for edge in connecting_path {
//             self.graph[edge].cut_value = None;
//         }
//     }
// }

struct Tight {
    graph: StableDiGraph<Vertex, Edge>,
    minimum_length: i32,
}

impl Tight {
    pub(crate) fn init_cutvalues(mut self) -> InitLowLim {
        // TODO: check if it is faster to collect tree edges or to do unecessary iterations
        // let tree_edges = self.graph.edge_indices().filter(|e| self.graph[*e].is_tree_edge).collect::<HashSet<_>>();
        let queue = self.leaves();
        // traverse tree inward via breadth first starting from leaves
        self.calculate_cut_values(queue);
        InitLowLim { graph: self.graph, minimum_length: self.minimum_length }
    }

    fn update_cutvalues(mut self, connecting_path: Vec<EdgeIndex>, removed_edge: (NodeIndex, NodeIndex)) -> InitLowLim {
        self.remove_outdated_cutvalues(connecting_path, removed_edge);
        let queue = VecDeque::from([removed_edge.0]);
        self.calculate_cut_values(queue);
        InitLowLim { graph: self.graph, minimum_length: self.minimum_length }
    }

    fn leaves(&self) -> VecDeque<NodeIndex> {
        self.graph.node_identifiers()
                  .filter(|v| 
                    1 == self.graph.edges_directed(*v, Incoming)
                              .chain(self.graph.edges_directed(*v, Outgoing))
                              .filter(|e| e.weight().is_tree_edge)
                              .count())
                  .collect::<VecDeque<_>>()
    }

    fn remove_outdated_cutvalues(&mut self, connecting_path: Vec<EdgeIndex>, removed_edge: (NodeIndex, NodeIndex)) {
        // starting from the first node, we know all adjacent cutvalues except for one.
        // thus we should be able to update every cut value efficiently by going through the path.
        // the last thing we need to do is calculate the cut value for the edge that was added.
        // remove all the cutvalues on the path:
        let removed_edge = self.graph.find_edge_undirected(removed_edge.0, removed_edge.1).unwrap().0;
        self.graph[removed_edge].cut_value = None;
        for edge in connecting_path {
            self.graph[edge].cut_value = None;
        }
    }

    fn calculate_cut_values(&mut self, mut queue: VecDeque<NodeIndex>) {
        while let Some(vertex) = queue.pop_front() {
            let incoming = self.get_neighborhood_info(vertex, Incoming); 
            let outgoing = self.get_neighborhood_info(vertex, Outgoing); 

            // if we can't calculate cut value yet, or the value is already known
            let (mut incoming, mut outgoing) = match (incoming, outgoing) {
                (Some(inc), Some(out)) => (inc, out),
                _ => continue,
            };

            let missing = match (incoming.missing, outgoing.missing) {
                (Some(u), None) => u,
                (None, Some(v)) => v,
                _ => continue,
            };

            let edge = match self.graph.find_edge(vertex, missing) {
                Some(e) => {
                    // switch direction, if vertex is tail component of edge
                    std::mem::swap(&mut incoming, &mut outgoing);
                    e
                },
                None => self.graph.find_edge(missing, vertex).unwrap()

            };

            self.graph[edge].cut_value = Some(self.calculate_cut_value(edge, incoming, outgoing));
            // continue traversing tree in direction of edge whose vertex was missing before
            queue.push_back(missing);
        }
    }

    fn calculate_cut_value(&self, edge: EdgeIndex, incoming: NeighborhoodInfo, outgoing: NeighborhoodInfo) -> i32 {
        self.graph[edge].weight 
        + incoming.non_tree_edge_weight_sum - incoming.cut_value_sum + incoming.tree_edge_weight_sum
        - outgoing.non_tree_edge_weight_sum + outgoing.cut_value_sum - outgoing.tree_edge_weight_sum
    }

    fn get_neighborhood_info(&self, vertex: NodeIndex, direction: Direction) -> Option<NeighborhoodInfo> {
        // return the sum of all cut values,
        // sum of weights of cut value edges
        // missing cut value (only if there is one)
        // sum of weights of edges who are not tree edges
        let mut cut_value_sum = 0;
        let mut tree_edge_weight_sum = 0;
        let mut non_tree_edge_weight_sum = 0;
        let mut missing = None;

        for edge in self.graph.edges_directed(vertex, direction) {
            let (tail, head) = (edge.source(), edge.target());
            let edge = *edge.weight();
            if !edge.is_tree_edge {
                non_tree_edge_weight_sum += edge.weight;
            } else if let Some(cut_value) = edge.cut_value {
                cut_value_sum += cut_value;
                tree_edge_weight_sum += edge.weight;
            } else if missing.is_none() {
                missing = Some(if tail == vertex { head } else { tail });
            } else {
                return None;
            }
        }
        Some(
            NeighborhoodInfo {
                cut_value_sum,
                tree_edge_weight_sum,
                non_tree_edge_weight_sum,
                missing,
            }
        )
    }

}


struct InitLowLim {
    graph: StableDiGraph<Vertex, Edge>,
    minimum_length: i32,
}

impl LowLimDFS for InitLowLim {
    fn graph(&mut self) -> &mut StableDiGraph<Vertex, Edge> {
        &mut self.graph
    }
} 

impl InitLowLim {
    fn initialize_low_lim(mut self) {
        // start at arbitrary root node
        let root = self.graph.node_indices().next().unwrap();
        let mut max_lim = self.graph.node_count() as u32;
        self.dfs_low_lim(root, None, &mut max_lim, &mut HashSet::new());
    }
}

struct UpdLowLim {
    least_common_ancestor: NodeIndex,
    graph: StableDiGraph<Vertex, Edge>,
    minimum_length: i32,
}

impl LowLimDFS for UpdLowLim {
    fn graph(&mut self) -> &mut StableDiGraph<Vertex, Edge> {
        &mut self.graph
    }
}

impl UpdLowLim {
    fn update_low_lim(mut self) {
        let parent = self.graph[self.least_common_ancestor].parent;
        let mut visited = match &parent {
            Some(parent) => HashSet::from([*parent]),
            None => HashSet::new()
        };
        let mut max_lim = self.graph[self.least_common_ancestor].lim;
        self.dfs_low_lim(self.least_common_ancestor, parent, &mut max_lim, &mut visited);
    }
}

trait LowLimDFS {
    fn dfs_low_lim(&mut self, next: NodeIndex, parent: Option<NodeIndex>, max_lim: &mut u32, visited: &mut HashSet<NodeIndex>) {
        visited.insert(next);
        self.graph()[next].lim = *max_lim;
        self.graph()[next].parent = parent;
        while let Some(n) = self.graph().neighbors_undirected(next).detach().next_node(self.graph()) {
            if visited.contains(&n) {
                continue;
            }
            *max_lim -= 1;
            self.dfs_low_lim(n, Some(next), max_lim, visited);
            self.graph()[next].low = *max_lim;
        }
    }
    fn graph(&mut self) -> &mut StableDiGraph<Vertex, Edge>;
}

trait CalculateCutValues {
    fn calculate_cut_values(&mut self, mut queue: VecDeque<NodeIndex>) {
        while let Some(vertex) = queue.pop_front() {
            let incoming = self.get_neighborhood_info(vertex, Incoming); 
            let outgoing = self.get_neighborhood_info(vertex, Outgoing); 

            // if we can't calculate cut value yet, or the value is already known
            let (mut incoming, mut outgoing) = match (incoming, outgoing) {
                (Some(inc), Some(out)) => (inc, out),
                _ => continue,
            };

            let missing = match (incoming.missing, outgoing.missing) {
                (Some(u), None) => u,
                (None, Some(v)) => v,
                _ => continue,
            };

            let edge = match self.graph_mut().find_edge(vertex, missing) {
                Some(e) => {
                    // switch direction, if vertex is tail component of edge
                    std::mem::swap(&mut incoming, &mut outgoing);
                    e
                },
                None => self.graph_mut().find_edge(missing, vertex).unwrap()

            };

            self.graph_mut()[edge].cut_value = Some(self.calculate_cut_value(edge, incoming, outgoing));
            // continue traversing tree in direction of edge whose vertex was missing before
            queue.push_back(missing);
        }
    }

    fn calculate_cut_value(&self, edge: EdgeIndex, incoming: NeighborhoodInfo, outgoing: NeighborhoodInfo) -> i32 {
        self.graph()[edge].weight 
        + incoming.non_tree_edge_weight_sum - incoming.cut_value_sum + incoming.tree_edge_weight_sum
        - outgoing.non_tree_edge_weight_sum + outgoing.cut_value_sum - outgoing.tree_edge_weight_sum
    }

    fn get_neighborhood_info(&self, vertex: NodeIndex, direction: Direction) -> Option<NeighborhoodInfo> {
        // return the sum of all cut values,
        // sum of weights of cut value edges
        // missing cut value (only if there is one)
        // sum of weights of edges who are not tree edges
        let mut cut_value_sum = 0;
        let mut tree_edge_weight_sum = 0;
        let mut non_tree_edge_weight_sum = 0;
        let mut missing = None;

        for edge in self.graph().edges_directed(vertex, direction) {
            let (tail, head) = (edge.source(), edge.target());
            let edge = *edge.weight();
            if !edge.is_tree_edge {
                non_tree_edge_weight_sum += edge.weight;
            } else if let Some(cut_value) = edge.cut_value {
                cut_value_sum += cut_value;
                tree_edge_weight_sum += edge.weight;
            } else if missing.is_none() {
                missing = Some(if tail == vertex { head } else { tail });
            } else {
                return None;
            }
        }
        Some(
            NeighborhoodInfo {
                cut_value_sum,
                tree_edge_weight_sum,
                non_tree_edge_weight_sum,
                missing,
            }
        )
    }

    fn graph_mut(&mut self) -> &mut StableDiGraph<Vertex, Edge>;
    fn graph(&self) -> &StableDiGraph<Vertex, Edge>;
}

struct UpdateRank {
    graph: StableDiGraph<Vertex, Edge>,
    minimum_length: i32,
}

impl UpdateRank {
    fn update_ranks(mut self) {
        let node = self.graph.node_identifiers().next().unwrap();
        let mut visited = HashSet::from([node]);
        self.graph[node].rank = 0;
        let mut queue = VecDeque::from([node]);

        while let Some(parent) = queue.pop_front() {
            self.update_neighbor_ranks(parent, Outgoing, 1, &mut queue, &mut visited);
            self.update_neighbor_ranks(parent, Incoming, -1, &mut queue, &mut visited);
        }
    }

    fn update_neighbor_ranks(&mut self, parent: NodeIndex, direction: Direction, coefficient: i32, queue: &mut VecDeque<NodeIndex>,  visited: &mut HashSet<NodeIndex>) {
        while let Some((edge, other)) = self.graph.neighbors_directed(parent, direction).detach().next(&self.graph) {
            if !self.graph[edge].is_tree_edge || visited.contains(&other) {
                continue;
            }
            self.graph[other].rank = self.graph[parent].rank + self.minimum_length * coefficient;
            queue.push_back(other);
            visited.insert(other);
        }
    }
}

struct Feasible {
    graph: StableDiGraph<Vertex, Edge>,
    minimum_length: i32,
}

impl_slack!(Feasible);

impl Feasible {
    fn rank(mut self) {

        while let Some(edge) = self.leave_edge() {
            // swap edges and calculate cut value
            let swap_edge = self.enter_edge(edge);
            self = self.exchange(edge, swap_edge).update_cutvalues().update_low_lim().update_ranks().execute();
        }

        // don't balance ranks since we want maximum width to 
        // give indication about number of parallel processes running
        // let Self { mut graph, minimum_length } = self;

        // merge tree and graph back together
        // build layers (this also normalizes ranks)
    }

    fn leave_edge(&self) -> Option<EdgeIndex> {
        for edge in self.graph.edge_indices() {
            if let Some(cut_value) = self.graph[edge].cut_value {
                if cut_value < 0 {
                    return Some(edge);
                }
            }
        }
        None
    }

    fn enter_edge(&mut self, edge: EdgeIndex) -> EdgeIndex {
        // find a non-tree edge to replace e.
        // remove e from tree
        // consider all edges going from head to tail component.
        // choose edge with minimum slack.
        let (mut u, mut v) = self.graph.edge_endpoints(edge).map(|(t, h)| (self.graph[t], self.graph[h])).unwrap();

        if !(u.lim < v.lim) {
            std::mem::swap(&mut u, &mut v); 
        }

        self.graph.edge_indices()
            .filter(|e| !self.graph[*e].is_tree_edge)
            .filter(|e| self.is_head_to_tail(*e, u, u.lim < v.lim))
            .min_by(|e1, e2| self.slack(*e1).cmp(&self.slack(*e2)))
            .unwrap()
    }

    fn is_head_to_tail(&self, edge: EdgeIndex, u: Vertex, root_is_in_head: bool) -> bool {
        // edge needs to go from head to tail. e.g. tail neads to be in head component, and head in tail component
        let (tail, head) = self.graph.edge_endpoints(edge).map(|(t, h)| (self.graph[t], self.graph[h])).unwrap();
        // check if head is in tail component
        root_is_in_head == (u.low <= head.lim && head.lim <= u.lim) &&
        // check if tail is in head component
        root_is_in_head != (u.low <= tail.lim && tail.lim <= u.lim)
    }

    fn exchange(mut self, edge: EdgeIndex, swap_edge: EdgeIndex) -> Upd {
        // get path connecting the head and tail of swap_edge in the tree
        let (connecting_path, least_common_ancestor) = self.get_path_in_tree(swap_edge);

        // swap edges 
        self.graph[edge].is_tree_edge = false;
        self.graph[swap_edge].is_tree_edge = true;

        // destructure self, since we need to build the tree anew:
        let Self { graph, minimum_length } = self;
        Upd::new(graph, minimum_length, connecting_path, edge, least_common_ancestor)
    }

    fn get_path_in_tree(&self, edge: EdgeIndex) -> (Vec<EdgeIndex>, NodeIndex) {
        assert!(!self.graph[edge].is_tree_edge);
        let (w_id, x_id)  = self.graph.edge_endpoints(edge).unwrap();
        let (w, x) = (self.graph[w_id], self.graph[x_id]);
        let mut path_w_l = Vec::new();
        let mut path_l_x = VecDeque::new();
        // follow path back until least common ancestor is found
        // record path from w to l
        let least_common_ancestor = loop {
            let l_id = w.parent.unwrap();
            path_w_l.push(self.graph.find_edge_undirected(w_id, l_id).unwrap().0);
            let l = self.graph[l_id];
            if l.low <= w.lim && x.lim <= l.lim {
                break l_id;
            }
        };

        // record path from x to l
        loop {
            let l_id = x.parent.unwrap();
            let l = self.graph[l_id];
            if l.low <= w.lim && x.lim <= l.lim {
                assert_eq!(l_id, least_common_ancestor); // for debugging check that roots are identical
                break
            }

            path_l_x.push_front(self.graph.find_edge_undirected(x_id, l_id).unwrap().0);
        }

        (path_w_l.into_iter().chain(path_l_x.into_iter()).collect::<Vec<_>>(), least_common_ancestor)
    }
}
// ---------------------------------
// ------- OLD IMPLEMENTATION ------
// ---------------------------------

// create from input graph
pub(crate) struct UnlayeredGraph<T: Default> {
    graph: StableDiGraph<Option<T>, usize>
}

impl<T: Default> UnlayeredGraph<T> {
    pub(crate) fn initial_ranking(self, minimum_length: usize) -> TightTreeBuilder<T> {
        let mut scanned = HashSet::<(NodeIndex, NodeIndex)>::new();
        let mut ranks = HashMap::<NodeIndex, isize>::new();

        // Sort nodes topologically so we don't need to verify that we've assigned
        // a rank to all incoming neighbors
        // assume graphs contain no circles for now
        for v in petgraph::algo::toposort(&self.graph, None).unwrap() {
            self.graph.neighbors_directed(v, Incoming).for_each(|u| assert!(scanned.contains(&(u, v))));
            
            let rank = self.graph.neighbors_directed(v, Incoming)
                                 .filter_map(|n| ranks.get(&n).and_then(|r| Some(r + 1)))
                                 .max()
                                 .unwrap_or(0);

            for n in self.graph.neighbors_directed(v, Outgoing) {
                scanned.insert((v, n));
            }

            ranks.insert(v, rank);
        }

        let ranks = Ranks::new(ranks, &self.graph, minimum_length);
        TightTreeBuilder { graph: self.graph, ranks }
    }
}

pub(crate) struct TightTreeBuilder<T: Default> {
    graph: StableDiGraph<Option<T>, usize>,
    ranks: Ranks,
}

impl<T: Default> TightTreeBuilder<T> {
    #[cfg(test)]
    fn new(graph: StableDiGraph<Option<T>, usize>, ranks: Ranks) -> Self {
        Self { graph, ranks }
    }

    pub(crate) fn make_tight(mut self) -> FeasibleTreeBuilder<T> {
        // take a random edge to start the tree from
        // split edges into sets consisting of tree and non tree edges.
        // in the beginning, all edges are non tree edges, and they are added
        // with each call to dfs.

        // build a new graph which is a tree. 
        // Remember only edges which where part of the original graph
        // each time we add an edge to the tree, we remove it from the graph
        let num_nodes = self.graph.node_count();
        let mut nodes = self.graph.node_indices().into_iter();
        let mut dfs = TightTreeDFSs::new();
        
        while dfs.build_tight_tree(&self.graph, &self.ranks, nodes.next().unwrap(), &mut HashSet::new()) < num_nodes {
            let (tail, head) = self.find_non_tight_edge(&dfs);
            let mut delta = self.ranks.slack(tail, head);

            if dfs.contains_vertex(&head) {
                delta = -delta;
            }

            self.ranks.tighten_edge(&dfs, delta)
        }

        // remove all edges which are contained in tree from graph
        dfs.make_edges_disjoint(&mut self.graph);

        FeasibleTreeBuilder { graph: self.graph, ranks: self.ranks, tree: dfs.into_tree_subgraph() }
    }
    
    fn find_non_tight_edge(&self, tree: &TightTreeDFSs) -> (NodeIndex, NodeIndex) {
        self.graph.edge_indices()
            .filter_map(|e| self.graph.edge_endpoints(e))
            .filter(|(tail, head)| !tree.contains_edge(*tail, *head) && tree.is_incident_edge(tail, head))
            .min_by(|a, b| self.ranks.slack(a.0, a.1).cmp(&self.ranks.slack(b.0, b.1))).unwrap()
    }
}

pub(crate) struct FeasibleTreeBuilder<T: Default> {
    graph: StableDiGraph<Option<T>, usize>,
    ranks: Ranks,
    tree: StableDiGraph<Option<T>, usize>,
}

impl<T: Default> FeasibleTreeBuilder<T> {
    fn calculate_cutvalues(&self, mut queue: VecDeque<NodeIndex>, cut_values: &mut HashMap<(NodeIndex, NodeIndex), isize>) {
        while let Some(vertex) = queue.pop_front() {
            // terminate early if all cutvalues are known
            if cut_values.len() == self.tree.edge_count() {
                println!("done early");
                break;
            }
            let (mut cut_values_incoming, mut missing_cut_values_incoming) = 
                self.get_neighborhood_info(vertex, cut_values, Incoming); 
            let (mut cut_values_outgoing, mut missing_cut_values_outgoing) = 
                self.get_neighborhood_info(vertex, cut_values, Outgoing); 
            let (mut incoming, mut outgoing) = (Direction::Incoming, Direction::Outgoing);

            // if we can't calculate cut value yet, or the value is already known
            if missing_cut_values_incoming.len() > 1 || missing_cut_values_outgoing.len() > 1 || 
                missing_cut_values_incoming.len() == 0 && missing_cut_values_outgoing.len() == 0 {
                continue;
            } 

            // switch direction, if vertex is tail component of edge
            let edge = if missing_cut_values_outgoing.len() == 1 {
                std::mem::swap(&mut cut_values_incoming, &mut cut_values_outgoing);
                std::mem::swap(&mut missing_cut_values_incoming, &mut missing_cut_values_outgoing);
                std::mem::swap(&mut incoming, &mut outgoing);
                (vertex, missing_cut_values_incoming[0])
            } else {
                (missing_cut_values_incoming[0], vertex)
            };

            let cut_value = 1 + self.graph.neighbors_directed(vertex, incoming).count() as isize - 
                cut_values_incoming.iter().sum::<isize>() + cut_values_incoming.len() as isize - 
                self.graph.neighbors_directed(vertex, outgoing).count() as isize + 
                cut_values_outgoing.iter().sum::<isize>() - cut_values_outgoing.len() as isize;
            
            cut_values.insert(edge, cut_value);
            // continue traversing tree in direction of edge whose vertex was missing before
            queue.push_back(missing_cut_values_incoming[0]);
        }
    }

    fn update_cutvalues(self, mut cut_values: HashMap<(NodeIndex, NodeIndex), isize>, connecting_path: Vec<NodeIndex>, removed_edge: (NodeIndex, NodeIndex)) -> UpdateLowLim<T> {
        self.remove_outdated_cutvalues(&mut cut_values, connecting_path, removed_edge);
        let queue = VecDeque::from([removed_edge.0]);
        self.calculate_cutvalues(queue, &mut cut_values);
        UpdateLowLim { graph: self.graph, tree: self.tree, cut_values, ranks: self.ranks }
    }

    pub(crate) fn init_cutvalues(self) -> InitializeLowLim<T> {
        // assumes all edges have a weight of one
        let mut cut_values = HashMap::<(NodeIndex, NodeIndex), isize>::new();
        let queue = self.leaves();

        // traverse tree inward via breadth first starting from leaves
        self.calculate_cutvalues(queue, &mut cut_values);

        InitializeLowLim { graph: self.graph, tree: self.tree, ranks: self.ranks, cut_values }
    }

    fn remove_outdated_cutvalues(&self, cut_values: &mut HashMap<(NodeIndex, NodeIndex), isize>, connecting_path: Vec<NodeIndex>, removed_edge: (NodeIndex, NodeIndex)) {
        // starting from the first node, we know all adjacent cutvalues except for one.
        // thus we should be able to update every cut value efficiently by going through the path.
        // the last thing we need to do is calculate the cut value for the edge that was added.
        // remove all the cutvalues on the path:
        cut_values.remove(&removed_edge);
        for (tail, head) in connecting_path[..connecting_path.len() - 1].iter().copied().zip(connecting_path[1..].iter().copied()) {
            if cut_values.remove(&(tail, head)).is_none() {
                cut_values.remove(&(head, tail));
            }
        }
    }

    fn get_neighborhood_info(
        &self, 
        vertex: NodeIndex, 
        cut_values: &mut HashMap<(NodeIndex, NodeIndex), isize>, 
        direction: Direction
    ) -> (Vec<isize>, Vec<NodeIndex>) {
        let mut cuts = Vec::new(); 
        let mut missing = Vec::new();
        for edge in self.tree.edges_directed(vertex, direction) {
            let (tail, head) = (edge.source(), edge.target());
            if let Some(cut_value) = cut_values.get(&(tail, head)) {
                cuts.push(*cut_value);
            } else {
                missing.push(if tail == vertex { head } else { tail });
            }
        }
        (cuts, missing)
    }

    fn leaves(&self) -> VecDeque<NodeIndex> {
        self.tree.node_indices().filter(|v| self.tree.neighbors_undirected(*v).count() < 2).collect::<VecDeque<_>>()
    }
}

// TODO: make an add an extra type which is used to init low lim values.
// add extra type for after updating cutvalues
// add extra type for initializing low_lim_values
// add extra types for udpating

pub(crate) struct InitializeLowLim<T> {
    graph: StableDiGraph<Option<T>, usize>,
    tree: StableDiGraph<Option<T>, usize>,
    ranks: Ranks,
    pub cut_values: HashMap<(NodeIndex, NodeIndex), isize>,
}

impl<T: Default> InitializeLowLim<T> {
    fn initialize_low_lim(self) -> FeasibleTree<T> {
        // start at arbitrary root node
        let root = self.tree.node_indices().next().unwrap();
        let mut max_lim = self.tree.node_count();
        let mut low_lim = HashMap::new();
        self.dfs_low_lim(&mut low_lim, root, None, &mut max_lim, &mut HashSet::new());

        FeasibleTree { graph: self.graph, tree: self.tree, ranks: self.ranks, cut_values: self.cut_values, low_lim }
    }
}

impl<T> LowLimDFSS<T> for InitializeLowLim<T> {
    fn tree(&self) -> &StableDiGraph<Option<T>, usize> {
        &self.tree
    }
}
trait LowLimDFSS<T> {
    fn dfs_low_lim(&self, low_lim: &mut HashMap<NodeIndex, TreeData>, next: NodeIndex, parent: Option<NodeIndex>, max_lim: &mut usize, visited: &mut HashSet<NodeIndex>) {
        visited.insert(next);
        low_lim.entry(next).and_modify(|e| { e.lim = *max_lim; e.parent = parent; });
        for n in self.tree().neighbors_undirected(next) {
            if visited.contains(&n) {
                continue;
            }
            *max_lim -= 1;
            self.dfs_low_lim(low_lim, n, Some(next), max_lim, visited);
            low_lim.entry(n).and_modify(|e| e.low = *max_lim);
        }
    }
    fn tree(&self) -> &StableDiGraph<Option<T>, usize>;
}
struct UpdateLowLim<T> {
    graph: StableDiGraph<Option<T>, usize>,
    tree: StableDiGraph<Option<T>, usize>,
    cut_values: HashMap<(NodeIndex, NodeIndex), isize>,
    ranks: Ranks,
}

impl<T: Default> UpdateLowLim<T> {
    fn update_low_lim(self, mut low_lim: HashMap<NodeIndex, TreeData>, least_common_ancestor: NodeIndex) -> UpdateRanks<T> {
        let lca_data = *low_lim.get(&least_common_ancestor).unwrap();
        let mut visited = match lca_data.parent {
            Some(parent) => HashSet::from([parent]),
            None => HashSet::new()
        };
        let mut max_lim = lca_data.lim;
        self.dfs_low_lim(&mut low_lim, least_common_ancestor, lca_data.parent, &mut max_lim, &mut visited);
        UpdateRanks { graph: self.graph, tree: self.tree, cut_values: self.cut_values, low_lim, ranks: self.ranks }
    }
}

impl<T> LowLimDFSS<T> for UpdateLowLim<T> {
    fn tree(&self) -> &StableDiGraph<Option<T>, usize> {
        &self.tree
    }
}

struct UpdateRanks<T> {
    graph: StableDiGraph<Option<T>, usize>,
    tree: StableDiGraph<Option<T>, usize>,
    cut_values: HashMap<(NodeIndex, NodeIndex), isize>,
    low_lim: HashMap<NodeIndex, TreeData>,
    ranks: Ranks
}

impl<T: Default> UpdateRanks<T> {
    fn update_ranks(self) -> FeasibleTree<T> {
        let node = self.tree.node_identifiers().next().unwrap();
        let mut new_ranks = HashMap::from([(node, 0)]);
        // start at arbitrary node and traverse the tree
        let mut queue = VecDeque::from([self.tree.node_identifiers().next().unwrap()]);
        let minimum_length = self.ranks.get_minimum_length() as isize;

        while let Some(parent) = queue.pop_front() {
            for n in self.tree.neighbors_directed(parent, Incoming) {
                if new_ranks.contains_key(&n) {
                    continue;
                }
                new_ranks.insert(n, new_ranks.get(&parent).unwrap() - minimum_length);
                queue.push_back(n);
            }

            for n in self.tree.neighbors_directed(parent, Outgoing) {
                if new_ranks.contains_key(&n) {
                    continue;
                }
                new_ranks.insert(n, new_ranks.get(&parent).unwrap() + minimum_length);
                queue.push_back(n);
            }
        }
        let updated_ranks = Ranks::new(new_ranks, &self.tree, self.ranks.get_minimum_length());
        FeasibleTree { graph: self.graph, tree: self.tree, ranks: updated_ranks, cut_values: self.cut_values, low_lim: self.low_lim }
    }
}

pub(crate) struct FeasibleTree<T: Default> {
    graph: StableDiGraph<Option<T>, usize>,
    tree: StableDiGraph<Option<T>, usize>,
    ranks: Ranks,
    pub cut_values: HashMap<(NodeIndex, NodeIndex), isize>,
    low_lim: HashMap<NodeIndex, TreeData>,
}

impl<T: Default> FeasibleTree<T> {
    fn rank(mut self) -> ProperLayeredGraph<T> {

        while let Some(edge) = self.leave_edge() {
            // swap edges and calculate cut value
            let swap_edge = self.enter_edge(edge);
            self = self.exchange(edge, swap_edge);
        }

        // don't balance ranks since we want maximum width to 
        // give indication about number of parallel processes running
        let Self {mut graph, tree, ranks, ..} = self;

        // merge tree and graph back together
        for edge in tree.edge_indices() {
            let (tail, head) = tree.edge_endpoints(edge).unwrap();
            graph.add_edge(tail, head, usize::default());
        }
        drop(tree);
        // build layers (this also normalizes ranks)
        let layers: Layers = ranks.into_layers(&graph);

        ProperLayeredGraph::new(layers, graph)
    }

    fn leave_edge(&self) -> Option<(NodeIndex, NodeIndex)> {
        for (edge, cut_value) in self.cut_values.iter() {
            if cut_value < &0 {
                return Some(*edge);
            }
        }
        None
    }

    fn enter_edge(&mut self, edge: (NodeIndex, NodeIndex)) -> (NodeIndex, NodeIndex) {
        // find a non-tree edge to replace e.
        // remove e from tree
        // consider all edges going from head to tail component.
        // choose edge with minimum slack.
        let (u, v) = edge;
        let mut u = *self.low_lim.get(&u).unwrap();
        let mut v = *self.low_lim.get(&v).unwrap();
        if !(u.lim < v.lim) {
            std::mem::swap(&mut u, &mut v); 
        }

        self.graph.edge_indices()
            .filter_map(|e| self.graph.edge_endpoints(e))
            .filter(|(tail, head)| { 
                self.is_head_to_tail(*tail, *head, u, u.lim < v.lim)
            })
            .min_by(|(tail_a, head_a), (tail_b, head_b)| self.ranks.slack(*tail_a, *head_a).cmp(&self.ranks.slack(*tail_b, *head_b)))
            .unwrap()
    }

    fn exchange(mut self, edge: (NodeIndex, NodeIndex), swap_edge: (NodeIndex, NodeIndex)) -> Self {
        // get path connecting the head and tail of swap_edge in the tree
        let (connecting_path, least_common_ancestor) = self.get_path_in_tree(swap_edge);

        // swap edges 
        self.tree.remove_edge(self.tree.find_edge(edge.0, edge.1).unwrap());
        self.tree.add_edge(swap_edge.0, swap_edge.1, usize::default());
        self.graph.remove_edge(self.graph.find_edge(swap_edge.0, swap_edge.1).unwrap());
        // is it a good idea to add the edge that was removed back to the graph or should we keep a separate list of removed edges?
        self.graph.add_edge(edge.0, edge.1, usize::default()); 

        // destructure self, since we need to build the tree anew:
        let Self { graph, tree, ranks, cut_values, low_lim } = self;
        FeasibleTreeBuilder { graph, ranks, tree }.update_cutvalues(cut_values, connecting_path, edge)
            .update_low_lim(low_lim, least_common_ancestor)
            .update_ranks()
    }

    fn get_path_in_tree(&self, edge: (NodeIndex, NodeIndex)) -> (Vec<NodeIndex>, NodeIndex) {
        let (w, x) = edge;
        let w_data = self.low_lim.get(&w).unwrap();
        let x_data = self.low_lim.get(&x).unwrap();
        let mut path_w_l = VecDeque::new();
        let mut path_l_x = VecDeque::new();
        // follow path back until least common ancestor is found
        // record path from w to l
        let least_common_ancestor = loop {
            let l = w_data.parent.unwrap();
            path_w_l.push_back(l);
            let l_data = self.low_lim.get(&l).unwrap();
            if l_data.low <= w_data.lim && x_data.lim <= l_data.lim {
                break l;
            }
        };

        // record path from x to l
        loop {
            let l = x_data.parent.unwrap();
            let l_data = self.low_lim.get(&l).unwrap();
            if l_data.low <= w_data.lim && x_data.lim <= l_data.lim {
                assert_eq!(l, least_common_ancestor); // for debugging check that roots are identical
                break
            }

            path_l_x.push_front(l);
        }

        path_w_l.append(&mut path_l_x);

        (path_w_l.into_iter().collect::<Vec<_>>(), least_common_ancestor)
    }

    fn is_head_to_tail(&self, tail: NodeIndex, head: NodeIndex, u: TreeData, root_is_in_head: bool) -> bool {
        // edge needs to go from head to tail. e.g. tail neads to be in head component, and head in tail component
        let tail = self.low_lim.get(&tail).unwrap();
        let head = self.low_lim.get(&head).unwrap();
        // check if head is in tail component
        root_is_in_head == (u.low <= head.lim && head.lim <= u.lim) &&
        // check if tail is in head component
        root_is_in_head != (u.low <= tail.lim && tail.lim <= u.lim)
    }

}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct TreeData {
    lim: usize,
    low: usize,
    parent: Option<NodeIndex>
}

impl TreeData {
    #[cfg(test)]
    fn new(lim: usize, low: usize, parent: Option<NodeIndex>) -> Self {
        Self { lim, low, parent }
    }
}

impl Default for TreeData {
    fn default() -> Self {
        TreeData{ lim: 0, low: 0, parent: None }
    }
}

