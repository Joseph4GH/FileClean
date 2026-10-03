pub mod audio;
pub mod exact;
pub mod perceptual;
pub mod raw_pair;

/// 并查集：用于把「相似」这种传递关系收敛成互不相交的分组。
#[derive(Debug)]
pub struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    pub fn new(n: usize) -> Self {
        UnionFind {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    pub fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            // 路径减半，保持近 O(1) 的摊还成本。
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    pub fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        match self.rank[ra].cmp(&self.rank[rb]) {
            std::cmp::Ordering::Less => self.parent[ra] = rb,
            std::cmp::Ordering::Greater => self.parent[rb] = ra,
            std::cmp::Ordering::Equal => {
                self.parent[rb] = ra;
                self.rank[ra] += 1;
            }
        }
    }

    /// 输出所有大小 >= `min_size` 的集合。
    pub fn groups(&mut self, min_size: usize) -> Vec<Vec<usize>> {
        use std::collections::HashMap;
        let n = self.parent.len();
        let mut buckets: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..n {
            let root = self.find(i);
            buckets.entry(root).or_default().push(i);
        }
        let mut out: Vec<Vec<usize>> = buckets
            .into_values()
            .filter(|v| v.len() >= min_size)
            .collect();
        for g in &mut out {
            g.sort_unstable();
        }
        out.sort_by_key(|g| g[0]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::UnionFind;

    #[test]
    fn merges_transitively() {
        let mut uf = UnionFind::new(5);
        uf.union(0, 1);
        uf.union(1, 2);
        uf.union(3, 4);
        let groups = uf.groups(2);
        assert_eq!(groups, vec![vec![0, 1, 2], vec![3, 4]]);
    }

    #[test]
    fn drops_singletons() {
        let mut uf = UnionFind::new(3);
        uf.union(0, 1);
        assert_eq!(uf.groups(2), vec![vec![0, 1]]);
    }
}
