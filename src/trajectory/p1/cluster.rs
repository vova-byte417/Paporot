//! P1 Cluster: DBSCAN-like density clustering + similarity grouping。
//!
//! Deterministic, no random seed. Based on cosine distance on P1 TrajectoryVector.

use crate::trajectory::p1::vector::TrajectoryVector;

/// 聚类结果。
#[derive(Debug, Clone)]
pub struct ClusterResult {
    /// 聚类标签（-1 = noise/未分配）
    pub labels: Vec<i32>,
    /// 聚类数量（不含 noise）
    pub cluster_count: usize,
    /// 每个 cluster 的成员索引
    pub clusters: Vec<Vec<usize>>,
    /// 相似度矩阵（vector_index → vector_index → similarity）
    pub similarity_matrix: Vec<Vec<f32>>,
}

/// DBSCAN-like density clusterer（deterministic）。
#[derive(Debug, Clone)]
pub struct Clusterer {
    /// 邻域距离阈值（cosine distance: 1 - cosine_similarity）
    pub eps: f32,
    /// 邻域最小点数（包括自身）才能形成核心点
    pub min_points: usize,
}

impl Default for Clusterer {
    fn default() -> Self {
        Clusterer {
            eps: 0.3,       // cosine distance < 0.3 → similar
            min_points: 3,  // at least 3 points form a cluster
        }
    }
}

impl Clusterer {
    pub fn new(eps: f32, min_points: usize) -> Self {
        Clusterer { eps, min_points }
    }

    /// 对一组 TrajectoryVector 执行聚类。
    pub fn cluster(&self, vectors: &[TrajectoryVector]) -> ClusterResult {
        let n = vectors.len();
        if n == 0 {
            return ClusterResult {
                labels: vec![],
                cluster_count: 0,
                clusters: vec![],
                similarity_matrix: vec![],
            };
        }

        // Compute similarity matrix
        let sim_matrix = self.compute_similarity_matrix(vectors);

        // Compute distance matrix: dist = 1 - similarity
        let dist_matrix: Vec<Vec<f32>> = sim_matrix
            .iter()
            .map(|row| row.iter().map(|s| 1.0 - s).collect())
            .collect();

        // Find neighbors for each point
        let neighbors: Vec<Vec<usize>> = (0..n)
            .map(|i| {
                (0..n)
                    .filter(|&j| i != j && dist_matrix[i][j] <= self.eps)
                    .collect()
            })
            .collect();

        // DBSCAN algorithm
        let mut labels = vec![-1_i32; n]; // -1 = unvisited/noise
        let mut current_cluster = 0_i32;

        for i in 0..n {
            if labels[i] != -1 {
                continue; // already assigned
            }

            if neighbors[i].len() < self.min_points - 1 {
                // Not a core point, mark as noise for now
                // (may be assigned later as border point)
                continue;
            }

            // Expand cluster from core point
            current_cluster += 1;
            labels[i] = current_cluster;
            let mut seed_set: Vec<usize> = neighbors[i].clone();

            let mut ptr = 0;
            while ptr < seed_set.len() {
                let j = seed_set[ptr];
                ptr += 1;

                if labels[j] == -1 {
                    labels[j] = current_cluster;
                } else if labels[j] > 0 {
                    continue; // already in a cluster
                }

                if neighbors[j].len() >= self.min_points - 1 {
                    // j is also a core point, add its neighbors
                    for &k in &neighbors[j] {
                        if labels[k] == -1 {
                            labels[k] = current_cluster;
                            seed_set.push(k);
                        }
                    }
                }
            }
        }

        // Organize clusters
        let mut clusters: Vec<Vec<usize>> = vec![vec![]; current_cluster as usize + 1];
        for (i, &label) in labels.iter().enumerate() {
            if label > 0 {
                clusters[label as usize].push(i);
            }
        }
        // Remove empty slot at index 0
        let non_empty: Vec<Vec<usize>> = clusters
            .into_iter()
            .filter(|c| !c.is_empty())
            .collect();
        let cluster_count = non_empty.len();

        ClusterResult {
            labels,
            cluster_count,
            clusters: non_empty,
            similarity_matrix: sim_matrix,
        }
    }

    /// 基于相似度矩阵做简单分组（不依赖密度）。
    /// threshold: 相似度 ≥ threshold 归为一组。
    pub fn similarity_group(
        vectors: &[TrajectoryVector],
        threshold: f32,
    ) -> Vec<Vec<usize>> {
        let n = vectors.len();
        if n == 0 {
            return vec![];
        }

        let sim_matrix = Self::compute_similarity_matrix_static(vectors);

        // Greedy grouping: group all pairs with similarity ≥ threshold
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut assigned = vec![false; n];

        for i in 0..n {
            if assigned[i] {
                continue;
            }
            let mut group = vec![i];
            assigned[i] = true;

            for j in (i + 1)..n {
                if assigned[j] {
                    continue;
                }
                // Check if j is similar to all members in the group
                let all_similar = group.iter().all(|&member| {
                    sim_matrix[member][j] >= threshold
                });
                if all_similar {
                    group.push(j);
                    assigned[j] = true;
                }
            }
            groups.push(group);
        }

        groups
    }

    fn compute_similarity_matrix(&self, vectors: &[TrajectoryVector]) -> Vec<Vec<f32>> {
        Self::compute_similarity_matrix_static(vectors)
    }

    fn compute_similarity_matrix_static(vectors: &[TrajectoryVector]) -> Vec<Vec<f32>> {
        let n = vectors.len();
        let mut matrix = vec![vec![0.0_f32; n]; n];

        for i in 0..n {
            matrix[i][i] = 1.0;
            for j in (i + 1)..n {
                let sim = crate::trajectory::p1::vector::cosine_similarity(
                    &vectors[i].to_scalar_vec(),
                    &vectors[j].to_scalar_vec(),
                );
                matrix[i][j] = sim;
                matrix[j][i] = sim;
            }
        }
        matrix
    }

    /// 计算聚类质量分数（平均 intra-cluster similarity - inter-cluster similarity）。
    pub fn cluster_quality(&self, result: &ClusterResult) -> f32 {
        if result.clusters.len() < 2 {
            return 1.0; // single cluster = perfect (trivially)
        }

        let sim = &result.similarity_matrix;

        let mut intra_sum = 0.0_f32;
        let mut intra_count = 0;
        let mut inter_sum = 0.0_f32;
        let mut inter_count = 0;

        for cluster in &result.clusters {
            // Intra-cluster similarity
            for i in 0..cluster.len() {
                for j in (i + 1)..cluster.len() {
                    intra_sum += sim[cluster[i]][cluster[j]];
                    intra_count += 1;
                }
            }
        }

        // Inter-cluster similarity
        for ci in 0..result.clusters.len() {
            for cj in (ci + 1)..result.clusters.len() {
                for &i in &result.clusters[ci] {
                    for &j in &result.clusters[cj] {
                        inter_sum += sim[i][j];
                        inter_count += 1;
                    }
                }
            }
        }

        let intra_avg = if intra_count > 0 {
            intra_sum / intra_count as f32
        } else {
            1.0
        };
        let inter_avg = if inter_count > 0 {
            inter_sum / inter_count as f32
        } else {
            0.0
        };

        intra_avg - inter_avg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_vector(entropy: f32, loop_r: f32, backtrack: f32) -> TrajectoryVector {
        TrajectoryVector {
            tool_entropy: entropy,
            phase_entropy: entropy * 0.8,
            transition_entropy: entropy * 0.6,
            loop_ratio: loop_r,
            backtrack_ratio: backtrack,
            burst_ratio: 0.1,
            state_stability_score: 0.7,
            edit_intensity_curve: vec![],
            ..Default::default()
        }
    }

    #[test]
    fn test_cluster_empty() {
        let clusterer = Clusterer::default();
        let result = clusterer.cluster(&[]);
        assert_eq!(result.cluster_count, 0);
        assert!(result.labels.is_empty());
    }

    #[test]
    fn test_cluster_all_noise() {
        let vectors = vec![
            make_vector(0.1, 0.1, 0.1),
            make_vector(0.9, 0.9, 0.9),
        ];
        let clusterer = Clusterer {
            eps: 0.1,
            min_points: 3,
        };
        let result = clusterer.cluster(&vectors);
        // With min_points=3 and only 2 vectors, all should be noise
        let noise_count = result.labels.iter().filter(|&&l| l <= 0).count();
        assert_eq!(noise_count, 2);
    }

    #[test]
    fn test_similarity_group() {
        let vectors = vec![
            make_vector(0.1, 0.1, 0.1),
            make_vector(0.12, 0.11, 0.09), // similar to vec 0
            make_vector(0.9, 0.9, 0.9), // different
        ];
        let groups = Clusterer::similarity_group(&vectors, 0.8);
        // First two should be in the same group
        assert!(groups.len() >= 2);
        assert!(groups.iter().any(|g| g.contains(&0) && g.contains(&1)));
    }

    #[test]
    fn test_cluster_quality() {
        let vectors = vec![
            make_vector(0.1, 0.1, 0.1),
            make_vector(0.11, 0.12, 0.08),
            make_vector(0.9, 0.9, 0.9),
            make_vector(0.88, 0.91, 0.92),
        ];
        let clusterer = Clusterer {
            eps: 0.15,
            min_points: 2,
        };
        let result = clusterer.cluster(&vectors);
        let quality = clusterer.cluster_quality(&result);
        // Quality should be positive (intra > inter)
        assert!(quality >= 0.0);
    }
}
