//! `PermissionGuard`: runs first in every `ClipApi` method (ai-integration §1).

use crate::api::Caller;
use crate::events::ApiError;

/// One variant per `ClipApi` method (plus `clear_history`, UI-facing only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    SearchClips,
    GetClip,
    ListCollections,
    CreateCollection,
    AddClipToCollection,
    UpdateClipNote,
    UpdateClipTags,
    PinClip,
    CopyClipToClipboard,
    DeleteClip,
    GetStorageStats,
    ExportCollection,
    SetCaptureMode,
    ClearHistory,
}

pub struct PermissionGuard;

impl PermissionGuard {
    pub fn check(caller: &Caller, op: Operation) -> Result<(), ApiError> {
        match caller {
            Caller::Ui => Ok(()),
            // P2: replaced by grant checks (ADR-0005)
            Caller::Cli => Ok(()),
            // P2: replaced by grant checks (ADR-0005)
            Caller::Mcp { .. } => match op {
                Operation::GetStorageStats => Ok(()),
                _ => Err(ApiError::PermissionDenied("no grant for this client")),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Operation; 14] = [
        Operation::SearchClips,
        Operation::GetClip,
        Operation::ListCollections,
        Operation::CreateCollection,
        Operation::AddClipToCollection,
        Operation::UpdateClipNote,
        Operation::UpdateClipTags,
        Operation::PinClip,
        Operation::CopyClipToClipboard,
        Operation::DeleteClip,
        Operation::GetStorageStats,
        Operation::ExportCollection,
        Operation::SetCaptureMode,
        Operation::ClearHistory,
    ];

    #[test]
    fn guard_rules() {
        let mcp = Caller::Mcp {
            client_id: "claude-code".into(),
        };
        for op in ALL {
            assert!(PermissionGuard::check(&Caller::Ui, op).is_ok());
            assert!(PermissionGuard::check(&Caller::Cli, op).is_ok());
            let r = PermissionGuard::check(&mcp, op);
            if op == Operation::GetStorageStats {
                assert!(r.is_ok());
            } else {
                assert!(matches!(r, Err(ApiError::PermissionDenied(_))), "{op:?}");
            }
        }
    }
}
