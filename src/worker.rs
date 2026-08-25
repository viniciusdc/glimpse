//! The recording worker: a thread that exclusively owns a [`Recorder`].
//!
//! GTK must stay responsive while a recording stops, and stopping means waiting
//! for a subprocess to finalise a container — which can take long enough to be
//! visible. So the child lives on its own thread and the UI polls for results.
//!
//! The ownership rule that makes shutdown safe: **the worker owns the child, and
//! dropping the worker reaps it.** If the UI disappears — window closed, panic,
//! application quit — the command channel closes, the thread sees that, and
//! terminates the child rather than leaving it recording into a temp directory
//! nobody will ever look at.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;

use crate::capture::{Recorder, RecorderConfig, Workspace};
use crate::session::CapturedVideo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopKind {
    /// Finish cleanly; the output is wanted.
    Finish,
    /// Kill it; the output is not wanted or not trustworthy.
    Abort,
}

/// What the worker reports back. One of these arrives per session, exactly once.
#[derive(Debug)]
pub enum WorkerEvent {
    Finished(CapturedVideo),
    Failed(String),
    /// The recording was aborted; nothing usable was produced.
    Aborted,
}

pub struct RecordingWorker {
    stop_tx: Option<Sender<StopKind>>,
    events: Receiver<WorkerEvent>,
    handle: Option<JoinHandle<()>>,
}

impl RecordingWorker {
    /// Spawn the worker and start recording.
    ///
    /// Returns immediately — a spawn failure arrives as [`WorkerEvent::Failed`]
    /// rather than as an error here, so the caller has exactly one place to
    /// handle recording failures instead of two.
    pub fn start(config: RecorderConfig, workspace: Workspace) -> Self {
        let (stop_tx, stop_rx) = mpsc::channel::<StopKind>();
        let (evt_tx, events) = mpsc::channel::<WorkerEvent>();

        let handle = std::thread::spawn(move || {
            let recorder = match Recorder::start(&config, workspace) {
                Ok(r) => r,
                Err(e) => {
                    let _ = evt_tx.send(WorkerEvent::Failed(format!("{e:#}")));
                    return;
                }
            };

            // A closed channel means the UI is gone. That is an abort, not a
            // clean finish — there is nobody left to hand a recording to.
            let event = match stop_rx.recv() {
                Ok(StopKind::Finish) => match recorder.stop() {
                    Ok(video) => WorkerEvent::Finished(video),
                    Err(e) => WorkerEvent::Failed(format!("{e:#}")),
                },
                Ok(StopKind::Abort) | Err(_) => match recorder.terminate() {
                    Ok(()) => WorkerEvent::Aborted,
                    Err(e) => WorkerEvent::Failed(format!("{e:#}")),
                },
            };
            let _ = evt_tx.send(event);
        });

        Self {
            stop_tx: Some(stop_tx),
            events,
            handle: Some(handle),
        }
    }

    /// Ask for a clean stop. The result arrives via [`Self::poll`].
    pub fn stop(&self) {
        if let Some(tx) = &self.stop_tx {
            let _ = tx.send(StopKind::Finish);
        }
    }

    /// Kill the recording. Used when the geometry drifted or the user cancelled.
    pub fn abort(&self) {
        if let Some(tx) = &self.stop_tx {
            let _ = tx.send(StopKind::Abort);
        }
    }

    /// Non-blocking check for the worker's result.
    pub fn poll(&self) -> Option<WorkerEvent> {
        match self.events.try_recv() {
            Ok(e) => Some(e),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }
}

impl Drop for RecordingWorker {
    /// Guarantee the child is reaped before this returns.
    ///
    /// Dropping the sender closes the command channel, which the thread reads as
    /// an abort. Joining then blocks until the child has actually been killed and
    /// waited on — brief, and bounded by [`crate::capture`]'s own escalation, so
    /// application shutdown cannot outrun the process it is responsible for.
    fn drop(&mut self) {
        self.stop_tx.take();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
