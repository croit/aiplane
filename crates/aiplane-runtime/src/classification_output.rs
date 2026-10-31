//! Marking what a conversation generates.
//!
//! Every file a tool, the sandbox or an MCP spill adds to a conversation goes
//! through [`upload_stamped`], which marks it with the conversation's level
//! before it is stored. The architecture test
//! `tool_crates_upload_through_the_stamping_helper` keeps it the only way in.

use std::sync::Arc;

use aiplane_core::server::classification::{self, ClassificationConfig, Scheme};
use aiplane_core::server::config::S3Config;
use aiplane_features::server::chat_attachments::{self, AttachmentError, UploadOutcome};
use aiplane_features::server::classification::{StampOptions, stamp_output};

use crate::server::tools::ToolContext;

/// The scheme and policy a turn stamps its output with, snapshotted when the
/// turn starts.
#[derive(Clone)]
pub struct TurnClassification {
    pub scheme: Arc<Scheme>,
    pub config: Arc<ClassificationConfig>,
}

impl TurnClassification {
    fn options(&self) -> StampOptions {
        StampOptions {
            visible: self.config.output.visible,
            metadata: self.config.output.metadata,
            filename: self.config.output.filename,
            mip_tenant_id: self.config.mip_tenant_id.clone(),
        }
    }
}

/// Store one generated file in the conversation, marked with the
/// conversation's level. The returned outcome names the file as stored: the
/// file name tag can change it, so callers must use `outcome.filename`.
///
/// With one level, outside a conversation, or at the lowest level the bytes
/// are stored exactly as given. A file that cannot be rewritten is stored
/// with the file name tag only.
pub async fn upload_stamped(
    ctx: &ToolContext,
    s3: &S3Config,
    turn_id: &str,
    filename: &str,
    mime: &str,
    bytes: Vec<u8>,
) -> Result<UploadOutcome, AttachmentError> {
    let (Some(policy), Some(session_id)) = (
        ctx.classification
            .as_ref()
            .filter(|policy| policy.scheme.is_active()),
        ctx.session_id.as_deref(),
    ) else {
        return chat_attachments::upload(s3, turn_id, filename, mime, bytes).await;
    };
    let level = classification::level_or_highest(&ctx.db, &policy.scheme, session_id).await;
    let options = policy.options();
    let stamped = match stamp_output(filename, mime, &bytes, level, &policy.scheme, &options) {
        Ok(stamped) => stamped,
        Err(error) => {
            tracing::warn!(
                filename,
                level = %level.id,
                error = %error,
                "generated file delivered with the file name tag only"
            );
            let tag_only = StampOptions {
                visible: false,
                metadata: false,
                ..options
            };
            match stamp_output(filename, mime, &bytes, level, &policy.scheme, &tag_only) {
                Ok(stamped) => stamped,
                Err(_) => {
                    return chat_attachments::upload(s3, turn_id, filename, mime, bytes).await;
                }
            }
        }
    };
    chat_attachments::upload(s3, turn_id, &stamped.filename, mime, stamped.bytes).await
}
