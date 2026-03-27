use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Partition {
    Objective,
    History,
    DecisionLog,
    Background,
    Reserve,
}

impl Partition {
    pub fn target_pct(self) -> f32 {
        match self {
            Self::Objective => 0.10,
            Self::History => 0.40,
            Self::DecisionLog => 0.20,
            Self::Background => 0.20,
            Self::Reserve => 0.10,
        }
    }

    pub fn all() -> [Partition; 5] {
        [
            Partition::Objective,
            Partition::History,
            Partition::DecisionLog,
            Partition::Background,
            Partition::Reserve,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetItem {
    pub id: String,
    pub token_count: usize,
    pub added_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub importance: f32,
    pub summarized: bool,
}

impl BudgetItem {
    pub fn new(id: impl Into<String>, token_count: usize, importance: f32) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            token_count,
            added_at: now,
            last_accessed: now,
            importance: importance.clamp(0.0, 1.0),
            summarized: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartitionBudget {
    pub target_pct: f32,
    pub current_tokens: usize,
    pub max_tokens: usize,
    pub items: Vec<BudgetItem>,
}

impl PartitionBudget {
    fn new(total_capacity: usize, partition: Partition) -> Self {
        let target_pct = partition.target_pct();
        Self {
            target_pct,
            current_tokens: 0,
            max_tokens: ((total_capacity as f32) * target_pct).round() as usize,
            items: Vec::new(),
        }
    }

    fn recalc(&mut self) {
        self.current_tokens = self.items.iter().map(|item| item.token_count).sum();
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenBudget {
    pub total_capacity: usize,
    pub partitions: BTreeMap<Partition, PartitionBudget>,
    pub last_gc: Option<DateTime<Utc>>,
    pub last_checkpoint: Option<DateTime<Utc>>,
}

impl TokenBudget {
    pub fn new(total_capacity: usize) -> Self {
        let mut partitions = BTreeMap::new();
        for partition in Partition::all() {
            partitions.insert(partition, PartitionBudget::new(total_capacity, partition));
        }
        Self {
            total_capacity,
            partitions,
            last_gc: None,
            last_checkpoint: None,
        }
    }

    pub fn total_used(&self) -> usize {
        self.partitions.values().map(|p| p.current_tokens).sum()
    }

    pub fn usage_pct(&self) -> f32 {
        if self.total_capacity == 0 {
            return 0.0;
        }
        self.total_used() as f32 / self.total_capacity as f32
    }

    pub fn add_item(&mut self, partition: Partition, item: BudgetItem) {
        if let Some(bucket) = self.partitions.get_mut(&partition) {
            bucket.items.push(item);
            bucket.recalc();
        }
    }

    pub fn create_checkpoint(
        &mut self,
        status: impl Into<String>,
        key_decisions: Vec<String>,
        next_step: impl Into<String>,
    ) -> Checkpoint {
        let timestamp = Utc::now();
        self.last_checkpoint = Some(timestamp);
        Checkpoint {
            status: status.into(),
            key_decisions,
            next_step: next_step.into(),
            partition_snapshot: self
                .partitions
                .iter()
                .map(|(partition, budget)| (*partition, budget.current_tokens))
                .collect(),
            timestamp,
        }
    }

    pub fn clear_summarized_outputs(&mut self) -> usize {
        let mut freed = 0;
        for partition in [
            Partition::DecisionLog,
            Partition::Background,
            Partition::Reserve,
        ] {
            if let Some(bucket) = self.partitions.get_mut(&partition) {
                let before = bucket.current_tokens;
                bucket.items.retain(|item| !item.summarized);
                bucket.recalc();
                freed += before.saturating_sub(bucket.current_tokens);
            }
        }
        freed
    }

    pub fn compact_old_history(&mut self, keep_recent: usize) -> usize {
        let Some(bucket) = self.partitions.get_mut(&Partition::History) else {
            return 0;
        };
        if bucket.items.len() <= keep_recent {
            return 0;
        }
        let remove_count = bucket.items.len() - keep_recent;
        let freed: usize = bucket.items[..remove_count]
            .iter()
            .map(|i| i.token_count)
            .sum();
        bucket.items.drain(..remove_count);
        bucket.recalc();
        freed
    }

    pub fn compact_background(&mut self) -> usize {
        let Some(bucket) = self.partitions.get_mut(&Partition::Background) else {
            return 0;
        };
        let target = bucket.max_tokens;
        if bucket.current_tokens <= target {
            return 0;
        }
        bucket
            .items
            .sort_by(|a, b| a.importance.partial_cmp(&b.importance).unwrap());
        let mut freed = 0;
        while bucket.current_tokens > target && !bucket.items.is_empty() {
            let removed = bucket.items.remove(0);
            freed += removed.token_count;
            bucket.recalc();
        }
        freed
    }

    pub fn mark_gc(&mut self) {
        self.last_gc = Some(Utc::now());
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub status: String,
    pub key_decisions: Vec<String>,
    pub next_step: String,
    pub partition_snapshot: BTreeMap<Partition, usize>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartitionReport {
    pub name: Partition,
    pub target_pct: f32,
    pub current_pct: f32,
    pub token_count: usize,
    pub item_count: usize,
    pub oldest_item: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetReport {
    pub total_capacity: usize,
    pub total_used: usize,
    pub usage_pct: f32,
    pub partitions: Vec<PartitionReport>,
    pub last_gc: Option<DateTime<Utc>>,
    pub last_checkpoint: Option<DateTime<Utc>>,
}

impl From<&TokenBudget> for BudgetReport {
    fn from(budget: &TokenBudget) -> Self {
        let partitions = budget
            .partitions
            .iter()
            .map(|(name, partition)| PartitionReport {
                name: *name,
                target_pct: partition.target_pct,
                current_pct: if budget.total_capacity == 0 {
                    0.0
                } else {
                    partition.current_tokens as f32 / budget.total_capacity as f32
                },
                token_count: partition.current_tokens,
                item_count: partition.items.len(),
                oldest_item: partition.items.iter().map(|item| item.added_at).min(),
            })
            .collect();

        Self {
            total_capacity: budget.total_capacity,
            total_used: budget.total_used(),
            usage_pct: budget.usage_pct(),
            partitions,
            last_gc: budget.last_gc,
            last_checkpoint: budget.last_checkpoint,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GcResult {
    NoAction,
    Compacted { freed_tokens: usize },
}

pub fn heartbeat_gc(
    budget: &mut TokenBudget,
    status: impl Into<String>,
    key_decisions: Vec<String>,
    next_step: impl Into<String>,
) -> (Checkpoint, GcResult) {
    let checkpoint = budget.create_checkpoint(status, key_decisions, next_step);
    if budget.usage_pct() < 0.80 {
        return (checkpoint, GcResult::NoAction);
    }

    let mut freed = 0;
    freed += budget.clear_summarized_outputs();
    freed += budget.compact_old_history(5);
    if budget.usage_pct() > 0.80 {
        freed += budget.compact_background();
    }
    budget.mark_gc();
    (
        checkpoint,
        GcResult::Compacted {
            freed_tokens: freed,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partition_targets_match_issue_spec() {
        assert_eq!(Partition::Objective.target_pct(), 0.10);
        assert_eq!(Partition::History.target_pct(), 0.40);
        assert_eq!(Partition::DecisionLog.target_pct(), 0.20);
        assert_eq!(Partition::Background.target_pct(), 0.20);
        assert_eq!(Partition::Reserve.target_pct(), 0.10);
    }

    #[test]
    fn checkpoint_captures_partition_snapshot() {
        let mut budget = TokenBudget::new(1000);
        budget.add_item(Partition::Objective, BudgetItem::new("goal", 50, 1.0));
        let checkpoint = budget.create_checkpoint("working", vec!["picked rust".into()], "ship pr");
        assert_eq!(
            checkpoint.partition_snapshot.get(&Partition::Objective),
            Some(&50)
        );
        assert_eq!(checkpoint.status, "working");
        assert_eq!(checkpoint.next_step, "ship pr");
    }

    #[test]
    fn heartbeat_gc_preserves_recent_history_and_clears_summarized_items() {
        let mut budget = TokenBudget::new(100);
        for idx in 0..8 {
            budget.add_item(
                Partition::History,
                BudgetItem::new(format!("h{idx}"), 10, 0.5),
            );
        }
        let mut summarized = BudgetItem::new("bg", 15, 0.1);
        summarized.summarized = true;
        budget.add_item(Partition::Background, summarized);

        let (_checkpoint, gc) = heartbeat_gc(&mut budget, "busy", vec![], "continue");
        match gc {
            GcResult::Compacted { freed_tokens } => assert!(freed_tokens >= 45),
            GcResult::NoAction => panic!("expected compaction"),
        }
        assert_eq!(budget.partitions[&Partition::History].items.len(), 5);
        assert_eq!(budget.partitions[&Partition::Background].items.len(), 0);
        assert!(budget.last_gc.is_some());
    }

    #[test]
    fn budget_report_exposes_partition_usage() {
        let mut budget = TokenBudget::new(1000);
        budget.add_item(Partition::Reserve, BudgetItem::new("tool-output", 100, 0.2));
        let report = BudgetReport::from(&budget);
        assert_eq!(report.total_used, 100);
        assert_eq!(report.partitions.len(), 5);
        assert!(
            report
                .partitions
                .iter()
                .any(|partition| partition.name == Partition::Reserve
                    && partition.token_count == 100)
        );
    }
}
