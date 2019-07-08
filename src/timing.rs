use hdrhistogram::Histogram;
use tokio::prelude::*;
use tracing::Dispatch;
use tracing_timing::Builder;

fn main() {
    let runtime = std::time::Duration::from_secs(120);
    let start = std::time::Instant::now();
    let subscriber =
        Builder::default().build(|| Histogram::new_with_bounds(10, 1_000_000, 4).unwrap());
    let dispatcher = Dispatch::new(subscriber);

    tracing::dispatcher::set_global_default(dispatcher.clone()).unwrap();
    tokio::run(future::lazy(move || {
        for ti in 0..150 {
            tokio::spawn(
                stream::iter_ok((0..).take_while(move |_| start.elapsed() < runtime))
                    .fold((), move |_, ri| {
                        let span = if ri % 1_000 == ti {
                            tracing::trace_span!("span")
                        } else {
                            tracing::Span::none()
                        };
                        let _guard = span.enter();
                        tracing::trace!("cache1");
                        tracing::trace!("cache2");
                        tracing::trace!("begin");
                        let (tx, rx) = tokio_sync::oneshot::channel();
                        tracing::trace!("then");
                        tokio::spawn(
                            tokio::timer::Delay::new(
                                std::time::Instant::now() + std::time::Duration::from_millis(1),
                            )
                            .map_err(|_| ())
                            .and_then(|_| tx.send(())),
                        );
                        rx.map_err(|_| ())
                    })
                    .map(|_| ()),
            );
        }
        Ok(())
    }));

    dispatcher
        .downcast_ref::<tracing_timing::TimingSubscriber>()
        .unwrap()
        .force_synchronize();
    dispatcher
        .downcast_ref::<tracing_timing::TimingSubscriber>()
        .unwrap()
        .with_histograms(|hs| {
            let hs = hs.get_mut("span").unwrap();
            for (e, h) in hs {
                println!("{}:", e);
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
                        "{:4}µs | {:40} | {:4.1}th %-ile",
                        (v.value_iterated_to() + 1) / 1_000,
                        "*".repeat(
                            (v.count_since_last_iteration() as f64 * 40.0 / h.len() as f64)
                                .ceil()
                                .max(0.0) as usize
                        ),
                        v.percentile(),
                    );
                }
            }
        });
}
