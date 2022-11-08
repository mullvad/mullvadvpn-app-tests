use lazy_static::lazy_static;
use log::{Level, LevelFilter, Metadata, Record, SetLoggerError};
use test_rpc::logging::Output;
use tokio::sync::{
    broadcast::{channel, Receiver, Sender},
    Mutex,
};

const MAX_OUTPUT_BUFFER: usize = 10_000;
lazy_static! {
    pub static ref LOGGER: StdOutBuffer = {
        let (sender, listener) = channel(MAX_OUTPUT_BUFFER);
        StdOutBuffer(Mutex::new(listener), sender)
    };
}

pub struct StdOutBuffer(pub Mutex<Receiver<Output>>, pub Sender<Output>);

impl log::Log for StdOutBuffer {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            match record.metadata().level() {
                Level::Error => {
                    self.1
                        .send(Output::Error(format!("{}", record.args())))
                        .unwrap();
                }
                Level::Warn => {
                    self.1
                        .send(Output::Warning(format!("{}", record.args())))
                        .unwrap();
                }
                Level::Info => {
                    if !record.metadata().target().contains("tarpc") {
                        self.1
                            .send(Output::Info(format!("{}", record.args())))
                            .unwrap();
                    }
                }
                _ => (),
            }
            println!("{}", record.args());
        }
    }

    fn flush(&self) {}
}

pub fn init_logger() -> Result<(), SetLoggerError> {
    log::set_logger(&*LOGGER).map(|()| log::set_max_level(LevelFilter::Info))
}
