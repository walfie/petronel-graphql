use slog::Drain;
use std::fmt::Debug;

pub enum Either<A, B> {
    A(A),
    B(B),
}

pub fn logger(json: bool) -> slog::Logger {
    let drain = drain(json).fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    slog::Logger::root(drain, slog::o!())
}

pub fn drain(json: bool) -> impl Drain<Ok = (), Err = std::io::Error> {
    if json {
        Either::A(slog_json::Json::default(std::io::stderr()))
    } else {
        let decorator = slog_term::TermDecorator::new().build();
        let drain = slog_term::FullFormat::new(decorator).build();
        Either::B(drain)
    }
}

impl<A, B> Drain for Either<A, B>
where
    A: Drain,
    B: Drain<Ok = A::Ok, Err = A::Err>,
    A::Err: Debug,
{
    type Ok = A::Ok;
    type Err = A::Err;

    fn log(
        &self,
        record: &slog::Record,
        values: &slog::OwnedKVList,
    ) -> Result<Self::Ok, Self::Err> {
        match self {
            Either::A(logger) => logger.log(record, values),
            Either::B(logger) => logger.log(record, values),
        }
    }
}
