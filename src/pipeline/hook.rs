use std::path::{Path, PathBuf};

use anyhow::Result;

/// A hook invoked between encoding and decoding in a [`roundtrip`](super::roundtrip).
///
/// Implement this trait to inject custom logic between the encode and decode
/// steps — for example, uploading the encoded video to YouTube and downloading
/// it back before decoding.
///
/// # Example
///
/// ```rust
/// use std::path::{Path, PathBuf};
/// use anyhow::Result;
/// use yts3::PipelineHook;
///
/// struct YoutubeHook;
///
/// impl PipelineHook for YoutubeHook {
///     fn after_encode(&self, encoded_path: &Path) -> Result<PathBuf> {
///         // upload encoded_path to YouTube ...
///         // download it back to a local file ...
///         // return the local path of the downloaded copy
///         Ok(encoded_path.to_path_buf()) // placeholder
///     }
/// }
/// ```
pub trait PipelineHook {
    /// Called after encoding completes. `encoded_path` is the local path of the
    /// freshly written `.mkv` file. Return the path the decoder should read from —
    /// this may be the same file, or a locally-downloaded copy after a remote round-trip.
    fn after_encode(&self, encoded_path: &Path) -> Result<PathBuf>;
}

/// A no-op hook that passes the encoded path through unchanged.
///
/// Used as the default when no intermediate steps are needed.
pub struct NoopHook;

impl PipelineHook for NoopHook {
    fn after_encode(&self, encoded_path: &Path) -> Result<PathBuf> {
        Ok(encoded_path.to_path_buf())
    }
}
