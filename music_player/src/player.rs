//! Audio playback engine, a thin wrapper around `rodio`.
//!
//! [`Player`] owns the OS audio stream and a single `Sink` (rodio's playback
//! queue). The UI never touches rodio directly; it calls [`Player::play`],
//! [`Player::pause`], [`Player::seek`] and friends.
//!
//! Seeking is the tricky part: rodio has no native "jump to position", so we
//! decode and discard samples up to the target. That is slow, so it runs on a
//! background thread. An atomic "operation id" makes sure that if the user
//! seeks again before the first seek finishes, the stale thread quietly drops
//! its result instead of starting playback from the wrong place.

use rodio::{Decoder, OutputStream, Sink};
use std::fs::File;
use std::io::BufReader;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Owns the audio output and exposes simple transport controls.
pub struct Player {
    /// Output stream; kept alive for the player's lifetime (dropping it stops
    /// all audio). Underscored because it is never read directly.
    _stream: OutputStream,
    /// Stream handle, retained alongside the stream. Never read directly.
    _stream_handle: rodio::OutputStreamHandle,
    /// The playback queue. `Arc` so background seek threads can append to it.
    sink: Arc<Sink>,
    /// Monotonically increasing id identifying the most recent transport
    /// operation. Background seeks compare against this to detect that they
    /// have been superseded and should abort. See [`Player::seek`].
    current_operation_id: Arc<AtomicU64>,
}

impl Player {
    /// Initializes the default audio device and a sink at 50% volume.
    ///
    /// # Panics
    /// Panics if no audio output device is available or the sink cannot be
    /// created — without audio the player has nothing to do.
    pub fn new() -> Self {
        let (stream, handle) =
            OutputStream::try_default().expect("❌ Failed to initialize audio output device");
        let sink = Sink::try_new(&handle).expect("❌ Failed to create audio sink");
        sink.set_volume(0.5);

        println!("🔊 Audio system ready.");

        Self {
            _stream: stream,
            _stream_handle: handle,
            sink: Arc::new(sink),
            current_operation_id: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Loads and starts playing the file at `path`, replacing whatever was
    /// playing. Returns the track's total duration when it can be determined.
    ///
    /// Duration comes from rodio when available; for MP3s that report `None`,
    /// we fall back to the `mp3-duration` crate. Returns `None` if the file
    /// cannot be opened or decoded.
    pub fn play(&self, path: &str) -> Option<Duration> {
        // Invalidate any in-flight background seek so it does not resurrect the
        // previous track on top of this one.
        self.current_operation_id.fetch_add(1, Ordering::SeqCst);

        self.sink.stop();
        println!("▶️ Loading track: {}", path);

        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                println!("❌ Failed to open file: {:?}", e);
                return None;
            }
        };

        let source = match Decoder::new(BufReader::new(file)) {
            Ok(s) => s,
            Err(e) => {
                println!("❌ Failed to decode audio: {:?}", e);
                return None;
            }
        };

        use rodio::Source;
        // Prefer rodio's reported duration; fall back to mp3-duration for MP3s
        // where rodio returns `None`.
        let duration = source
            .total_duration()
            .or_else(|| mp3_duration::from_path(path).ok());

        println!("📊 Decoded successfully. Duration: {:?}", duration);
        self.sink.append(source);
        self.sink.play();
        duration
    }

    /// Pauses playback (position is preserved).
    pub fn pause(&self) {
        self.sink.pause();
    }

    /// Resumes playback after a pause.
    pub fn resume(&self) {
        self.sink.play();
    }

    /// Sets output volume in the range `0.0..=1.0`.
    pub fn set_volume(&self, volume: f32) {
        self.sink.set_volume(volume);
    }

    /// Seeks `path` to `position` by decoding and discarding leading samples.
    ///
    /// The heavy decode loop runs on its own OS thread so the GUI stays
    /// responsive. The sink is stopped immediately for instant feedback, then
    /// the background thread re-appends the source positioned at `position` —
    /// but only if no newer transport operation has happened in the meantime
    /// (tracked via [`Self::current_operation_id`]). A superseded thread exits
    /// without touching the sink.
    pub fn seek(&self, path: &str, position: Duration) {
        // Claim a unique id for this seek. `fetch_add` returns the previous
        // value, so our id is that + 1.
        let op_id = self.current_operation_id.fetch_add(1, Ordering::SeqCst) + 1;

        // Stop the old audio right away so the player reacts instantly.
        self.sink.stop();

        // Clone the Arcs so the background thread can use them safely.
        let sink_clone = Arc::clone(&self.sink);
        let id_clone = Arc::clone(&self.current_operation_id);
        let path_clone = path.to_string();

        thread::spawn(move || {
            let Ok(file) = File::open(&path_clone) else {
                return;
            };
            let Ok(mut source) = Decoder::new(BufReader::new(file)) else {
                return;
            };

            use rodio::Source;
            let sample_rate = source.sample_rate();
            let channels = source.channels();
            let secs = position.as_secs_f32();
            let samples_to_skip = (secs * sample_rate as f32 * channels as f32) as usize;

            // Discard samples up to the target position. This runs in parallel
            // and does not block the UI thread.
            for _ in 0..samples_to_skip {
                let _ = source.next();
            }

            // Only start playing if this is still the most recent operation.
            // If the user seeked again, `id_clone` will have advanced and this
            // now-stale thread simply returns.
            if id_clone.load(Ordering::SeqCst) == op_id {
                sink_clone.append(source);
                sink_clone.play();
            }
        });
    }
}
