//! Composing a plugin's face, off the thread that draws.
//!
//! A face is not a lookup: it shapes three lines of text (each one stepping its
//! size down until it fits), clones the picture per band, and re-encodes a JPEG
//! every time the number moves. On a page with six gauges that is a burst of work
//! every sample, and the sample arrives on the window's own tick — which is
//! exactly where a 90 ms hitch becomes a visible stutter.
//!
//! So the work happens here, on a thread of its own, and the window only paints
//! what arrives. The thread owns its own [`TextEngine`]: building one reads the
//! system's fonts, which `cosmic-text` documents as slow, and a second database
//! costs one scan and a few megabytes in exchange for never shaping on the UI
//! thread. Titles keep the window's engine; readings never touch it.
//!
//! A job carries everything the face needs — the cell, the reading, the font
//! family, the deck's image size — because the worker cannot read the
//! controller. The answer carries the bytes *and* the pixels: the bytes go to
//! the deck, and the pixels are what the window's own tile draws, so the two can
//! never show different faces.

use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::thread::JoinHandle;

use deck_api::ImageSpec;
use image::RgbaImage;
use ss550_plugin_api::{CellId, PluginId, Reading};

use crate::plugintile;
use crate::titletext::TextEngine;

/// How many faces may be waiting for the worker.
///
/// A page is at most a few dozen keys, and a sample queues only the ones that
/// moved. The bound is what keeps a burst of samples from building a backlog: a
/// job that does not fit is dropped, and the key stays due for the next sample.
const QUEUE: usize = 8;

/// One face to compose.
pub(crate) struct FaceJob {
    /// The key's number, so the answer knows where it goes.
    pub(crate) key: u16,
    /// The plugin and cell the key is bound to, so an answer that arrives after
    /// a rebind can be refused instead of painting the wrong face.
    pub(crate) plugin: PluginId,
    pub(crate) cell: CellId,
    /// The cell's description: its label and accent.
    pub(crate) spec: ss550_plugin_api::CellSpec,
    /// What the cell measured.
    pub(crate) reading: Reading,
    /// The family the window draws titles in, so a face matches them.
    pub(crate) family: String,
    /// The deck's tile size, which is what makes the payload a payload.
    pub(crate) image: ImageSpec,
}

/// One composed face.
pub(crate) struct FaceDone {
    pub(crate) key: u16,
    pub(crate) plugin: PluginId,
    pub(crate) cell: CellId,
    /// The reading it was drawn from, so the repaint rule remembers exactly what
    /// went out.
    pub(crate) reading: Reading,
    /// The pixels, for the window's tile.
    pub(crate) picture: RgbaImage,
    /// The deck's payload, or why it could not be made.
    pub(crate) jpeg: Result<Vec<u8>, String>,
}

/// The face-composing thread and the channel back.
pub(crate) struct FaceWorker {
    /// `None` once the worker was told to stop, which is what closes the channel
    /// its loop is waiting on.
    jobs: Option<SyncSender<FaceJob>>,
    done: Receiver<FaceDone>,
    thread: Option<JoinHandle<()>>,
}
impl FaceWorker {
    /// Starts the worker.
    ///
    /// A thread that cannot start is not a broken window: every key keeps the
    /// face it has, and `is_alive` is what the controller asks before telling the
    /// user why nothing moves.
    pub(crate) fn start() -> Self {
        let (jobs, job_rx) = mpsc::sync_channel::<FaceJob>(QUEUE);
        let (done_tx, done) = mpsc::channel::<FaceDone>();

        let thread = std::thread::Builder::new()
            .name("plugin-face".to_string())
            .spawn(move || run(job_rx, done_tx))
            .ok();

        Self {
            jobs: Some(jobs),
            done,
            thread,
        }
    }

    /// Whether the thread is alive.
    pub(crate) fn is_alive(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }

    /// Queues one face, answering whether it was accepted.
    ///
    /// `false` when the queue is full or the worker is gone: the key keeps its
    /// old face and stays due, because nothing was painted and nothing was
    /// remembered as painted.
    pub(crate) fn submit(&self, job: FaceJob) -> bool {
        let Some(jobs) = &self.jobs else {
            return false;
        };
        match jobs.try_send(job) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
        }
    }

    /// Takes every face the worker has finished.
    pub(crate) fn try_recv(&self) -> Option<FaceDone> {
        self.done.try_recv().ok()
    }
}

impl Drop for FaceWorker {
    fn drop(&mut self) {
        // The sender goes first: the worker's loop is waiting on it, so joining
        // while it still exists would wait for a thread that was never told to
        // leave. Closing the channel is the whole of the stop signal.
        self.jobs.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The worker's loop: one engine, then one face per job until the queue closes.
fn run(jobs: Receiver<FaceJob>, done: Sender<FaceDone>) {
    let mut text = TextEngine::new();

    while let Ok(job) = jobs.recv() {
        let width = u32::from(job.image.width()).max(1);
        let height = u32::from(job.image.height()).max(1);
        let picture = plugintile::face(
            width,
            height,
            &job.spec,
            &job.reading,
            &job.family,
            &mut text,
        );
        let jpeg = match plugintile::encode(job.image, &job.spec, &picture) {
            Some(jpeg) => Ok(jpeg),
            None => Err(format!(
                "This deck's tile ({} bytes) could not hold the reading as a picture.",
                job.image.max_bytes()
            )),
        };

        // Nobody is listening any more: the window is gone.
        if done
            .send(FaceDone {
                key: job.key,
                plugin: job.plugin,
                cell: job.cell,
                reading: job.reading,
                picture,
                jpeg,
            })
            .is_err()
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ss550_plugin_api::{CellSpec, PluginId};

    fn job(key: u16) -> FaceJob {
        FaceJob {
            key,
            plugin: PluginId::parse("system-stats").expect("a usable id"),
            cell: CellId::parse("cpu.usage").expect("a usable id"),
            spec: CellSpec::new(
                PluginId::parse("system-stats").expect("a usable id"),
                CellId::parse("cpu.usage").expect("a usable id"),
                "CPU",
                "Processor load, every core together",
                [0x38, 0xBD, 0xF8],
            ),
            reading: Reading::percent(42.0).with_detail("12 threads"),
            family: String::new(),
            image: ImageSpec::new(112, 112, 4096, deck_api::ImageFormat::Jpeg),
        }
    }

    #[test]
    fn a_queued_face_comes_back_with_its_bytes_and_its_pixels() {
        let worker = FaceWorker::start();
        assert!(worker.is_alive());
        assert!(worker.submit(job(3)));

        // Composing a face reads the fonts on the worker's own thread, so the
        // wait is generous and bounded: a broken worker fails the test instead of
        // hanging it.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut answer = None;
        while std::time::Instant::now() < deadline {
            if let Some(done) = worker.try_recv() {
                answer = Some(done);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }

        let done = answer.expect("the worker answered");
        assert_eq!(done.key, 3);
        assert_eq!(done.picture.dimensions(), (112, 112));
        let jpeg = done.jpeg.expect("a payload");
        assert!(jpeg.starts_with(&[0xFF, 0xD8]));
    }

    #[test]
    fn a_face_that_cannot_fit_its_deck_answers_with_the_reason() {
        let worker = FaceWorker::start();
        let mut small = job(1);
        // A tile that cannot hold any JPEG at all: the answer is a sentence, not
        // a panic on the worker thread.
        small.image = ImageSpec::new(112, 112, 16, deck_api::ImageFormat::Jpeg);
        assert!(worker.submit(small));

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            if let Some(done) = worker.try_recv() {
                assert!(done.jpeg.is_err(), "16 bytes cannot hold a face");
                assert!(done.picture.dimensions() == (112, 112), "the pixels remain");
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("the worker did not answer");
    }
}
