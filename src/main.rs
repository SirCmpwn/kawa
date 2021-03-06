#![cfg_attr(feature = "nightly", feature(alloc_system))]
#[cfg(feature = "nightly")]
extern crate alloc_system;

#[macro_use]
extern crate slog;
extern crate slog_term;
extern crate slog_async;
extern crate toml;
extern crate serde;
extern crate serde_json;
extern crate reqwest;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate rouille;
extern crate amy;
extern crate httparse;
extern crate url;

extern crate kaeru;

mod radio;
mod config;
mod api;
mod queue;
mod util;
mod tc_queue;
mod prebuffer;
mod broadcast;

use std::env;
use std::sync::{Arc, Mutex, mpsc};
use std::io::{Read};
use std::collections::HashMap;

lazy_static! {
    pub static ref LOG: slog::Logger = {
        use slog::Drain;

        let decorator = slog_term::TermDecorator::new().stderr().build();
        let drain = slog_term::FullFormat::new(decorator).build().fuse();
        let drain = slog_async::Async::new(drain).build().fuse();
        slog::Logger::root(drain, o!())
    };
}

fn main() {

    let root_log = LOG.clone();

    #[cfg(feature = "nightly")]
    info!(root_log, "Using system alloc");

    info!(root_log, "Initializing ffmpeg");
    kaeru::init();

    let path = env::args().nth(1).unwrap_or("config.toml".to_owned());
    let mut s = String::new();
    if let Ok(mut f) = std::fs::File::open(&path) {
        if f.read_to_string(&mut s).is_err() {
            crit!(root_log, "Config file could not be read!");
            return;
        }
    } else {
        crit!(root_log, "A config file path must be passed as argv[1] or must exist as ./config.toml");
        return;
    }

    info!(root_log, "Initializing config");
    let config = match config::parse_config(&s) {
        Ok(c) => c,
        Err(e) => {
            crit!(root_log, "Failed to parse config: {}", e);
            return;
        }
    };

    let api_log = root_log.new(o!("thread" => "api"));
    let queue_log = root_log.new(o!("queue" => ()));
    let radio_log = root_log.new(o!("thread" => "radio"));

    info!(root_log, "Starting");
    let queue = Arc::new(Mutex::new(queue::Queue::new(config.clone(), queue_log)));
    let listeners = Arc::new(Mutex::new(HashMap::new()));
    let (tx, rx) = mpsc::channel();
    let btx = broadcast::start(&config, listeners.clone(), root_log.new(o!("thread" => "broadcast")));
    api::start_api(config.api.clone(), queue.clone(), listeners, tx, api_log);
    radio::start_streams(config.clone(), queue, rx, btx, radio_log);
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::kaeru::{Input, Output, GraphBuilder};
    use std::{thread, io};
    use std::fs::File;

    #[ignore]
    #[test]
    fn test_tc() {
        #[cfg(feature = "nightly")]
        info!(LOG, "Using system alloc");
        kaeru::init();
        tc();
        thread::sleep_ms(30000);
    }

    struct Dum(usize);

    impl io::Write for Dum {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0 += buf.len();
            return Ok(buf.len());
            if self.0 < 4096 * 32 {
                Ok(buf.len())
            } else {
                Err(io::Error::new(io::ErrorKind::Other, "oh no!"))
            }
        }

        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }

    fn tc() -> kaeru::Result<()> {
        let fin = File::open("/tmp/in.mp3").unwrap();
        let i = Input::new(fin, "mp3")?;

        let o1 = Output::new_writer(Dum(0), "mp3", kaeru::AVCodecID::AV_CODEC_ID_MP3, Some(192))?;
        let o2 = Output::new_writer(Dum(0), "ogg", kaeru::AVCodecID::AV_CODEC_ID_OPUS, Some(192))?;
        let o3 = Output::new_writer(Dum(0), "ogg", kaeru::AVCodecID::AV_CODEC_ID_FLAC, None)?;
        let mut gb = GraphBuilder::new(i)?;
        gb.add_output(o1)?.add_output(o2)?.add_output(o3)?;
        let g = gb.build()?;
        let gt = thread::spawn(move || g.run().unwrap());
        gt.join();
        Ok(())
    }
}
