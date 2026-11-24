use crate::t_common;
use crate::t_sqlite::{self, Face, Person};
use rand::seq::SliceRandom;
use std::collections::HashMap;

/// Clustering progress information
#[derive(Clone, serde::Serialize)]
pub struct ClusterProgress {
    pub phase: String, // "graph", "iterate", "converged", "assign", "thumbnail"
    pub current: usize,
    pub total: usize,
}

/// Calculate cosine distance between two PRE-PARSED embeddings
/// Distance = 1.0 - Cosine Similarity
/// NOTE: Assumes input vectors are already normalized!
fn cosine_distance(emb1: &[f32], emb2: &[f32]) -> f32 {
    // Dot product of normalized vectors = cosine similarity
    let dot_product: f32 = emb1.iter().zip(emb2.iter()).map(|(x, y)| x * y).sum();

    // Clamp similarity to [-1.0, 1.0] to handle floating point errors
    let similarity = dot_product.clamp(-1.0, 1.0);
    1.0 - similarity
}

/// Helper: Parse raw byte embedding to normalized f32 vector
fn parse_embedding(bytes: &[u8]) -> Option<Vec<f32>> {
    // Embedding bytes should be tightly packed f32 values.
    // If not, skip this embedding instead of panicking.
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return None;
    }
    let emb_vec: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|chunk| {
            let arr = [chunk[0], chunk[1], chunk[2], chunk[3]];
            f32::from_le_bytes(arr)
        })
        .collect();

    // Normalize
    let norm: f32 = emb_vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        Some(emb_vec.iter().map(|x| x / norm).collect())
    } else {
        None
    }
}

/// Edge in the similarity graph
#[derive(Clone)]
struct Edge {
    to: usize,   // Target node index
    weight: f32, // Edge weight (similarity = 1 - distance)
}

/// Insert edge into candidate list, maintaining at most max_edges entries sorted by weight descending.
/// If list is full and new weight is smaller than the smallest, it is dropped.
fn insert_top_k(candidates: &mut Vec<(usize, f32)>, edge: (usize, f32), max_edges: usize) {
    if candidates.len() < max_edges {
        candidates.push(edge);
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    } else {
        // Find the smallest weight (last element after sort)
        if let Some(&(_, min_weight)) = candidates.last() {
            if edge.1 > min_weight {
                candidates.pop();
                candidates.push(edge);
                candidates
                    .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            }
        }
    }
}

/// Input to the pure clustering step.
pub(crate) struct ClusterNode {
    pub file_id: i64,
    pub embedding: Option<Vec<f32>>,
    /// Person this face is confirmed as (fixed label that never changes).
    pub confirmed_person: Option<i64>,
    /// Persons this face must never be assigned to.
    pub rejected: Vec<i64>,
    /// Unreliable embedding: excluded from forming clusters, attached afterwards
    /// only on a clear match (ignored for confirmed faces).
    pub low_quality: bool,
}

/// Output of the pure clustering step (indices into the input nodes).
#[derive(Debug, Default)]
pub(crate) struct ClusterOutcome {
    /// Unconfirmed nodes that joined an existing person.
    pub matched: Vec<(usize, i64)>,
    /// Groups of unconfirmed nodes that form new persons.
    pub new_clusters: Vec<Vec<usize>>,
}

/// Cluster faces over a thresholded K-NN graph, growing confirmed persons.
///
/// Unconfirmed faces are grouped with Chinese Whispers; each face or cluster linked
/// to a person's confirmed faces joins that person, and the remaining clusters
/// become new persons. Rejected (face, person) pairs are never assigned, and a
/// person is kept at most once per photo.
pub(crate) fn run_clustering<C>(
    nodes: &[ClusterNode],
    threshold: f32,
    mut progress_fn: impl FnMut(ClusterProgress),
    is_cancelled_fn: C,
) -> Option<ClusterOutcome>
where
    C: Fn() -> bool,
{
    const K_NEIGHBORS: usize = t_common::K_NEIGHBORS;
    let n = nodes.len();

    // 1. Build K-NN Graph with early Top-K pruning (memory bounded at N * K_NEIGHBORS)
    let mut candidate_lists: Vec<Vec<(usize, f32)>> = vec![Vec::new(); n];
    let total_pairs = n.saturating_mul(n.saturating_sub(1)) / 2;
    let mut pairs_done: usize = 0;
    let mut last_pct = 0;

    for i in 0..n {
        if is_cancelled_fn() {
            return None;
        }

        if let Some(emb_i) = &nodes[i].embedding {
            for j in (i + 1)..n {
                pairs_done += 1;
                // Faces in the same file cannot be edges (prevents merging distinct people in same photo)
                if nodes[i].file_id == nodes[j].file_id {
                    continue;
                }
                // Two confirmed faces never need an edge: neither label can change.
                if nodes[i].confirmed_person.is_some() && nodes[j].confirmed_person.is_some() {
                    continue;
                }
                // Low-quality faces never link others together (they bridge different people).
                if is_deferred(&nodes[i]) || is_deferred(&nodes[j]) {
                    continue;
                }
                if let Some(emb_j) = &nodes[j].embedding {
                    let dist = cosine_distance(emb_i, emb_j);
                    if dist < threshold {
                        let weight = 1.0 - dist;
                        // Square weight to punish weak links further
                        let adjusted_weight = weight * weight;
                        insert_top_k(&mut candidate_lists[i], (j, adjusted_weight), K_NEIGHBORS);
                        insert_top_k(&mut candidate_lists[j], (i, adjusted_weight), K_NEIGHBORS);
                    }
                }
            }
        } else {
            pairs_done += n.saturating_sub(i + 1);
        }

        let current_pct = if total_pairs > 0 { pairs_done * 100 / total_pairs } else { 100 };
        if current_pct >= last_pct + 5 || pairs_done == total_pairs {
            progress_fn(ClusterProgress {
                phase: "graph".to_string(),
                current: current_pct,
                total: 100,
            });
            last_pct = current_pct;
        }
    }

    let graph: Vec<Vec<Edge>> = candidate_lists
        .into_iter()
        .map(|candidates| {
            candidates
                .into_iter()
                .map(|(to, weight)| Edge { to, weight })
                .collect()
        })
        .collect();

    // 2. Chinese Whispers among unconfirmed faces (confirmed faces don't vote here,
    //    otherwise a tight group of unconfirmed faces would always outvote a single
    //    confirmed seed and known people would never grow).
    let mut labels: Vec<usize> = (0..n).collect();
    let mut order: Vec<usize> = (0..n)
        .filter(|&i| nodes[i].confirmed_person.is_none() && !is_deferred(&nodes[i]))
        .collect();
    let mut rng = rand::thread_rng();
    let max_iterations = 20;

    for iter in 0..max_iterations {
        if is_cancelled_fn() {
            return None;
        }
        progress_fn(ClusterProgress {
            phase: "iterate".to_string(),
            current: iter + 1,
            total: max_iterations,
        });

        let mut changed = false;
        order.shuffle(&mut rng);

        for &node in &order {
            let mut label_weights: HashMap<usize, f32> = HashMap::new();
            for edge in &graph[node] {
                if nodes[edge.to].confirmed_person.is_none() {
                    *label_weights.entry(labels[edge.to]).or_insert(0.0) += edge.weight;
                }
            }
            let Some(best_label) = label_weights
                .into_iter()
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(label, _)| label)
            else {
                continue;
            };
            if labels[node] != best_label {
                labels[node] = best_label;
                changed = true;
            }
        }

        if !changed {
            progress_fn(ClusterProgress {
                phase: "converged".to_string(),
                current: iter + 1,
                total: max_iterations,
            });
            break;
        }
    }

    if is_cancelled_fn() {
        return None;
    }

    // 3. Link unconfirmed faces to confirmed persons. A face with direct edges to a
    //    person's confirmed faces goes to the strongest such person; otherwise it
    //    follows its cluster's strongest person link. Rejected pairs are skipped.
    let best_of = |weights: &HashMap<i64, f32>, rejected: &[i64]| -> Option<i64> {
        weights
            .iter()
            .filter(|(pid, _)| !rejected.contains(pid))
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal).then(b.0.cmp(a.0)))
            .map(|(&pid, _)| pid)
    };
    let mut direct: Vec<HashMap<i64, f32>> = vec![HashMap::new(); n];
    let mut cluster_links: HashMap<usize, HashMap<i64, f32>> = HashMap::new();
    for i in 0..n {
        if nodes[i].confirmed_person.is_some() {
            continue;
        }
        for edge in &graph[i] {
            if let Some(pid) = nodes[edge.to].confirmed_person {
                if nodes[i].rejected.contains(&pid) {
                    continue;
                }
                *direct[i].entry(pid).or_insert(0.0) += edge.weight;
                *cluster_links.entry(labels[i]).or_default().entry(pid).or_insert(0.0) += edge.weight;
            }
        }
    }
    let tentative: Vec<Option<i64>> = (0..n)
        .map(|i| {
            if nodes[i].confirmed_person.is_some() {
                return None;
            }
            best_of(&direct[i], &nodes[i].rejected).or_else(|| {
                cluster_links
                    .get(&labels[i])
                    .and_then(|links| best_of(links, &nodes[i].rejected))
            })
        })
        .collect();
    let person_of = |j: usize| nodes[j].confirmed_person.or(tentative[j]);

    // A person appears at most once per photo: confirmed faces hold their slot, and
    // otherwise the face most strongly connected to the person's faces wins.
    let mut occupied: std::collections::HashSet<(i64, i64)> = nodes
        .iter()
        .filter_map(|node| node.confirmed_person.map(|pid| (node.file_id, pid)))
        .collect();
    let mut person_candidates: Vec<(usize, i64, f32)> = (0..n)
        .filter_map(|i| {
            let pid = tentative[i]?;
            let support: f32 = graph[i]
                .iter()
                .filter(|e| person_of(e.to) == Some(pid))
                .map(|e| e.weight)
                .sum();
            Some((i, pid, support))
        })
        .collect();
    person_candidates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));

    let mut outcome = ClusterOutcome::default();
    let mut assigned = vec![false; n];
    let mut suppressed_duplicate = vec![false; n];
    for (i, pid, _) in person_candidates {
        if occupied.insert((nodes[i].file_id, pid)) {
            outcome.matched.push((i, pid));
            assigned[i] = true;
        } else {
            // Same person already appears in this photo (e.g. a photo-booth collage);
            // keep the strongest match and ignore the duplicate copy rather than
            // letting it fall through and create a new person.
            suppressed_duplicate[i] = true;
        }
    }

    // Also suppress unconfirmed faces that closely match an already-assigned face in
    // the SAME photo (e.g. a collage copy wearing sunglasses that had no cross-photo
    // edges). Same-file edges are skipped in step 1 so distinct people aren't merged,
    // which means collage duplicates can otherwise look like isolated new people.
    let same_photo_dup_threshold = threshold.min(0.50);
    let mut by_file: HashMap<i64, Vec<usize>> = HashMap::new();
    for i in 0..n {
        by_file.entry(nodes[i].file_id).or_default().push(i);
    }
    for indices in by_file.values() {
        if indices.len() < 2 {
            continue;
        }
        for &i in indices {
            if nodes[i].confirmed_person.is_some()
                || assigned[i]
                || suppressed_duplicate[i]
                || is_deferred(&nodes[i])
            {
                continue;
            }
            let Some(emb_i) = &nodes[i].embedding else { continue };
            let is_dup_of_assigned = indices.iter().any(|&j| {
                if j == i {
                    return false;
                }
                let Some(pid_j) = nodes[j].confirmed_person.or_else(|| {
                    if assigned[j] { tentative[j] } else { None }
                }) else {
                    return false;
                };
                if nodes[i].rejected.contains(&pid_j) {
                    return false;
                }
                nodes[j]
                    .embedding
                    .as_deref()
                    .is_some_and(|emb_j| cosine_distance(emb_i, emb_j) < same_photo_dup_threshold)
            });
            if is_dup_of_assigned {
                suppressed_duplicate[i] = true;
            }
        }
    }

    // 4. Remaining unconfirmed faces form new persons by cluster. A cluster can still
    //    gather two faces of one photo through other faces, so keep only the member most
    //    connected to the rest of its cluster in each photo.
    let mut cluster_map: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        if nodes[i].confirmed_person.is_none()
            && !assigned[i]
            && !suppressed_duplicate[i]
            && !is_deferred(&nodes[i])
        {
            cluster_map.entry(labels[i]).or_default().push(i);
        }
    }
    let cluster_support = |i: usize| -> f32 {
        graph[i]
            .iter()
            .filter(|e| labels[e.to] == labels[i] && nodes[e.to].confirmed_person.is_none())
            .map(|e| e.weight)
            .sum()
    };
    for members in cluster_map.values_mut() {
        let mut best_in_file: HashMap<i64, (usize, f32)> = HashMap::new();
        for &i in members.iter() {
            let s = cluster_support(i);
            let entry = best_in_file.entry(nodes[i].file_id).or_insert((i, s));
            if s > entry.1 || (s == entry.1 && i < entry.0) {
                *entry = (i, s);
            }
        }
        members.retain(|&i| best_in_file[&nodes[i].file_id].0 == i);
    }
    const MIN_SAMPLES: usize = t_common::MIN_SAMPLES;
    let mut clusters: Vec<Vec<usize>> = cluster_map
        .into_values()
        .filter(|members| members.len() >= MIN_SAMPLES)
        .collect();
    // Largest first, so "Person 1" is the most frequent new face.
    clusters.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));

    // Suppress duplicate copies of the same unconfirmed person within a single collage
    // photo across separate new clusters (keeping the highest-priority cluster's copy).
    let mut kept_in_file: HashMap<i64, Vec<usize>> = HashMap::new();
    for members in &mut clusters {
        members.retain(|&i| {
            let is_dup = nodes[i].embedding.as_deref().is_some_and(|emb_i| {
                kept_in_file
                    .get(&nodes[i].file_id)
                    .is_some_and(|kept| {
                        kept.iter().any(|&j| {
                            nodes[j]
                                .embedding
                                .as_deref()
                                .is_some_and(|emb_j| cosine_distance(emb_i, emb_j) < same_photo_dup_threshold)
                        })
                    })
            });
            if is_dup {
                false
            } else {
                kept_in_file.entry(nodes[i].file_id).or_default().push(i);
                true
            }
        });
    }
    clusters.retain(|members| members.len() >= MIN_SAMPLES);
    outcome.new_clusters = clusters;

    // 5. Attach low-quality faces to the group they clearly match; leave the rest
    //    unassigned instead of creating one-photo persons.
    attach_low_quality(nodes, threshold, &mut occupied, &mut outcome);

    Some(outcome)
}

/// Unconfirmed low-quality face: kept out of graph building and Chinese Whispers.
fn is_deferred(node: &ClusterNode) -> bool {
    node.low_quality && node.confirmed_person.is_none()
}

/// Group a face ended up in after clustering.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Group {
    Person(i64),
    NewCluster(usize),
}

/// Assign each deferred face to its nearest group when that group is within a
/// stricter threshold and clearly closer than the next-best group.
fn attach_low_quality(
    nodes: &[ClusterNode],
    threshold: f32,
    occupied: &mut std::collections::HashSet<(i64, i64)>,
    outcome: &mut ClusterOutcome,
) {
    let mut group_of: Vec<Option<Group>> = nodes.iter().map(|node| node.confirmed_person.map(Group::Person)).collect();
    for &(i, pid) in &outcome.matched {
        group_of[i] = Some(Group::Person(pid));
    }
    for (k, members) in outcome.new_clusters.iter().enumerate() {
        for &i in members {
            group_of[i] = Some(Group::NewCluster(k));
        }
    }
    let members: Vec<(usize, &Vec<f32>, Group)> = nodes
        .iter()
        .enumerate()
        .filter_map(|(j, node)| Some((j, node.embedding.as_ref()?, group_of[j]?)))
        .collect();

    let mut cluster_files: std::collections::HashSet<(i64, usize)> = outcome
        .new_clusters
        .iter()
        .enumerate()
        .flat_map(|(k, ms)| ms.iter().map(move |&i| (nodes[i].file_id, k)))
        .collect();

    let max_distance = threshold * t_common::LOW_QUALITY_MATCH_FACTOR;
    let mut candidates: Vec<(usize, Group, f32)> = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        if !is_deferred(node) {
            continue;
        }
        let Some(emb) = &node.embedding else { continue };
        let mut best_per_group: HashMap<Group, f32> = HashMap::new();
        for &(j, other, group) in &members {
            if nodes[j].file_id == node.file_id {
                continue;
            }
            if let Group::Person(pid) = group {
                if node.rejected.contains(&pid) {
                    continue;
                }
            }
            let sim = 1.0 - cosine_distance(emb, other);
            let best = best_per_group.entry(group).or_insert(f32::MIN);
            if sim > *best {
                *best = sim;
            }
        }
        let mut ranked: Vec<(Group, f32)> = best_per_group.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let Some(&(group, best)) = ranked.first() else { continue };
        let second = ranked.get(1).map_or(f32::MIN, |r| r.1);
        if 1.0 - best < max_distance && best - second >= t_common::LOW_QUALITY_MATCH_MARGIN {
            candidates.push((i, group, best));
        }
    }

    // Strongest matches claim the once-per-photo slot first.
    candidates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    for (i, group, _) in candidates {
        let file_id = nodes[i].file_id;
        match group {
            Group::Person(pid) => {
                if occupied.insert((file_id, pid)) {
                    outcome.matched.push((i, pid));
                }
            }
            Group::NewCluster(k) => {
                if cluster_files.insert((file_id, k)) {
                    outcome.new_clusters[k].push(i);
                }
            }
        }
    }
}

/// Re-cluster faces while keeping every user-confirmed assignment.
///
/// Automatic assignments and auto-created persons are rebuilt from scratch; confirmed
/// faces keep their person and pull similar unconfirmed faces into it.
pub fn cluster_faces<F, C>(
    threshold: f32,
    kind: t_sqlite::PersonKind,
    mut progress_fn: F,
    is_cancelled_fn: C,
) -> Result<usize, String>
where
    F: FnMut(ClusterProgress),
    C: Fn() -> bool,
{
    let mut conn = t_sqlite::open_conn()?;

    // 1. Repair persons with several faces in one photo, then drop automatic
    //    assignments (confirmed faces keep their person)
    Person::dedupe_all_per_photo(&conn)?;
    Face::reset_unconfirmed_assignments_with_conn(&mut conn, kind)?;

    // 2. Load faces and rejections
    let faces = Face::get_all_for_clustering_with_conn(&conn, kind)?;
    if faces.is_empty() {
        return Ok(0);
    }
    let mut rejections: HashMap<i64, Vec<i64>> = HashMap::new();
    for (face_id, person_id) in Face::get_rejections_with_conn(&conn)? {
        rejections.entry(face_id).or_default().push(person_id);
    }

    let face_ids: Vec<i64> = faces.iter().map(|f| f.id).collect();
    let nodes: Vec<ClusterNode> = faces
        .into_iter()
        .map(|face| ClusterNode {
            file_id: face.file_id,
            embedding: face.embedding.as_deref().and_then(parse_embedding),
            confirmed_person: face.person_id.filter(|_| face.confirmed),
            rejected: rejections.remove(&face.id).unwrap_or_default(),
            low_quality: face.low_quality,
        })
        .collect();

    let Some(outcome) = run_clustering(&nodes, threshold, &mut progress_fn, &is_cancelled_fn) else {
        return Ok(0);
    };
    drop(nodes);

    // 3. Persist assignments
    let total_clusters = outcome.new_clusters.len();
    progress_fn(ClusterProgress {
        phase: "assign".to_string(),
        current: 0,
        total: total_clusters,
    });
    let matched: Vec<(i64, i64)> = outcome
        .matched
        .iter()
        .map(|&(i, pid)| (face_ids[i], pid))
        .collect();
    let new_clusters: Vec<Vec<i64>> = outcome
        .new_clusters
        .iter()
        .map(|members| members.iter().map(|&i| face_ids[i]).collect())
        .collect();
    let total_assigned = Face::apply_auto_assignments_with_conn(&mut conn, kind, &matched, &new_clusters)?;
    progress_fn(ClusterProgress {
        phase: "assign".to_string(),
        current: total_clusters,
        total: total_clusters,
    });

    // 9. Generate thumbnails
    progress_fn(ClusterProgress {
        phase: "thumbnail".to_string(),
        current: 0,
        total: total_clusters,
    });

    Person::update_all_thumbnails()?;

    progress_fn(ClusterProgress {
        phase: "thumbnail".to_string(),
        current: total_clusters,
        total: total_clusters,
    });

    Ok(total_assigned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(v: [f32; 4]) -> Option<Vec<f32>> {
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        Some(v.iter().map(|x| x / norm).collect())
    }

    fn node(file_id: i64, emb: [f32; 4], confirmed: Option<i64>, rejected: Vec<i64>) -> ClusterNode {
        ClusterNode { file_id, embedding: unit(emb), confirmed_person: confirmed, rejected, low_quality: false }
    }

    fn run(nodes: &[ClusterNode]) -> ClusterOutcome {
        run_clustering(nodes, 0.42, |_| {}, || false).unwrap()
    }

    #[test]
    fn confirmed_faces_pull_similar_faces_into_their_person() {
        let nodes = vec![
            node(1, [1.0, 0.0, 0.0, 0.0], Some(7), vec![]),
            node(2, [0.95, 0.1, 0.0, 0.0], None, vec![]),
            node(3, [0.9, 0.15, 0.05, 0.0], None, vec![]),
            node(4, [0.0, 0.0, 1.0, 0.0], None, vec![]),
            node(5, [0.0, 0.05, 0.95, 0.0], None, vec![]),
        ];
        let mut outcome = run(&nodes);
        outcome.matched.sort();
        assert_eq!(outcome.matched, vec![(1, 7), (2, 7)]);
        assert_eq!(outcome.new_clusters.len(), 1);
        let mut cluster = outcome.new_clusters[0].clone();
        cluster.sort();
        assert_eq!(cluster, vec![3, 4]);
    }

    #[test]
    fn cluster_follows_its_link_to_a_confirmed_person() {
        // Face 2 is too far from the confirmed seed for a direct edge, but it clusters
        // with face 1, which is linked to the seed: the whole cluster joins person 7.
        let nodes = vec![
            node(1, [1.0, 0.0, 0.0, 0.0], Some(7), vec![]),
            node(2, [0.9, 0.3, 0.0, 0.0], None, vec![]),
            node(3, [0.4, 0.9, 0.0, 0.0], None, vec![]),
        ];
        let mut outcome = run(&nodes);
        outcome.matched.sort();
        assert_eq!(outcome.matched, vec![(1, 7), (2, 7)]);
        assert!(outcome.new_clusters.is_empty());
    }

    fn low(file_id: i64, emb: [f32; 4]) -> ClusterNode {
        ClusterNode { low_quality: true, ..node(file_id, emb, None, vec![]) }
    }

    #[test]
    fn low_quality_face_cannot_bridge_two_people() {
        // A and B are 60 degrees apart (no edge); the low-quality face sits between
        // them, close to both. It must not merge them, and as an ambiguous match it
        // stays unassigned rather than becoming its own person.
        let (c60, s60) = (0.5f32, 0.866f32);
        let (c30, s30) = (0.866f32, 0.5f32);
        let nodes = vec![
            node(1, [1.0, 0.0, 0.0, 0.0], None, vec![]),
            node(2, [0.99, 0.05, 0.0, 0.0], None, vec![]),
            node(3, [c60, s60, 0.0, 0.0], None, vec![]),
            node(4, [c60 - 0.02, s60 + 0.02, 0.0, 0.0], None, vec![]),
            low(5, [c30, s30, 0.0, 0.0]),
        ];
        let outcome = run(&nodes);
        let mut clusters: Vec<Vec<usize>> = outcome
            .new_clusters
            .iter()
            .map(|c| { let mut c = c.clone(); c.sort(); c })
            .collect();
        clusters.sort();
        assert_eq!(clusters, vec![vec![0, 1], vec![2, 3]]);
        assert!(outcome.matched.is_empty());
    }

    #[test]
    fn low_quality_face_joins_a_clear_match_and_is_otherwise_left_out() {
        let nodes = vec![
            node(1, [1.0, 0.0, 0.0, 0.0], Some(7), vec![]),
            low(2, [0.95, 0.2, 0.0, 0.0]),
            low(3, [0.0, 0.0, 1.0, 0.0]),
            // Close enough for a normal edge, but not within the stricter low-quality limit.
            low(4, [0.6, 0.0, 0.0, 0.8]),
        ];
        let outcome = run(&nodes);
        assert_eq!(outcome.matched, vec![(1, 7)]);
        assert!(outcome.new_clusters.is_empty());
    }

    #[test]
    fn rejected_person_is_never_assigned() {
        let nodes = vec![
            node(1, [1.0, 0.0, 0.0, 0.0], Some(7), vec![]),
            node(2, [0.97, 0.05, 0.0, 0.0], None, vec![7]),
        ];
        let outcome = run(&nodes);
        assert!(outcome.matched.is_empty());
        assert_eq!(outcome.new_clusters, vec![vec![1]]);
    }

    #[test]
    fn person_is_matched_at_most_once_per_photo() {
        let nodes = vec![
            node(1, [1.0, 0.0, 0.0, 0.0], Some(7), vec![]),
            node(2, [0.99, 0.02, 0.0, 0.0], None, vec![]),
            node(2, [0.9, 0.2, 0.0, 0.0], None, vec![]),
            // Another confirmed photo of person 7 that already occupies file 3.
            node(3, [0.98, 0.0, 0.05, 0.0], Some(7), vec![]),
            node(3, [0.97, 0.04, 0.0, 0.0], None, vec![]),
        ];
        let outcome = run(&nodes);
        assert_eq!(outcome.matched, vec![(1, 7)]);
        // Duplicate copies of person 7 in files 2 and 3 must be ignored, not turned into new people.
        assert!(outcome.new_clusters.is_empty());
    }

    #[test]
    fn collage_duplicates_of_same_person_are_ignored_instead_of_creating_new_people() {
        // Photo-booth collage (file 10) with:
        // - 3 copies of confirmed person 7 (one confirmed, one identical, one slightly shifted like sunglasses)
        // - 2 copies of an unconfirmed person who only appears in this collage
        // - 1 distinct third person
        let nodes = vec![
            node(10, [1.0, 0.0, 0.0, 0.0], Some(7), vec![]),
            node(10, [0.98, 0.1, 0.0, 0.0], None, vec![]),
            node(10, [0.85, 0.5, 0.0, 0.0], None, vec![]),
            node(10, [0.0, 0.0, 1.0, 0.0], None, vec![]),
            node(10, [0.0, 0.0, 0.96, 0.15], None, vec![]),
            node(10, [0.0, 0.0, 0.0, 1.0], None, vec![]),
        ];
        let outcome = run(&nodes);
        assert!(outcome.matched.is_empty());
        // Should keep only 1 copy of the unconfirmed collage person and 1 copy of the distinct person.
        assert_eq!(outcome.new_clusters, vec![vec![3], vec![5]]);
    }
}
