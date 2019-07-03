use tokio::prelude::*;
use hdrhistogram::{Histogram, SyncHistogram};
use futures::try_ready;

fn main() {
    let runtime = std::time::Duration::from_secs(10);
    let start = std::time::Instant::now();
    tokio::run(future::lazy(move || {
        let hist = Histogram::new_with_bounds(10_000, 3_000_000, 3).unwrap().into_sync();
        let ir = hist.recorder().into_idle();
        let (tx, rx) = tokio_sync::mpsc::channel(5);
        tokio::spawn(
            Svc(
                quanta::Clock::new(),
                Histogram::new_with_bounds(10_000, 3_000_000, 3).unwrap(),
                hist,
                rx,
            )
            .map_err(|e| {
                unimplemented!("{:?}", e);
            })
        );
        let time = quanta::Clock::new();
        for _ in 0..150 {
            let worker = tx.clone();
            let time = time.clone();
            let recorder = ir.recorder();
            tokio::spawn(
                stream::iter_ok(
                    (0..).take_while(move |_| start.elapsed() < runtime)
                )
                    .fold((time, recorder, worker), |(time, mut recorder, worker), _| {
                        let mut worker = Some(worker);
                        future::poll_fn(move || match worker.as_mut().unwrap().poll_ready().unwrap() {
                            Async::Ready(()) => Ok(Async::Ready(worker.take().unwrap())),
                            Async::NotReady => Ok(Async::NotReady),
                        })
                            .and_then(move |mut worker| {
                                let (tx, rx) = tokio_sync::oneshot::channel();
                                let sent = time.now();
                                worker.try_send((sent, tx)).unwrap();
                                rx.map(move |sent_from_worker| {
                                    recorder.saturating_record(time.now() - sent_from_worker);
                                    (time, recorder, worker)
                                })
                            })
                            .map_err(|e| {
                                unimplemented!("{:?}", e);
                            })
                    })
                    .map(|_| ())
            );
        }
        Ok(())
    }));
}

struct Svc(quanta::Clock, Histogram<u64>, SyncHistogram<u64>, tokio_sync::mpsc::Receiver<(u64, tokio_sync::oneshot::Sender<u64>)>);
impl Future for Svc {
    type Item = ();
    type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

    fn poll(&mut self) -> Poll<(), Self::Error> {
        while let Some((sent, tx)) = try_ready!(self.3.poll()) {
            let got = self.0.now();
            self.1.saturating_record(got - sent);
            let time = self.0.clone();
            std::thread::sleep(std::time::Duration::from_nanos(10_000));
            tokio::spawn(future::lazy(move || {
                tx.send(time.now()).map_err(|e| {
                    unimplemented!("{:?}", e);
                })
            }));
        }
        Ok(Async::Ready(()))
    }
}

impl Drop for Svc {
    fn drop(&mut self) {
        println!("{}[ client -> worker ]{}", "-".repeat(30), "-".repeat(30));
        let h = &self.1;
        println!(
            "mean: {:.1}µs, p50: {}µs, p90: {}µs, p99: {}µs, p999: {}µs, max: {}µs, #: {}",
            h.mean() / 1000.0,
            h.value_at_quantile(0.5) / 1_000,
            h.value_at_quantile(0.9) / 1_000,
            h.value_at_quantile(0.99) / 1_000,
            h.value_at_quantile(0.999) / 1_000,
            h.max() / 1_000,
            h.len()
        );
        for v in h.iter_log(1_000, 2.0) {
            println!(
                "{:4}µs - {:4}µs | {:40} | {:4.1}th %-ile",
                (v.value_iterated_to() + 1) / 2 / 1_000,
                (v.value_iterated_to() + 1) / 1_000,
                "*".repeat(
                    (v.count_since_last_iteration() as f64 * 40.0 / h.len() as f64).ceil().max(0.0) as usize
                ),
                v.percentile(),
            );
        }

        println!("");
        println!("{}[ worker -> client ]{}", "-".repeat(30), "-".repeat(30));
        let h = &mut self.2;
        h.refresh();
        println!(
            "mean: {:.1}µs, p50: {}µs, p90: {}µs, p99: {}µs, p999: {}µs, max: {}µs, #: {}",
            h.mean() / 1000.0,
            h.value_at_quantile(0.5) / 1_000,
            h.value_at_quantile(0.9) / 1_000,
            h.value_at_quantile(0.99) / 1_000,
            h.value_at_quantile(0.999) / 1_000,
            h.max() / 1_000,
            h.len()
        );
        for v in h.iter_log(1_000, 2.0) {
            println!(
                "{:4}µs - {:4}µs | {:40} | {:4.1}th %-ile",
                (v.value_iterated_to() + 1) / 2 / 1_000,
                (v.value_iterated_to() + 1) / 1_000,
                "*".repeat(
                    (v.count_since_last_iteration() as f64 * 40.0 / h.len() as f64).ceil().max(0.0) as usize
                ),
                v.percentile(),
            );
        }
    }
}
