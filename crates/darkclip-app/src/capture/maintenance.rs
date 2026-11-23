//! Data-management commands on the actor thread (Task 411): delete old clips, remove orphaned
//! blob files, compact the database. The actor owns the only writable `Storage` (D-028).

use super::actor::Actor;
use super::{MaintOp, MaintReply};
use crate::events::{map_storage, ApiError};

impl Actor {
    pub(super) fn maintain(&mut self, op: MaintOp) -> Result<MaintReply, ApiError> {
        let st = &mut self.storage;
        match op {
            MaintOp::DeleteOlderThan(days) => st
                .delete_older_than(days)
                .map(MaintReply::Deleted)
                .map_err(|e| map_storage(&e)),
            MaintOp::SweepOrphans => st
                .sweep_orphans()
                .map(MaintReply::Orphans)
                .map_err(|e| map_storage(&e)),
            MaintOp::Compact(free_bytes) => st
                .compact(free_bytes)
                .map(MaintReply::Compacted)
                .map_err(|e| map_storage(&e)),
        }
    }
}
