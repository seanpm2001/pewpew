mod csv_reader;
mod json_reader;
mod line_reader;

use self::{csv_reader::CsvReader, json_reader::JsonReader, line_reader::LineReader};

use crate::error::TestError;
use crate::line_writer::MsgType;
use crate::util::json_value_to_string;
use crate::TestEndReason;

use ether::Either3;
use futures::{
    channel::mpsc::{self, channel, Sender as FCSender},
    executor::block_on,
    sink::{Sink, SinkExt},
    stream, Stream, StreamExt, TryStreamExt,
};
use serde_json as json;
use tokio::{sync::broadcast, task::spawn_blocking};

use std::{
    borrow::Cow,
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicIsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

#[derive(Clone)]
pub struct Provider {
    pub auto_return: Option<config::EndpointProvidesSendOptions>,
    pub rx: channel::Receiver<json::Value>,
    pub tx: channel::Sender<json::Value>,
    pub on_demand: channel::OnDemandReceiver<json::Value>,
}

impl Provider {
    fn new(
        auto_return: Option<config::EndpointProvidesSendOptions>,
        rx: channel::Receiver<json::Value>,
        tx: channel::Sender<json::Value>,
    ) -> Self {
        Provider {
            auto_return,
            on_demand: channel::OnDemandReceiver::new(&rx),
            rx,
            tx,
        }
    }
}

pub fn file(
    mut template: config::FileProvider,
    test_killer: broadcast::Sender<Result<TestEndReason, TestError>>,
) -> Result<Provider, TestError> {
    let file = std::mem::take(&mut template.path);
    let file2 = file.clone();
    let stream = match template.format {
        config::FileFormat::Csv => Either3::A(
            CsvReader::new(&template, &file)
                .map_err(|e| TestError::CannotOpenFile(file.into(), e.into()))?
                .into_stream(),
        ),
        config::FileFormat::Json => Either3::B(
            JsonReader::new(&template, &file)
                .map_err(|e| TestError::CannotOpenFile(file.into(), e.into()))?
                .into_stream(),
        ),
        config::FileFormat::Line => Either3::C(
            LineReader::new(&template, &file)
                .map_err(|e| TestError::CannotOpenFile(file.into(), e.into()))?
                .into_stream(),
        ),
    };
    let (tx, rx) = channel::channel(template.buffer);
    let tx2 = tx.clone();
    let prime_tx = async move {
        let r = stream
            .map_err(move |e| {
                let e = TestError::FileReading(file2.clone(), e.into());
                channel::ChannelClosed::wrapped(e)
            })
            .forward(tx2)
            .await;
        if let Err(e) = r {
            if let Some(e) = e.inner_cast() {
                let _ = test_killer.send(Err(*e));
            }
        }
    };

    tokio::spawn(prime_tx);
    Ok(Provider::new(template.auto_return, rx, tx))
}

pub fn response(template: config::ResponseProvider) -> Provider {
    let (tx, rx) = channel::channel(template.buffer);
    Provider::new(template.auto_return, rx, tx)
}

pub fn literals(list: config::StaticList) -> Provider {
    let rs = stream::iter(list.into_iter().map(Ok));
    let (tx, rx) = channel::channel(config::Limit::auto());
    let tx2 = tx.clone();
    let prime_tx = rs.forward(tx2);
    tokio::spawn(prime_tx);
    Provider::new(None, rx, tx)
}

pub fn range(range: config::RangeProvider) -> Provider {
    let (tx, rx) = channel::channel(config::Limit::auto());
    let prime_tx = stream::iter(range.0.map(|v| Ok(v.into()))).forward(tx.clone());
    tokio::spawn(prime_tx);
    Provider::new(None, rx, tx)
}

#[derive(Clone)]
pub struct Logger {
    limit: Option<Arc<AtomicIsize>>,
    pretty: bool,
    test_killer: Option<broadcast::Sender<Result<TestEndReason, TestError>>>,
    writer: FCSender<MsgType>,
}

impl Logger {
    fn json_to_msg_type(&self, j: json::Value) -> MsgType {
        let s = if self.pretty && !j.is_string() {
            format!("{:#}\n", j)
        } else {
            let mut s = json_value_to_string(Cow::Owned(j)).into_owned();
            s.push('\n');
            s
        };
        MsgType::Other(s)
    }
}

impl Sink<json::Value> for Logger {
    type Error = mpsc::SendError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = Pin::into_inner(self);
        Pin::new(&mut this.writer).poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: json::Value) -> Result<(), Self::Error> {
        let msg = self.json_to_msg_type(item);
        if let Some(limit) = &self.limit {
            let i = limit.fetch_sub(1, Ordering::Release);
            if i <= 0 {
                if let Some(killer) = &self.test_killer {
                    let _ = killer.send(Ok(TestEndReason::KilledByLogger));
                }
                self.writer.disconnect();
            }
        }
        self.writer.start_send(msg)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = Pin::into_inner(self);
        Pin::new(&mut this.writer).poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = Pin::into_inner(self);
        Pin::new(&mut this.writer).poll_close(cx)
    }
}

pub fn logger(
    template: config::Logger,
    test_killer: &broadcast::Sender<Result<TestEndReason, TestError>>,
    writer: FCSender<MsgType>,
) -> Logger {
    let pretty = template.pretty;
    let kill = template.kill;

    let test_killer = if kill {
        Some(test_killer.clone())
    } else {
        None
    };

    let limit = if kill && template.limit.is_none() {
        Some(1)
    } else {
        template.limit
    }
    .map(|limit| Arc::new(AtomicIsize::new(limit as isize)));

    Logger {
        limit,
        pretty,
        test_killer,
        writer,
    }
}

fn into_stream<I: Iterator<Item = Result<json::Value, io::Error>> + Send + 'static>(
    iter: I,
) -> impl Stream<Item = Result<json::Value, io::Error>> {
    let (mut tx, rx) = channel(5);
    spawn_blocking(move || {
        for value in iter {
            let value = value.map_err(|e| io::Error::new(io::ErrorKind::Other, e));
            // this should only error when the receiver is dropped, and in that case we can stop sending
            if block_on(tx.send(value)).is_err() {
                break;
            }
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::line_writer::blocking_writer;

    use config::FromYaml;
    use futures::executor::{block_on, block_on_stream};
    use futures_timer::Delay;
    use json::json;
    use test_common::TestWriter;
    use tokio::runtime::Runtime;

    use std::time::Duration;

    #[test]
    fn range_provider_works() {
        let mut rt = Runtime::new().unwrap();
        rt.block_on(async move {
            let range_params = r#"
                start: 0
                end: 20
            "#;
            let range_params =
                config::RangeProviderPreProcessed::from_yaml_str(range_params).unwrap();
            let p = range(range_params.into());
            let expect: Vec<_> = (0..=20).collect();

            let Provider { rx, tx, .. } = p;
            drop(tx);

            let values: Vec<_> = rx.map(|j| j.as_u64().unwrap()).collect().await;

            assert_eq!(values, expect, "first");

            let range_params = r#"
                start: 0
                end: 20
                step: 2
            "#;
            let range_params =
                config::RangeProviderPreProcessed::from_yaml_str(range_params).unwrap();
            let p = range(range_params.into());

            let expect: Vec<_> = (0..=20).step_by(2).collect();

            let Provider { rx, tx, .. } = p;
            drop(tx);

            let values: Vec<_> = rx.map(|j| j.as_u64().unwrap()).collect().await;

            assert_eq!(values, expect, "second");

            let range_params = r#"
                    start: 0
                    end: 20
                    repeat: true
                "#;
            let range_params =
                config::RangeProviderPreProcessed::from_yaml_str(range_params).unwrap();
            let p = range(range_params.into());

            let expect: Vec<_> = (0..=20).cycle().take(100).collect();

            let Provider { rx, tx, .. } = p;
            drop(tx);

            let values: Vec<_> = rx.take(100).map(|j| j.as_u64().unwrap()).collect().await;

            assert_eq!(values, expect, "third");
        });
    }

    #[test]
    fn literals_provider_works() {
        let mut rt = Runtime::new().unwrap();
        rt.block_on(async move {
            let jsons = vec![json!(1), json!(2), json!(3)];
            let esl = config::ExplicitStaticList {
                values: jsons.clone(),
                repeat: false,
                random: false,
            };

            let p = literals(esl.into());
            let expect = jsons.clone();

            let Provider { rx, tx, .. } = p;
            drop(tx);

            let values: Vec<_> = rx.collect().await;

            assert_eq!(values, expect, "first");

            let esl = config::ExplicitStaticList {
                values: jsons.clone(),
                repeat: false,
                random: true,
            };

            let p = literals(esl.into());
            let mut expect: Vec<_> = jsons.iter().map(|j| j.as_u64().unwrap()).collect();

            let Provider { rx, tx, .. } = p;
            drop(tx);

            let mut values: Vec<_> = rx.map(|j| j.as_u64().unwrap()).collect().await;

            expect.sort_unstable();
            values.sort_unstable();

            assert_eq!(values, expect, "second");

            let esl = config::ExplicitStaticList {
                values: jsons.clone(),
                repeat: true,
                random: false,
            };

            let p = literals(esl.into());
            let expect: Vec<_> = jsons.clone().into_iter().cycle().take(100).collect();

            let values: Vec<_> = p.rx.take(100).collect().await;

            assert_eq!(values, expect, "third");

            let esl = config::ExplicitStaticList {
                values: jsons.clone(),
                repeat: true,
                random: true,
            };

            let p = literals(esl.into());
            let mut expect: Vec<_> = jsons
                .iter()
                .cycle()
                .take(100)
                .map(|j| j.as_u64().unwrap())
                .collect();

            let mut values: Vec<_> = p.rx.take(100).map(|j| j.as_u64().unwrap()).collect().await;

            assert_ne!(values, expect, "fourth");

            expect.sort_unstable();
            expect.dedup();
            values.sort_unstable();
            values.dedup();

            assert_eq!(values, expect, "fifth");
        });
    }

    #[test]
    fn response_provider_works() {
        let jsons = vec![json!(1), json!(2), json!(3)];
        let rp = config::ResponseProvider {
            auto_return: None,
            buffer: config::Limit::auto(),
        };
        let mut p = response(rp);
        for value in &jsons {
            let _ = block_on(p.tx.send(value.clone()));
        }

        let expects = jsons;

        let Provider { rx, tx, .. } = p;
        drop(tx);

        let values: Vec<_> = block_on_stream(rx).collect();

        assert_eq!(values, expects);
    }

    #[test]
    fn basic_logger_works() {
        let mut rt = Runtime::new().unwrap();
        rt.block_on(async move {
            let logger_params = r#"
                to: ""
                kill: true
            "#;
            let logger_params = config::FromYaml::from_yaml_str(logger_params).unwrap();
            let (logger_params, _) = config::Logger::from_pre_processed(
                logger_params,
                &Default::default(),
                &mut Default::default(),
            )
            .unwrap();
            let (test_killer, mut test_killed_rx) = broadcast::channel(1);
            let writer = TestWriter::new();
            let (writer_channel, _) =
                blocking_writer(writer.clone(), test_killer.clone(), "".into());

            let mut tx = logger(logger_params, &test_killer, writer_channel);

            for value in vec![json!(1), json!(2)] {
                let _ = tx.send(value).await;
            }

            // add slight delay because writing to the channel does not mean it's yet written to the file
            Delay::new(Duration::from_millis(100)).await;

            let left = writer.get_string();
            let right = "1\n";
            assert_eq!(left, right, "value in writer should match");

            let check = if let Ok(Ok(TestEndReason::KilledByLogger)) = test_killed_rx.try_recv() {
                true
            } else {
                false
            };
            assert!(check, "test should be killed");
        });
    }

    #[test]
    fn basic_logger_works_with_large_data() {
        let mut rt = Runtime::new().unwrap();
        rt.block_on(async move {
            let logger_params = r#"
                to: ""
            "#;
            let logger_params = config::FromYaml::from_yaml_str(logger_params).unwrap();
            let (logger_params, _) = config::Logger::from_pre_processed(
                logger_params,
                &Default::default(),
                &mut Default::default(),
            )
            .unwrap();
            let (test_killer, mut test_killed_rx) = broadcast::channel(1);
            let writer = TestWriter::new();
            let (writer_channel, _) = blocking_writer(writer.clone(), test_killer.clone(), "".into());

            let mut tx = logger(logger_params, &test_killer, writer_channel);

            let right: String = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.".repeat(1000);

            let _ = tx.send(right.clone().into()).await;

            // add slight delay because writing to the channel does not mean it's yet written to the file
            Delay::new(Duration::from_millis(100)).await;

            let left = writer.get_string();
            assert_eq!(left, format!("{}\n", right), "value in writer should match");

            let check = if let Ok(Err(_)) = test_killed_rx.try_recv() {
                false
            } else {
                true
            };
            assert!(check, "test should not be killed");
        });
    }

    #[test]
    fn basic_logger_works_with_would_block() {
        let mut rt = Runtime::new().unwrap();
        rt.block_on(async move {
            let logger_params = r#"
                to: ""
            "#;
            let logger_params = config::FromYaml::from_yaml_str(logger_params).unwrap();
            let (logger_params, _) = config::Logger::from_pre_processed(
                logger_params,
                &Default::default(),
                &mut Default::default(),
            )
            .unwrap();
            let (test_killer, mut test_killed_rx) = broadcast::channel(1);
            let writer = TestWriter::new();
            writer.do_would_block_on_next_write();
            let (writer_channel, _) =
                blocking_writer(writer.clone(), test_killer.clone(), "".into());

            let mut tx = logger(logger_params, &test_killer, writer_channel);

            for value in vec![json!(1), json!(2)] {
                let _ = tx.send(value).await;
            }

            // add slight delay because writing to the channel does not mean it's yet written to the file
            Delay::new(Duration::from_millis(100)).await;

            let left = writer.get_string();
            let right = "1\n2\n";
            assert_eq!(left, right, "value in writer should match");

            let check = if let Ok(Err(_)) = test_killed_rx.try_recv() {
                false
            } else {
                true
            };
            assert!(check, "test should not be killed");
        });
    }

    #[test]
    fn logger_limit_works() {
        let mut rt = Runtime::new().unwrap();
        rt.block_on(async move {
            let logger_params = r#"
                to: ""
                limit: 1
            "#;
            let logger_params = config::FromYaml::from_yaml_str(logger_params).unwrap();
            let (logger_params, _) = config::Logger::from_pre_processed(
                logger_params,
                &Default::default(),
                &mut Default::default(),
            )
            .unwrap();
            let (test_killer, mut test_killed_rx) = broadcast::channel(1);
            let writer = TestWriter::new();
            let (writer_channel, _) =
                blocking_writer(writer.clone(), test_killer.clone(), "".into());

            let mut tx = logger(logger_params, &test_killer, writer_channel);

            for value in vec![json!(1), json!(2)] {
                let _ = tx.send(value).await;
            }

            // add slight delay because writing to the channel does not mean it's yet written to the file
            Delay::new(Duration::from_millis(100)).await;

            let left = writer.get_string();
            let right = "1\n";
            assert_eq!(left, right, "value in writer should match");

            let check = test_killed_rx.try_recv().is_err();
            assert!(check, "test should not be killed");
        });
    }

    #[test]
    fn logger_pretty_works() {
        let mut rt = Runtime::new().unwrap();
        rt.block_on(async move {
            let logger_params = r#"
                to: ""
                pretty: true
            "#;
            let logger_params = config::FromYaml::from_yaml_str(logger_params).unwrap();
            let (logger_params, _) = config::Logger::from_pre_processed(
                logger_params,
                &Default::default(),
                &mut Default::default(),
            )
            .unwrap();
            let (test_killer, mut test_killed_rx) = broadcast::channel(1);
            let writer = TestWriter::new();
            let (writer_channel, _) =
                blocking_writer(writer.clone(), test_killer.clone(), "".into());

            let mut tx = logger(logger_params, &test_killer, writer_channel);

            for value in vec![json!({"foo": [1, 2, 3]}), json!(2)] {
                let _ = tx.send(value).await;
            }
            // add slight delay because writing to the channel does not mean it's yet written to the file
            Delay::new(Duration::from_millis(100)).await;

            let left = writer.get_string();
            let right = "{\n  \"foo\": [\n    1,\n    2,\n    3\n  ]\n}\n2\n";
            assert_eq!(left, right, "value in writer should match");

            let check = test_killed_rx.try_recv().is_err();
            assert!(check, "test should not be killed");
        });
    }
}
