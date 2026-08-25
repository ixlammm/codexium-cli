use tracing::dispatcher::DefaultGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub struct TestTracingContext {
    _guard: DefaultGuard,
}

pub fn install_test_tracing(_tracer_name: &str) -> TestTracingContext {
    let subscriber = tracing_subscriber::registry().with(tracing_subscriber::fmt::layer());

    TestTracingContext {
        _guard: subscriber.set_default(),
    }
}
