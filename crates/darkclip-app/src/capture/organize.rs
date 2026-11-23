//! Organisation commands on the actor thread: collections, favorites, tags and notes
//! (Task 402). The actor owns the only writable `Storage` (D-028).

use super::actor::Actor;
use super::{OrganizeOp, OrganizeReply};
use crate::events::{map_storage, ApiError};
use darkclip_core::CollectionSort;

impl Actor {
    pub(super) fn organize(&mut self, op: OrganizeOp) -> Result<OrganizeReply, ApiError> {
        let st = &mut self.storage;
        let done = |r: Result<(), darkclip_storage::StorageError>| {
            r.map(|()| OrganizeReply::Done).map_err(|e| map_storage(&e))
        };
        match op {
            OrganizeOp::CreateCollection(name) => st
                .create_collection(&name, CollectionSort::Manual, None)
                .map(OrganizeReply::Collection)
                .map_err(|e| map_storage(&e)),
            OrganizeOp::RenameCollection(id, name) => done(st.rename_collection(&id, &name)),
            OrganizeOp::DeleteCollection(id) => done(st.delete_collection(&id)),
            OrganizeOp::AddToCollection(col, clip) => done(st.add_to_collection(&col, &clip)),
            OrganizeOp::RemoveFromCollection(col, clip) => {
                done(st.remove_from_collection(&col, &clip))
            }
            OrganizeOp::Reorder(col, clip, index) => {
                done(st.move_in_collection(&col, &clip, index))
            }
            OrganizeOp::SetFavorite(clip, on) => {
                let fav = st.favorites_id().map_err(|e| map_storage(&e))?;
                if on {
                    done(st.add_to_collection(&fav, &clip))
                } else {
                    done(st.remove_from_collection(&fav, &clip))
                }
            }
            OrganizeOp::SetNote(clip, note) => done(st.set_note(&clip, note.as_deref())),
            OrganizeOp::SetTags(clip, tags) => st
                .set_clip_tags(&clip, &tags)
                .map(OrganizeReply::Tags)
                .map_err(|e| map_storage(&e)),
        }
    }
}
