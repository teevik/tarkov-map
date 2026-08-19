//! Effect Runner (ADR-0002): the one place Effects become tasks over the ports
//! and Messages come back. Cancellation by named slot; all tasks in a JoinSet.

use std::sync::Arc;

use futures::StreamExt;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::domain::MapId;
use crate::model::{Effect, Msg};
use crate::ports::{ImageDecoder, Ports, PositionSource};

pub type Wake = Arc<dyn Fn() + Send + Sync>;

pub struct Runner<P: Ports> {
    ports: Arc<P>,
    handle: Handle,
    tx: mpsc::UnboundedSender<Msg>,
    wake: Wake,
    tasks: JoinSet<()>,
    decode: Option<(MapId, CancellationToken)>,
    watch: Option<CancellationToken>,
}

impl<P: Ports> Runner<P> {
    pub fn new(ports: Arc<P>, handle: Handle, wake: Wake) -> (Self, mpsc::UnboundedReceiver<Msg>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Self {
                ports,
                handle,
                tx,
                wake,
                tasks: JoinSet::new(),
                decode: None,
                watch: None,
            },
            rx,
        )
    }

    pub fn run(&mut self, effect: Effect) {
        match effect {
            Effect::DecodeImage(map) => self.decode(map),
            Effect::WatchScreenshots(dir) => self.watch(dir),
            Effect::StopWatching => {
                if let Some(t) = self.watch.take() {
                    t.cancel();
                }
            }
            Effect::PersistSettings(_) => { /* SettingsStore port: not in this skeleton */ }
            Effect::PresentImage { .. } => {
                unreachable!("PresentImage is app-intercepted and never reaches the runner")
            }
        }
    }

    fn post(tx: &mpsc::UnboundedSender<Msg>, wake: &Wake, msg: Msg) {
        if tx.send(msg).is_ok() {
            wake();
        }
    }

    fn decode(&mut self, map: MapId) {
        if let Some((_, t)) = self.decode.take() {
            t.cancel(); // at most one decode in flight (spawn_blocking can't be aborted: model also drops stale results)
        }
        let token = CancellationToken::new();
        self.decode = Some((map.clone(), token.clone()));
        let (ports, tx, wake, handle) = (
            self.ports.clone(),
            self.tx.clone(),
            self.wake.clone(),
            self.handle.clone(),
        );
        self.tasks.spawn_on(
            async move {
                let key = crate::domain::MapImageKey(format!("maps/{map}.bc7z")); // skeleton: real one looks up catalog
                let m = map.clone();
                let result = tokio::select! {
                    _ = token.cancelled() => return,
                    r = handle.spawn_blocking(move || ports.decoder().decode(&key)) => r,
                };
                let msg = match result {
                    Ok(Ok(image)) => Msg::ImageDecoded { map: m, image },
                    Ok(Err(error)) => Msg::ImageDecodeFailed { map: m, error },
                    Err(_join) => return,
                };
                Self::post(&tx, &wake, msg);
            },
            &self.handle,
        );
    }

    fn watch(&mut self, dir: std::path::PathBuf) {
        if let Some(t) = self.watch.take() {
            t.cancel();
        }
        let token = CancellationToken::new();
        self.watch = Some(token.clone());
        let (ports, tx, wake) = (self.ports.clone(), self.tx.clone(), self.wake.clone());
        self.tasks.spawn_on(
            async move {
                let mut events = std::pin::pin!(ports.positions().observe(dir));
                loop {
                    tokio::select! {
                        _ = token.cancelled() => break,
                        next = events.next() => match next {
                            Some(ev) => Self::post(&tx, &wake, Msg::PositionEvent(ev)),
                            None => break,
                        },
                    }
                }
            },
            &self.handle,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::model::Msg;
    use crate::ports::fakes::{FakeDecoder, FakePorts, FakePositionSource};
    use crate::ports::{PositionEvent, SourceKind};

    fn runner(
        ports: FakePorts,
    ) -> (
        Runner<FakePorts>,
        mpsc::UnboundedReceiver<Msg>,
        Arc<AtomicUsize>,
    ) {
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = wakes.clone();
        let (r, rx) = Runner::new(
            Arc::new(ports),
            Handle::current(),
            Arc::new(move || {
                w.fetch_add(1, Ordering::SeqCst);
            }),
        );
        (r, rx, wakes)
    }

    #[tokio::test]
    async fn watch_forwards_stream_items_as_messages_and_wakes() {
        let ports = FakePorts {
            positions: FakePositionSource::scripted(vec![PositionEvent::Started(SourceKind::Demo)]),
            decoder: FakeDecoder::instant(),
        };
        let (mut r, mut rx, wakes) = runner(ports);
        r.run(Effect::WatchScreenshots("/shots".into()));
        let msg = rx.recv().await.unwrap();
        assert!(matches!(
            msg,
            Msg::PositionEvent(PositionEvent::Started(SourceKind::Demo))
        ));
        assert_eq!(wakes.load(Ordering::SeqCst), 1);
        assert_eq!(
            r.ports.positions().observed.lock().unwrap().as_slice(),
            &[std::path::PathBuf::from("/shots")]
        );
    }

    #[tokio::test]
    async fn decode_posts_decoded_and_stale_decode_is_cancelled() {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let mut decoder = FakeDecoder::instant();
        decoder.gate = Some(gate.clone());
        let ports = FakePorts {
            positions: FakePositionSource::default(),
            decoder,
        };
        let (mut r, mut rx, _) = runner(ports);
        r.run(Effect::DecodeImage(MapId::new("a"))); // blocks in spawn_blocking until gate
        r.run(Effect::DecodeImage(MapId::new("b"))); // cancels the first slot, blocks too
        gate.add_permits(2); // releases both blocked decodes; the cancelled one must never surface
        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(msg, Msg::ImageDecoded { map, .. } if map == MapId::new("b")));
        // the cancelled "a" never arrives
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
                .await
                .is_err()
        );
    }
}
