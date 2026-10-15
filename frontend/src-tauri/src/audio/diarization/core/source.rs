//! The audio-window seam (05b D4): what the shared core reads its audio from.
//!
//! The core segments and embeds windows of 16 kHz audio; where those windows
//! come from is the only thing a driver has to supply. The batch drivers read
//! them from a saved recording (an ffmpeg pipe, or samples already in memory),
//! and the live driver's source is the VAD-filtered chunks a recording
//! delivers. Keeping that one difference behind this trait is what lets a test
//! assert that the core does not care which driver it is serving.

use std::borrow::Cow;

/// A window of 16 kHz audio and its absolute start, in seconds, in recording time.
pub(crate) type Window<'a> = (f64, Cow<'a, [f32]>);

pub(crate) trait AudioSource {
    /// The next 16 kHz window with its absolute start in recording time, or
    /// `Ok(None)` at the end of the audio.
    ///
    /// An error ends the run instead of ending the audio: a source that
    /// swallowed an I/O failure would present a truncated recording as a
    /// complete one, and every speaker after the cut would silently be missing.
    fn next_window(&mut self) -> Result<Option<Window<'_>>, String>;
}
